use crate::base::{BitKey, Node};
use std::net::IpAddr;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransactionID(u64);

pub struct Header {
    pub node_id: BitKey,
    pub transaction_id: TransactionID,
}

pub enum RPCPayload {
    Ping,
    PingResp,
    FindValue(String),
    FindValueResp(String),
    FindValueNodes(Vec<Node>),
    FindNode(BitKey),
    FindNodeResp(Vec<Node>),
    Store(String, String),
    StoreResp,
}

pub struct Message {
    pub header: Header,
    pub payload: RPCPayload,
}

impl Message {
    pub fn write(self, buf: &mut [u8]) -> usize {
        use RPCPayload::*;
        write_bitkey(self.header.node_id, buf);
        write_transaction_id(self.header.transaction_id, &mut buf[16..]);
        match self.payload {
            Ping => {
                buf[24] = 1;
                25
            }
            PingResp => {
                buf[24] = 2;
                25
            }
            FindNode(id) => {
                buf[24] = 3;
                write_bitkey(id, &mut buf[25..]);
                31
            }
            FindNodeResp(nodes) => {
                buf[24] = 4;
                let len = write_nodes(nodes, &mut buf[25..]);
                len + 25
            }
            Store(key, val) => {
                buf[24] = 5;
                let key_len = write_string(key, &mut buf[25..]);
                let val_len = write_string(val, &mut buf[25 + key_len..]);
                key_len + val_len + 25
            }
            StoreResp => {
                buf[24] = 6;
                25
            }
            FindValue(key) => {
                buf[24] = 7;
                let len = write_string(key, &mut buf[25..]);
                len + 25
            }
            FindValueNodes(nodes) => {
                buf[24] = 8;
                let len = write_nodes(nodes, &mut buf[25..]);
                len + 25
            }
            FindValueResp(val) => {
                buf[24] = 9;
                let len = write_string(val, &mut buf[25..]);
                len + 25
            }
        }
    }
}

fn write_bitkey(key: BitKey, buf: &mut [u8]) {
    let mut num = key.0;
    for i in (0..15).rev() {
        buf[i] = num as u8;
        num >>= 8;
    }
}

fn write_transaction_id(id: TransactionID, buf: &mut [u8]) {
    let mut num = id.0;
    for i in (0..8).rev() {
        buf[i] = num as u8;
        num >>= 8;
    }
}

// This will only work with strings less than 256 bytes
fn write_string(string: String, buf: &mut [u8]) -> usize {
    let len = string.len();
    buf[0] = len as u8;
    let str_buf = &mut buf[1..];
    for (i, b) in string.bytes().enumerate() {
        str_buf[i] = b;
    }
    len + 1
}

fn write_nodes(nodes: Vec<Node>, mut buf: &mut [u8]) -> usize {
    buf[0] = nodes.len() as u8;
    let mut count = 1;
    buf = &mut buf[1..];
    for node in nodes {
        write_bitkey(node.id, buf);
        let version = if node.udp_addr.is_ipv4() { 4 } else { 6 };
        buf[0] = version;
        buf = &mut buf[1..];
        let written = match node.udp_addr.ip() {
            IpAddr::V4(v4) => {
                for (i, b) in v4.octets().into_iter().enumerate() {
                    buf[i] = *b;
                }
                4
            }
            IpAddr::V6(v6) => {
                for (i, b) in v6.octets().into_iter().enumerate() {
                    buf[i] = *b;
                }
                16
            }
        };
        buf = &mut buf[written..];
        let port = node.udp_addr.port();
        buf[0] = (port >> 8) as u8;
        buf[1] = port as u8;
        count += written + 3;
    }
    count
}
