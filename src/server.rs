use crate::base::{BitKey, Node};
use crate::messages::{Header, Message, RPCPayload, TransactionID};
use crate::rand::rngs::ThreadRng;
use crate::rand::thread_rng;
use crate::routing::{KBucketInsert, RoutingTable};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::mpsc::{channel, Receiver, RecvError, SendError, Sender};
use std::time::{Duration, Instant};

// How big to make our buckets
const K: usize = 20;
const BUF_SIZE: usize = 2048;

#[derive(Debug)]
pub enum ToServerMsg {
    Store(String, String),
    Get(String),
}

#[derive(Debug)]
pub enum FromServerMsg {
    StoreResp,
    GetResp(Option<String>),
}

pub struct ServerSender {
    to: Sender<ToServerMsg>,
    from: Receiver<FromServerMsg>,
}

impl ServerSender {
    pub fn send(&self, msg: ToServerMsg) -> Result<(), SendError<ToServerMsg>> {
        self.to.send(msg)
    }

    pub fn receive(&self) -> Result<FromServerMsg, RecvError> {
        self.from.recv()
    }
}

pub struct ServerReceiver {
    from: Receiver<ToServerMsg>,
    to: Sender<FromServerMsg>,
}

pub fn make_server_comms() -> (ServerSender, ServerReceiver) {
    let (sender_to, receiver_to) = channel();
    let (sender_from, receiver_from) = channel();
    let sender = ServerSender {
        to: sender_to,
        from: receiver_from,
    };
    let receiver = ServerReceiver {
        to: sender_from,
        from: receiver_to,
    };
    (sender, receiver)
}

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

    fn remove_stale(&mut self, buf: &mut Vec<BitKey>) {
        let now = Instant::now();
        self.transactions.retain(|_, (then, key)| {
            if now.duration_since(*then) > Duration::new(5, 0) {
                buf.push(*key);
                false
            } else {
                true
            }
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
enum QueryIntention {
    Store(String, String),
    Get(String),
}

impl QueryIntention {
    fn key_to_find(&self) -> Option<String> {
        match self {
            QueryIntention::Store(_, _) => None,
            QueryIntention::Get(key) => Some(key.clone()),
        }
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
    intention: QueryIntention,
    closest: Vec<NodeQuery>,
    transactions: TransactionTable,
    final_k: bool,
}

impl Query {
    fn new(intention: QueryIntention) -> Self {
        let key = match &intention {
            QueryIntention::Store(key, _) => key,
            QueryIntention::Get(key) => key,
        };
        Query {
            target: BitKey::from_hash(&key),
            intention,
            closest: Vec::with_capacity(K),
            transactions: TransactionTable::new(),
            final_k: false,
        }
    }

    fn find_node(&self, key: BitKey) -> Result<usize, usize> {
        let distance = key.distance(self.target);
        let cmp_distance = |x: &NodeQuery| x.distance.cmp(&distance);
        self.closest.binary_search_by(cmp_distance)
    }

    fn add_node(&mut self, node: Node) -> bool {
        if let Err(index) = self.find_node(node.id) {
            self.closest
                .insert(index, NodeQuery::new(node, self.target));
            if self.closest.len() > K {
                self.closest.pop();
            }
            true
        } else {
            false
        }
    }

    fn update_status(&mut self, target: BitKey, status: QueryStatus) {
        if let Ok(index) = self.find_node(target) {
            self.closest[index].status = status;
        }
    }

    fn remove(&mut self, key: BitKey) {
        if let Ok(index) = self.find_node(key) {
            self.closest.remove(index);
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
    sock: UdpSocket,
    receiver: ServerReceiver,
    table: RoutingTable,
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
            self.continue_query(node)?;
        }
        Ok(())
    }

    fn continue_query(&mut self, node: Node) -> io::Result<()> {
        let query = self.query.as_mut().unwrap();
        query.update_status(node.id, QueryStatus::Started);
        let target = query.target;
        let payload = if let Some(key) = query.intention.key_to_find() {
            RPCPayload::FindValue(key)
        } else {
            RPCPayload::FindNode(target)
        };
        let message = Message::create(&mut self.rng, self.table.this_node_id(), payload);
        query.transactions.insert(message.header);
        self.send_message(message, node.udp_addr)
    }

    fn remove_stale(&mut self) -> io::Result<()> {
        let mut buf = Vec::new();
        if let Some(query) = &mut self.query {
            query.transactions.remove_stale(&mut buf);
            for &key in &buf {
                query.remove(key);
            }
            if query.all_done() {
                self.query = None;
            } else if !query.final_k {
                if let Some(node) = query.get_closest() {
                    self.continue_query(node)?;
                }
            }
        }
        self.keep_alives.remove_stale(&mut buf);
        for &key in &buf {
            self.table.remove(key);
        }
        Ok(())
    }

    fn handle_client(&mut self) -> io::Result<()> {
        match self.receiver.from.try_recv() {
            Ok(ToServerMsg::Get(key)) => {
                let value = self.key_store.get(&key).cloned();
                let msg = FromServerMsg::GetResp(value);
                self.receiver.to.send(msg).unwrap();
                Ok(())
            }
            Ok(ToServerMsg::Store(_key, _v)) => {
                let msg = FromServerMsg::StoreResp;
                self.receiver.to.send(msg).unwrap();
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

pub fn run_server<S: ToSocketAddrs>(receiver: ServerReceiver, address: S) -> io::Result<()> {
    let mut rng = thread_rng();
    let sock = UdpSocket::bind(address)?;
    let this_addr = sock.local_addr()?;
    let this_node = Node::create(&mut rng, this_addr);
    let table = RoutingTable::new(this_node, K);
    let buf = Box::new([0; BUF_SIZE]);
    let mut handle = ServerHandle {
        table,
        receiver,
        sock,
        key_store: HashMap::new(),
        query: None,
        keep_alives: TransactionTable::new(),
        rng,
        buf,
    };
    let timeout = Duration::from_millis(400);
    handle.sock.set_read_timeout(Some(timeout))?;
    loop {
        if let Ok((amt, src)) = handle.sock.recv_from(&mut *handle.buf) {
            let try_message = Message::try_from(&handle.buf[..amt]);
            match try_message {
                Err(e) => println!("Error parsing message from {} error: {:?}", src, e),
                Ok(message) => handle.handle_message(message, src)?,
            }
        }
        handle.remove_stale()?;
        handle.handle_client()?;
    }
}
