use crate::base::{BitKey, Node};
use crate::rand::distributions::{Distribution, Standard};
use crate::rand::Rng;
use std::net::IpAddr;

const BITKEY_BYTES: usize = 16;

/// Represents a Transaction ID used to identify RPC calls
///
/// RPC calls include a transaction id in order to match responses
/// to requests, as well as to provide some mitigation against IP spoofing.
/// Transaction IDs can be generated randomly, but the Message struct already
/// provides a utility for generating them when creating a message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransactionID(u64);

impl Distribution<TransactionID> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> TransactionID {
        TransactionID(rng.gen())
    }
}

/// Represents the Header included with every RPC message.
///
/// This contains information about the node that sent the message, as well
/// as the transaction ID identifying this message. The transaction ID
/// is unique when this message is a call, and matches the request when
/// this message is a response
pub struct Header {
    /// The ID for the node that is sending this message
    pub node_id: BitKey,
    /// A transaction ID identifying this RPC call
    pub transaction_id: TransactionID,
}

/// Represents the data differing between RPC messages.
///
/// This contains branches for both RPC requests, and RPC responses.
pub enum RPCPayload {
    /// Request a Ping response from a node.
    ///
    /// This is mainly used to check whether or not a node is still alive.
    Ping,
    /// Respond to a ping request from a node.
    PingResp,
    /// Ask for the value bound to a given key
    FindValue(String),
    /// Respond with the value for the key requested
    FindValueResp(String),
    /// Respond with up to K of the closest nodes we know of to the requested key
    ///
    /// This will get returned instead of `FindValuesResp` unless we've received
    /// a `Store` call directly.
    FindValueNodes(Vec<Node>),
    /// Try and find the K closest nodes to a given key
    FindNode(BitKey),
    /// Respond with up to K of the closest nodes to the requested key
    FindNodeResp(Vec<Node>),
    /// Store a `(key, value)` pair in a given node
    Store(String, String),
    /// Respond to a `Store` request, confirming that it happened
    StoreResp,
}

/// Represents an RPC message sent between two nodes.
///
/// Every header contains a header, giving us information about
/// the sender, as well as identifying the RPC call, allowing us
/// to link responses with requests. After the header, we have
/// a payload identifying the specific kind of request we're dealing with.
pub struct Message {
    /// This contains general metadata about this message
    pub header: Header,
    /// This contains specific data depending on the message we're sending
    pub payload: RPCPayload,
}

impl Message {
    /// Create a new message, including our node id, and a payload.
    ///
    /// This will generate a new transaction ID for this message as well.
    /// This should be used when we're initiating an RPC call, as we want a new transaction ID
    /// for the message. This shouldn't be used when we're responding to an RPC call, because
    /// we want to include the transaction ID used in that call.
    /// In that case,
    pub fn create<R: Rng + ?Sized>(rng: &mut R, this_node_id: BitKey, payload: RPCPayload) -> Self {
        let transaction_id = rng.gen();
        Self::response(transaction_id, this_node_id, payload)
    }

    /// Create a new message, including our own node_id, a payload, and matching a transaction ID.
    ///
    /// This should be used when responding to an RPC call, since we want to include
    /// the transaction ID used in that call. This can't be used when initiating
    /// an RPC call, since we have no transaction ID to mirror, and instead need to generate
    /// a fresh one.
    /// This can be done with
    /// [create](struct.Message.html#method.create).
    pub fn response(transaction_id: TransactionID, node_id: BitKey, payload: RPCPayload) -> Self {
        let header = Header {
            node_id,
            transaction_id,
        };
        Message { header, payload }
    }

    /// Serialize a message to a buffer, returning the number of bytes written.
    pub fn write(self, buf: &mut [u8]) -> usize {
        use RPCPayload::*;
        write_bitkey(self.header.node_id, buf);
        write_transaction_id(self.header.transaction_id, &mut buf[BITKEY_BYTES..]);
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
                41
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
    for i in (0..BITKEY_BYTES).rev() {
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
        buf = &mut buf[BITKEY_BYTES..];
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
        count += written + 19;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_req_write() {
        let header = Header {
            node_id: BitKey(0x102030405060708090A0B0C0D0E0F),
            transaction_id: TransactionID(0x0102030405060708),
        };
        let bytes = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1, 2, 3, 4, 5, 6, 7, 8, 1,
        ];
        let mut buf = [0; 0x100];
        let msg = Message {
            header,
            payload: RPCPayload::Ping,
        };
        let count = msg.write(&mut buf);
        assert_eq!(&bytes, &buf[..count]);
    }

    #[test]
    fn ping_resp_write() {
        let header = Header {
            node_id: BitKey(0x102030405060708090A0B0C0D0E0F),
            transaction_id: TransactionID(0x0102030405060708),
        };
        let bytes = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1, 2, 3, 4, 5, 6, 7, 8, 2,
        ];
        let mut buf = [0; 0x100];
        let msg = Message {
            header,
            payload: RPCPayload::PingResp,
        };
        let count = msg.write(&mut buf);
        assert_eq!(&bytes, &buf[..count]);
    }

