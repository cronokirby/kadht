use crate::base::{BitKey, Node};
use crate::messages::{Header, Message, RPCPayload, TransactionID};
use crate::rand::rngs::ThreadRng;
use crate::rand::thread_rng;
use crate::routing::{KBucketInsert, RoutingTable};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Instant;

// How big to make our buckets
const K: usize = 20;
const BUF_SIZE: usize = 2048;

struct TransactionTable {
    transactions: HashMap<TransactionID, (Instant, BitKey)>,
}

impl TransactionTable {
    fn new() -> Self {
        TransactionTable {
            transactions: HashMap::new(),
        }
    }

    fn insert(&mut self, header: Header) {
        let expiration = (Instant::now(), header.node_id);
        self.transactions.insert(header.transaction_id, expiration);
    }

    fn contains(&self, transaction_id: TransactionID) -> bool {
        self.transactions.contains_key(&transaction_id)
    }

    fn remove(&mut self, transaction_id: TransactionID) -> bool {
        self.transactions.remove(&transaction_id).is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum QueryStatus {
    Empty,
    Started,
    Finished,
}

#[derive(Debug, Clone, Copy)]
struct NodeQuery {
    node: Node,
    status: QueryStatus,
    distance: u128,
}

impl NodeQuery {
    fn new(node: Node, target: BitKey) -> Self {
        NodeQuery {
            node,
            status: QueryStatus::Empty,
            distance: node.id.distance(target),
        }
    }
}

struct Query {
    target: BitKey,
    target_value: Option<String>,
    closest: Vec<NodeQuery>,
    transactions: TransactionTable,
    final_k: bool,
}

impl Query {
    fn add_node(&mut self, node: Node) -> bool {
        let node_q = NodeQuery::new(node, self.target);
        let cmp_distance = |x: &NodeQuery| x.distance.cmp(&node_q.distance);
        if let Err(index) = self.closest.binary_search_by(cmp_distance) {
            self.closest.insert(index, node_q);
            if self.closest.len() > K {
                self.closest.pop();
            }
            true
        } else {
            false
        }
    }

    fn update_status(&mut self, target: BitKey, status: QueryStatus) {
        for node in &mut self.closest {
            if node.node.id == target {
                node.status = status;
            }
        }
    }

    fn get_closest(&self) -> Option<Node> {
        for node in &self.closest {
            if node.status == QueryStatus::Empty {
                return Some(node.node);
            }
        }
        None
    }

    fn all_done(&self) -> bool {
        for node in &self.closest {
            if node.status != QueryStatus::Finished {
                return false;
            }
        }
        true
    }
}

struct ServerHandle {
    table: RoutingTable,
    sock: UdpSocket,
    key_store: HashMap<String, String>,
    query: Option<Query>,
    keep_alives: TransactionTable,
    rng: ThreadRng,
    buf: Box<[u8]>,
}

impl ServerHandle {
    fn send_message(&mut self, message: Message, addr: SocketAddr) -> io::Result<()> {
        let amt = message.write(&mut *self.buf);
        self.sock.send_to(&self.buf[..amt], addr)?;
        Ok(())
    }

    fn handle_message(&mut self, message: Message, src: SocketAddr) -> io::Result<()> {
        use RPCPayload::*;
        let node = Node {
            id: message.header.node_id,
            udp_addr: src,
        };
        if let KBucketInsert::Ping(to_ping) = self.table.insert(node) {
            let message = Message::create(&mut self.rng, self.table.this_node_id(), Ping);
            self.keep_alives.insert(message.header);
            self.send_message(message, to_ping.udp_addr)?;
        }
        match message.payload {
            Ping => {
                let message = Message::response(message.header, PingResp);
                self.send_message(message, src)
            }
            PingResp => {
                self.keep_alives.remove(message.header.transaction_id);
                Ok(())
            }
            FindValue(key) => {
                let message = match self.key_store.get(&key) {
                    None => {
                        let nodes = self.table.k_closest(BitKey::from_hash(&key), K);
                        Message::response(message.header, FindValueNodes(nodes))
                    }
                    Some(val) => Message::response(message.header, FindValueResp(val.clone())),
                };
                self.send_message(message, src)
            }
            FindValueResp(_val) => {
                if let Some(query) = &mut self.query {
                    if query.transactions.contains(message.header.transaction_id) {
                        // We've found the corresponding value
                        self.query = None;
                    }
                }
                Ok(())
            }
            FindValueNodes(nodes) => self.handle_nodes(message.header, &nodes),
            FindNode(id) => {
                let nodes = self.table.k_closest(id, K);
                let message = Message::response(message.header, FindNodeResp(nodes));
                self.send_message(message, src)
            }
            FindNodeResp(nodes) => self.handle_nodes(message.header, &nodes),
            Store(key, val) => {
                self.key_store.insert(key, val);
                let message = Message::response(message.header, StoreResp);
                self.send_message(message, src)
            }
            StoreResp => {
                self.keep_alives.remove(message.header.transaction_id);
                Ok(())
            }
        }
    }

    fn handle_nodes(&mut self, header: Header, nodes: &[Node]) -> io::Result<()> {
        let mut contact_nodes = Vec::new();
        if let Some(query) = &mut self.query {
            // We simply ignore this transaction if we didn't create it
            if !query.transactions.remove(header.transaction_id) {
                return Ok(());
            }
            let mut added = false;
            for node in nodes {
                added = query.add_node(*node) || added;
            }
            query.update_status(header.node_id, QueryStatus::Finished);
            if added {
                if let Some(next) = query.get_closest() {
                    contact_nodes.push(next);
                } else {
                    // There are no nodes left to contact, and no further work can be done
                    self.query = None;
                }
            } else if !query.final_k {
                query.final_k = true;
                for node in &query.closest {
                    if node.status == QueryStatus::Empty {
                        contact_nodes.push(node.node);
                    }
                }
            } else if query.all_done() {
                // We've finished querying the k closest nodes
                self.query = None;
            }
        }
        for node in contact_nodes {
            let query = self.query.as_mut().unwrap();
            query.update_status(node.id, QueryStatus::Started);
            let target = query.target;
            let payload = if let Some(key) = query.target_value.clone() {
                RPCPayload::FindValue(key)
            } else {
                RPCPayload::FindNode(target)
            };
            let message = Message::create(&mut self.rng, self.table.this_node_id(), payload);
            query.transactions.insert(message.header);
            self.send_message(message, node.udp_addr)?;
        }
        Ok(())
    }
}

pub fn run_server<S: ToSocketAddrs>(address: S) -> io::Result<()> {
    let mut rng = thread_rng();
    let sock = UdpSocket::bind(address)?;
    let this_addr = sock.local_addr()?;
    let this_node = Node::create(&mut rng, this_addr);
    let table = RoutingTable::new(this_node, K);
    let buf = Box::new([0; BUF_SIZE]);
    let mut handle = ServerHandle {
        table,
        sock,
        key_store: HashMap::new(),
        query: None,
        keep_alives: TransactionTable::new(),
        rng,
        buf,
    };
    loop {
        let (amt, src) = handle.sock.recv_from(&mut *handle.buf)?;
        let try_message = Message::try_from(&handle.buf[..amt]);
        match try_message {
            Err(e) => println!("Error parsing message from {} error: {:?}", src, e),
            Ok(message) => handle.handle_message(message, src)?,
        }
    }
}
