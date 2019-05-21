use crate::base::{Node, KEY_SIZE};
use std::collections::VecDeque;
use std::net::SocketAddr;

/// How many nodes should be active in a bucket
const BUCKET_SIZE: usize = 20;

/// Represents the result of inserting into a KBucket.
///
/// Depending on the state of the bucket, we might not be able to insert
/// a new value. Since the bucket doesn't have the capability of hitting
/// the network to check whether or not nodes are alive, we need to do that
/// ourselves, after having called
/// [insert](struct.KBucket.html#method.insert).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KBucketInsert<T> {
    /// We successfully inserted the item into the bucket
    Inserted,
    /// We couldn't insert the item into the bucket, and need to ping the network.
    ///
    /// In this case, we want to check if the oldest node in the bucket is still
    /// alive, so we need to ping that node, and then report back to the bucket.
    /// If the node is still alive, we then call
    /// [insert](struct.KBucket.html#method.succcessful_ping),
    /// otherwise we call
    /// [remove](struct.KBucket.html#method.failed_ping).
    Ping(T),
}

/// This represents a KBucket used in the Kademlia DHT.
///
/// Each KBucket is used to store a fixed size set of nodes.
/// New nodes can be added into the bucket until it fills up, at which
/// point new nodes will only be added if a node dies.
/// We have this preference for long-lived nodes, since the longer a node
/// lives, the longer it tends to stay alive as well.
#[derive(Clone, Debug)]
pub struct KBucket<T> {
    // The max size never changes, and should usually be 20, but
    // we store it inside the struct itself since we access it frequently.
    max_size: usize,
    // This acts as a FILO stack for pending nodes.
    // New nodes can only be inserted into a full bucket if an existing
    // node in that bucket is died. We always want to insert the most
    // recently known nodes, so we use this stack order for the waiting
    // elements.
    waiting: Vec<T>,
    // This holds the actual elements in the bucket
    data: VecDeque<T>,
}

impl<T: Clone + PartialEq> KBucket<T> {
    /// Create a new KBucket with a given max_size
    ///
    /// The default specified in the Kademlia paper is 20.
    pub fn new(max_size: usize) -> Self {
        KBucket {
            max_size,
            waiting: Vec::new(),
            data: VecDeque::with_capacity(max_size),
        }
    }

    /// Try and insert an element into the bucket.
    ///
    /// This should be called whenever any message is received from a node,
    /// regardless of whether or not it's an rpc call or response.
    ///
    /// If the bucket still has room left, we just insert the element
    /// directly, and `Inserted` is returned. If we can't insert the element,
    /// then we return an element that needs to be pinged to check if it's
    /// still alive. After performing that check, either insert should
    /// be called again, since we received a ping response from that node,
    /// or remove should be called, since we know that node has died.
    pub fn insert(&mut self, item: T) -> KBucketInsert<T> {
        let existing = self.data.iter().position(|x| *x == item);
        if let Some(index) = existing {
            self.data.remove(index);
        }
        if self.data.len() < self.max_size {
            self.data.push_back(item);
            return KBucketInsert::Inserted;
        } else {
            self.waiting.push(item);
            return KBucketInsert::Ping(self.data[0].clone());
        }
    }

    /// Remove a dead node from this bucket.
    ///
    /// This should be called after an RPC call to a node timed out,
    /// which indicates that the node appears to be dead. This also
    /// applies in the case that we were asked to ping a node after
    /// inserting an item into the bucket.
    ///
    /// Removing a node also has the effect of inserting the node we
    /// tried to insert most recently, but couldn't because of the lack of
    /// dead nodes.
    pub fn remove(&mut self, item: T) {
        let existing = self.data.iter().position(|x| *x == item);
        if let Some(index) = existing {
            self.data.remove(index);
            if let Some(new) = self.waiting.pop() {
                self.data.push_back(new);
            }
        }
    }
}

