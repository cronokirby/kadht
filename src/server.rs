use crate::base::Node;
use crate::messages::{Message, RPCPayload};
use crate::rand::thread_rng;
use crate::routing::RoutingTable;
use std::convert::TryFrom;
use std::io;
use std::net::{ToSocketAddrs, UdpSocket};

// How big to make our buckets
const K: usize = 20;
const BUF_SIZE: usize = 2048;

struct ServerHandle {
    table: RoutingTable,
    sock: UdpSocket,
}

pub fn run_server<S: ToSocketAddrs>(address: S) -> io::Result<()> {
    let mut rng = thread_rng();
    let sock = UdpSocket::bind(address)?;
    let this_addr = sock.local_addr()?;
    let this_node = Node::create(&mut rng, this_addr);
    let table = RoutingTable::new(this_node, K);
    let mut handle = ServerHandle { table, sock };
    let mut buf = Box::new([0; BUF_SIZE]);
    loop {
        let (amt, src) = handle.sock.recv_from(&mut *buf)?;
        let try_message = Message::try_from(&buf[..amt]);
        match try_message {
            Err(e) => println!("Error parsing message from {} error: {:?}", src, e),
            Ok(message) => handle_message(&mut handle, message)?,
        }
    }
}

fn handle_message(handle: &mut ServerHandle, message: Message) -> io::Result<()> {
    use RPCPayload::*;
    match message.payload {
        Ping => unimplemented!(),
        PingResp => unimplemented!(),
        FindValue(key) => unimplemented!(),
        FindValueResp(key) => unimplemented!(),
        FindValueNodes(nodes) => unimplemented!(),
        FindNode(id) => unimplemented!(),
        FindNodeResp(nodes) => unimplemented!(),
        Store(key, val) => unimplemented!(),
        StoreResp => unimplemented!(),
    }
}
