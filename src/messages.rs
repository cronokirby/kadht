use crate::base::{BitKey, Node};
use crate::rand::distributions::{Distribution, Standard};
use crate::rand::Rng;
use std::convert::{TryFrom, TryInto};
use std::net::{IpAddr, SocketAddr};

const BITKEY_BYTES: usize = 16;

/// Represents an error when parsing out a message.
///
/// This is produced when we try and parse a message, and fail for
/// some reason.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParseError {
    /// There were not enough bytes to parse the message
    InsufficientLength,
    /// The string had an invalid UTF8 format
    InvalidString,
    /// The type of message was unrecognized
    UnknownMessageType,
}

fn try_bitkey_from(data: &[u8]) -> Result<BitKey, ParseError> {
    let len = std::mem::size_of::<BitKey>();
    let bitkey_bytes = data[..len]
        .try_into()
        .map_err(|_| ParseError::InsufficientLength)?;
    Ok(BitKey(u128::from_be_bytes(bitkey_bytes)))
}

// This returns the string, and the total amount of bytes consumed
fn try_string_from(data: &[u8]) -> Result<(String, usize), ParseError> {
    let (head, rest) = data.split_first().ok_or(ParseError::InsufficientLength)?;
    let byte_count = *head as usize;
    if rest.len() < byte_count {
        return Err(ParseError::InsufficientLength);
    }
    let string = String::from_utf8(rest.into()).map_err(|_| ParseError::InvalidString)?;
    Ok((string, byte_count + 1))
}

fn try_nodes_from(data: &[u8]) -> Result<Vec<Node>, ParseError> {
    let (head, rest) = data.split_first().ok_or(ParseError::InsufficientLength)?;
    let capacity = *head as usize;
    let mut buf = Vec::with_capacity(capacity);
    let mut data = rest;
    while buf.len() < capacity {
        let start_len = 1 + BITKEY_BYTES;
        if data.len() < start_len {
            return Err(ParseError::InsufficientLength);
        }
        let id = try_bitkey_from(data).unwrap();
        let ip_type = data[BITKEY_BYTES];
        data = &data[start_len..];
        let ip_len = if ip_type == 4 { 4 } else { 16 };
        let end_len = ip_len + std::mem::size_of::<u16>();
        if data.len() < end_len {
            return Err(ParseError::InsufficientLength);
        }
        // The unwrapping is fine since we already checked the length
        let ip = if ip_type == 4 {
            let ip4_bytes: [u8; 4] = data[..ip_len].try_into().unwrap();
            IpAddr::V4(ip4_bytes.into())
        } else {
            let ip16_bytes: [u8; 16] = data[..ip_len].try_into().unwrap();
            IpAddr::V6(ip16_bytes.into())
        };
        let port_bytes = data[ip_len..end_len].try_into().unwrap();
        let port = u16::from_be_bytes(port_bytes);
        let udp_addr = SocketAddr::new(ip, port);
        buf.push(Node { id, udp_addr });
        data = &data[end_len..]
    }
    Ok(buf)
}

/// Represents a Transaction ID used to identify RPC calls
///
/// RPC calls include a transaction id in order to match responses
/// to requests, as well as to provide some mitigation against IP spoofing.
/// Transaction IDs can be generated randomly, but the Message struct already
/// provides a utility for generating them when creating a message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransactionID(u64);

impl TryFrom<&[u8]> for TransactionID {
    type Error = ParseError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        let bytes = data[..std::mem::size_of::<u64>()]
            .try_into()
            .map_err(|_| ParseError::InsufficientLength)?;
        Ok(TransactionID(u64::from_be_bytes(bytes)))
    }
}

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

impl TryFrom<&[u8]> for Header {
    type Error = ParseError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        if data.len() < std::mem::size_of::<Header>() {
            return Err(ParseError::InsufficientLength);
        }
        let (start, rest) = data.split_at(std::mem::size_of::<BitKey>());
        // We know that the length is sufficient in both cases
        let node_id = try_bitkey_from(start).unwrap();
        let transaction_id = rest.try_into().unwrap();
        Ok(Header {
            node_id,
            transaction_id,
        })
    }
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

impl TryFrom<&[u8]> for RPCPayload {
    type Error = ParseError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        let (msg_type, rest) = data.split_first().ok_or(ParseError::InsufficientLength)?;
        match msg_type {
            1 => Ok(RPCPayload::Ping),
            2 => Ok(RPCPayload::PingResp),
            3 => {
                let id = try_bitkey_from(rest)?;
                Ok(RPCPayload::FindNode(id))
            }
            4 => {
                let nodes = try_nodes_from(rest)?;
                Ok(RPCPayload::FindNodeResp(nodes))
            }
            5 => {
                let (key, read_count) = try_string_from(rest)?;
                let rest = &rest[read_count..];
                let (val, _) = try_string_from(rest)?;
                Ok(RPCPayload::Store(key, val))
            }
            6 => Ok(RPCPayload::StoreResp),
            7 => {
                let (key, _) = try_string_from(rest)?;
                Ok(RPCPayload::FindValue(key))
            }
            8 => {
                let nodes = try_nodes_from(rest)?;
                Ok(RPCPayload::FindValueNodes(nodes))
            }
            9 => {
                let (val, _) = try_string_from(rest)?;
                Ok(RPCPayload::FindValueResp(val))
            }
            _ => Err(ParseError::UnknownMessageType),
        }
    }
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

impl TryFrom<&[u8]> for Message {
    type Error = ParseError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        let header = data.try_into()?;
        // Indexing past this is safe, since we managed to parse the header
        let data = &data[std::mem::size_of::<Header>()..];
        let payload = data.try_into()?;
        Ok(Message { header, payload })
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
                for (i, b) in v4.octets().iter().enumerate() {
                    buf[i] = *b;
                }
                4
            }
            IpAddr::V6(v6) => {
                for (i, b) in v6.octets().iter().enumerate() {
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
