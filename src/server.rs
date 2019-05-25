use crate::base::{BitKey, Node};
use crate::messages::{Message, RPCPayload};
use crate::rand::thread_rng;
use crate::routing::RoutingTable;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

// How big to make our buckets
const K: usize = 20;
const BUF_SIZE: usize = 2048;

struct ServerHandle {
    table: RoutingTable,
    sock: UdpSocket,
    key_store: HashMap<String, String>,
    buf: Box<[u8]>,
}

impl ServerHandle {
    fn send_message(&mut self, message: Message, addr: SocketAddr) -> io::Result<()> {
        let amt = message.write(&mut *self.buf);
        self.sock.send_to(&self.buf[..amt], addr)?;
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
    let key_store = HashMap::new();
    let mut handle = ServerHandle { table, sock, key_store, buf };
    loop {
        let (amt, src) = handle.sock.recv_from(&mut *handle.buf)?;
        let try_message = Message::try_from(&handle.buf[..amt]);
        match try_message {
            Err(e) => println!("Error parsing message from {} error: {:?}", src, e),
            Ok(message) => handle_message(&mut handle, message, src)?,
        }
    }
}

fn handle_message(handle: &mut ServerHandle, message: Message, src: SocketAddr) -> io::Result<()> {
    use RPCPayload::*;
    match message.payload {
        Ping => {
            let message = Message::response(message.header, PingResp);
            handle.send_message(message, src)
        }
        PingResp => unimplemented!(),
        FindValue(key) => {
            let message = match handle.key_store.get(&key) {
                None => {
                    // Todo hash the key to get the bitkey
                    let nodes = handle.table.k_closest(BitKey(0), K);
                    Message::response(message.header, FindValueNodes(nodes))
                }
                Some(val) => {
                    Message::response(message.header, FindValueResp(val.clone()))
                }
            };
            handle.send_message(message, src)
        },
        FindValueResp(val) => unimplemented!(),
        FindValueNodes(nodes) => unimplemented!(),
        FindNode(id) => {
            let nodes = handle.table.k_closest(id, K);
            let message = Message::response(message.header, FindNodeResp(nodes));
            handle.send_message(message, src)
        },
        FindNodeResp(nodes) => unimplemented!(),
        Store(key, val) => {
            handle.key_store.insert(key, val);
            let message = Message::response(message.header, StoreResp);
            handle.send_message(message, src)
        },
        StoreResp => unimplemented!(),
    }
}
