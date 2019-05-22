use crate::base::{BitKey, Node};

#[derive(Clone, Copy, Debug, PartialEq)]
struct TransactionID(u64);

struct Header {
    node_id: BitKey,
    transaction_id: TransactionID
}

enum RPCPayload {
    Ping,
    PingResp,
    FindValue(String),
    FindValueResp(String),
    FindValueNodes(Vec<Node>),
    FindNode(BitKey),
    FindNodeResp(Vec<Node>),
    Store(String, String),
    StoreResp
}