    #[test]
    fn find_value_req_write() {
        let header = Header {
            node_id: BitKey(0x102030405060708090A0B0C0D0E0F),
            transaction_id: TransactionID(0x0102030405060708),
        };
        let string = String::from("AAAA");
        let bytes = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1, 2, 3, 4, 5, 6, 7, 8, 7, 4, 65,
            65, 65, 65,
        ];
        let mut buf = [0; 0x100];
        let msg = Message {
            header,
            payload: RPCPayload::FindValue(string),
        };
        let count = msg.write(&mut buf);
        assert_eq!(&bytes, &buf[..count]);
    }

    #[test]
    fn find_value_resp_write() {
        let header = Header {
            node_id: BitKey(0x102030405060708090A0B0C0D0E0F),
            transaction_id: TransactionID(0x0102030405060708),
        };
        let string = String::from("AAAA");
        let bytes = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1, 2, 3, 4, 5, 6, 7, 8, 9, 4, 65,
            65, 65, 65,
        ];
        let mut buf = [0; 0x100];
        let msg = Message {
            header,
            payload: RPCPayload::FindValueResp(string),
        };
        let count = msg.write(&mut buf);
        assert_eq!(&bytes, &buf[..count]);
    }

    #[test]
    fn find_value_nodes_write() {
        let header = Header {
            node_id: BitKey(0x102030405060708090A0B0C0D0E0F),
            transaction_id: TransactionID(0x0102030405060708),
        };
        let nodes = vec![Node {
            id: header.node_id,
            udp_addr: "127.0.0.1:8080".parse().unwrap(),
        }];
        let bytes = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1, 2, 3, 4, 5, 6, 7, 8, 8, 1, 0,
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 4, 127, 0, 0, 1, 31, 144,
        ];
        let mut buf = [0; 0x100];
        let msg = Message {
            header,
            payload: RPCPayload::FindValueNodes(nodes),
        };
        let count = msg.write(&mut buf);
        assert_eq!(&bytes[0..], &buf[..count]);
    }

    #[test]
    fn find_node_req_write() {
        let header = Header {
            node_id: BitKey(0x102030405060708090A0B0C0D0E0F),
            transaction_id: TransactionID(0x0102030405060708),
        };
        let bytes = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1, 2, 3, 4, 5, 6, 7, 8, 3, 0, 1,
            2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
        ];
        let id = header.node_id;
        let mut buf = [0; 0x100];
        let msg = Message {
            header,
            payload: RPCPayload::FindNode(id),
        };
        let count = msg.write(&mut buf);
        assert_eq!(&bytes[0..], &buf[..count]);
    }

    #[test]
    fn find_node_resp_write() {
        let header = Header {
            node_id: BitKey(0x102030405060708090A0B0C0D0E0F),
            transaction_id: TransactionID(0x0102030405060708),
        };
        let nodes = vec![Node {
            id: header.node_id,
            udp_addr: "127.0.0.1:8080".parse().unwrap(),
        }];
        let bytes = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1, 2, 3, 4, 5, 6, 7, 8, 4, 1, 0,
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 4, 127, 0, 0, 1, 31, 144,
        ];
        let mut buf = [0; 0x100];
        let msg = Message {
            header,
            payload: RPCPayload::FindNodeResp(nodes),
        };
        let count = msg.write(&mut buf);
        assert_eq!(&bytes[0..], &buf[..count]);
    }

    #[test]
    fn store_req_write() {
        let header = Header {
            node_id: BitKey(0x102030405060708090A0B0C0D0E0F),
            transaction_id: TransactionID(0x0102030405060708),
        };
        let key = String::from("AAAA");
        let val = String::from("BBBB");
        let bytes: [u8; 35] = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1, 2, 3, 4, 5, 6, 7, 8, 5, 4, 65,
            65, 65, 65, 4, 66, 66, 66, 66,
        ];
        let mut buf = [0; 0x100];
        let msg = Message {
            header,
            payload: RPCPayload::Store(key, val),
        };
        let count = msg.write(&mut buf);
        assert_eq!(&bytes[0..], &buf[..count]);
    }

    #[test]
    fn store_resp_write() {
        let header = Header {
            node_id: BitKey(0x102030405060708090A0B0C0D0E0F),
            transaction_id: TransactionID(0x0102030405060708),
        };
        let bytes = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1, 2, 3, 4, 5, 6, 7, 8, 6,
        ];
        let mut buf = [0; 0x100];
        let msg = Message {
            header,
            payload: RPCPayload::StoreResp,
        };
        let count = msg.write(&mut buf);
        assert_eq!(&bytes, &buf[..count]);
    }
}