// Our implementation for the routing table initializes all buckets immediately,
// instead of doing "lazy" splitting of buckets closer to the range our node
// is contained in. This has the advantage of making the implementation quite simple.
/// Represents a routing table, containing buckets at varying distances.
///
/// We organise buckets based on certain intervals of distances. Each bucket
/// contains nodes whose distance from this instance is between 2 subsequent
/// powers of 2. This means that the further away a range is from us, the less
/// information we have about nodes in that range.
pub struct RoutingTable {
    // We node to know which nodemaps to this instance,
    // since the routing table is based on buckets of certain
    // distance intervals from this node
    this_node: Node,
    // The buffer containing KEY_SIZE buckets.
    // The Nth element is a bucket containing elements with distance
    // in [2^(KEY_SIZE - N); 2^(KEY_SIZE - N + 1)[ from this node.
    // This can be calculated more simply by saying that the bucket with index i
    // contains nodes with i leading zeros in their distance from this node.
    // For example, if the distance between a node and this node is 00101b,
    // then this would go in the bucket with index 2.
    buckets: Vec<KBucket<Node>>,
}

impl RoutingTable {
    /// Construct a new routing table with a node for this instance.
    ///
    /// We need to know which node is representing this instance
    /// in order to evaluate the distance between this instance and the nodes
    /// we try and insert into the routing table.
    pub fn new(this_node: Node) -> Self {
        let buckets = vec![KBucket::new(BUCKET_SIZE); KEY_SIZE];
        RoutingTable { this_node, buckets }
    }

    /// Insert a node from the routing table.
    ///
    /// See
    /// [KBucket::insert](struct.KBucket.html#method.insert)
    /// for more information about this operation, as well as under
    /// which conditions this operation should be executed.
    ///
    /// Inserting the node for this instance will just return `KBucketInsert::Inserted`
    /// but do nothing to the underlying buckets. There's no reason
    /// to ever call this method with the node for this instance however.
    pub fn insert(&mut self, node: Node) -> KBucketInsert<Node> {
        // In theory no one should even try to insert this node, but
        // it can be handled as if we successfully inserted it.
        // It's like the first field of this struct is the bucket for nodes
        // of distance 0, i.e. just this node.
        if self.this_node == node {
            return KBucketInsert::Inserted;
        }
        let distance = self.this_node.id.distance(node.id);
        let i = distance.leading_zeros() as usize;
        self.buckets[i].insert(node)
    }

    /// Remove a node from the routing table.
    ///
    /// See
    /// [KBucket::remove](struct.KBucket.html#method.remove)
    /// for more information about this operation, as well as under
    /// which conditions this operation should be executed.
    ///
    /// This does nothing the node for this instance is passed.
    pub fn remove(&mut self, node: Node) {
        if self.this_node == node {
            return;
        }
        let distance = self.this_node.id.distance(node.id);
        let i = distance.leading_zeros() as usize;
        self.buckets[i].remove(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::BitKey;

    #[test]
    fn kbucket_can_insert_max_size() {
        let max_size = 20;
        let mut bucket: KBucket<usize> = KBucket::new(max_size);
        for x in 0..max_size {
            assert_eq!(KBucketInsert::Inserted, bucket.insert(x));
        }
    }

    #[test]
    fn kbucket_pings_first_inserted() {
        let max_size = 20;
        let mut bucket: KBucket<usize> = KBucket::new(max_size);
        for x in 0..max_size {
            bucket.insert(x);
        }
        assert_eq!(KBucketInsert::Ping(0), bucket.insert(max_size));
    }

    #[test]
    fn kbucket_remove_replaces_waiting() {
        let max_size = 20;
        let mut bucket: KBucket<usize> = KBucket::new(max_size);
        for x in 0..max_size {
            bucket.insert(x);
        }
        bucket.insert(max_size);
        bucket.remove(0);
        assert_eq!(Some(1), bucket.data.pop_front());
        assert_eq!(Some(max_size), bucket.data.pop_back());
    }

    #[test]
    fn routing_table_can_insert() {
        let udp_addr = "127.0.0.1:1234".parse().unwrap();
        let this_node = Node {
            id: BitKey(0),
            udp_addr,
        };
        let mut table = RoutingTable::new(this_node);
        for k in 0..KEY_SIZE {
            let id = BitKey(1 << k);
            let node = Node { id, udp_addr };
            assert_eq!(KBucketInsert::Inserted, table.insert(node));
        }
    }
}
