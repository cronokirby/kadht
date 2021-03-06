use crate::base::{BitKey, Node, KEY_SIZE};
use std::collections::VecDeque;

/// Represents the result of inserting into a KBucket.
///
/// Depending on the state of the bucket, we might not be able to insert
/// a new value. Since the bucket doesn't have the capability of hitting
/// the network to check whether or not nodes are alive, we need to do that
/// ourselves, after having called
/// [insert](struct.KBucket.html#method.insert).
#[derive(Clone, Debug, PartialEq)]
pub enum KBucketInsert {
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
    Ping(Node),
}

/// This represents a KBucket used in the Kademlia DHT.
///
/// Each KBucket is used to store a fixed size set of nodes.
/// New nodes can be added into the bucket until it fills up, at which
/// point new nodes will only be added if a node dies.
/// We have this preference for long-lived nodes, since the longer a node
/// lives, the longer it tends to stay alive as well.
#[derive(Clone, Debug)]
pub struct KBucket {
    // The max size never changes, and should usually be 20, but
    // we store it inside the struct itself since we access it frequently.
    max_size: usize,
    // This acts as a FILO stack for pending nodes.
    // New nodes can only be inserted into a full bucket if an existing
    // node in that bucket is died. We always want to insert the most
    // recently known nodes, so we use this stack order for the waiting
    // elements.
    waiting: Vec<Node>,
    // This holds the actual elements in the bucket
    data: VecDeque<Node>,
}

impl KBucket {
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
    pub fn insert(&mut self, item: Node) -> KBucketInsert {
        let existing = self.data.iter().position(|x| *x == item);
        if let Some(index) = existing {
            self.data.remove(index);
        }
        if self.data.len() < self.max_size {
            self.data.push_back(item);
            KBucketInsert::Inserted
        } else {
            self.waiting.push(item);
            KBucketInsert::Ping(self.data[0])
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
    pub fn remove(&mut self, id: BitKey) {
        let existing = self.data.iter().position(|x| x.id == id);
        if let Some(index) = existing {
            self.data.remove(index);
            if let Some(new) = self.waiting.pop() {
                self.data.push_back(new);
            }
        }
    }

    /// Find up to the the k closest nodes to a target in this bucket.
    ///
    /// This will return `min(k, bucket_items)` items. This pushes the items
    /// to the bucket in sorted order as well.
    pub fn k_closest(&self, buf: &mut Vec<Node>, target: BitKey, k: usize) -> usize {
        let mut scratch: Vec<Node> = self.data.iter().cloned().collect();
        scratch.sort_by_cached_key(|node| node.id.distance(target));
        for node in scratch.into_iter().take(k) {
            buf.push(node);
        }
        self.data.len().min(k)
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
    buckets: Vec<KBucket>,
}

impl RoutingTable {
    /// Construct a new routing table with a node for this instance.
    ///
    /// We need to know which node is representing this instance
    /// in order to evaluate the distance between this instance and the nodes
    /// we try and insert into the routing table.
    pub fn new(this_node: Node, bucket_size: usize) -> Self {
        let buckets = vec![KBucket::new(bucket_size); KEY_SIZE];
        RoutingTable { this_node, buckets }
    }

    pub fn this_node_id(&self) -> BitKey {
        self.this_node.id
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
    pub fn insert(&mut self, node: Node) -> KBucketInsert {
        // In theory no one should even try to insert this node, but
        // it can be handled as if we successfully inserted it.
        // It's like the first field of this struct is the bucket for nodes
        // of distance 0, i.e. just this node.
        if self.this_node == node {
            return KBucketInsert::Inserted;
        }
        let distance = self.this_node.distance(&node);
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
    pub fn remove(&mut self, id: BitKey) {
        if self.this_node.id == id {
            return;
        }
        let distance = self.this_node.id.distance(id);
        let i = distance.leading_zeros() as usize;
        self.buckets[i].remove(id);
    }

    /// Find the k_closest elements to the target key in the routing table.
    ///
    /// This may return less than k elements, but only if there are less than
    /// k nodes in the routing table as a whole.
    ///
    /// This will include the node for this instance as well, if it's close enough.
    ///
    /// This is a key operation used in many places throughout the protocol.
    /// There are a lot of procedures in the DHT protocol which involve locating
    /// the closest nodes to a given a key.
    pub fn k_closest(&self, target: BitKey, k: usize) -> Vec<Node> {
        let mut buf = Vec::with_capacity(k);
        // The following operations seem like gibberish without a bit of explanation, so
        // let's try and do a bit of that. Let's denote by "t" our target node,
        // by "this" the node for this instance, and "a" some given other node.
        // First let's remember that since d(a, t) = a ^ t.
        // Other useful properties of ^ are that for any x, x ^ x = 0, and x ^ 0 = x.
        // Thus, a ^ t = a ^ this ^ this ^ t = d(a, this) ^ d(t, this).
        // Thankfully we have already organised our nodes into buckets based on the most
        // significant bit of d(a, this).
        // We can separate nodes into 2 categories,
        // those such that d(a, this) ^ d(t, this) < d(t, this),
        // and those such that d(a, this) ^ d(t, this) >= d(t, this).
        // We can actually tell which category a node is in based on which bucket the node is in!
        // Each bucket corresponds to a specific bit, with bucket 0 being the MSB. Each node
        // in that bucket has a d(a, this) such that that bit is 1, and all more significant bits are 0.
        // If a specific bit "i" in d(t, this) is 1,
        // then the nodes in the corresponding bucket belong to the first category,
        // if it is 0, then the nodes in that bucket correspond to the second.
        // For example if d(t, this) is 0101, then the nodes in the second bucket have a d(a, this)
        // that looks like 01XX, which only decreases d(t, this). Furthermore,
        // the more significant the bit for nodes in the first category,
        // the more it decreases the distance, whereas for the second category this is flipped:
        // the more significant, the further away nodes in that bucket are from t.
        //
        // Our algorithm thus consists of looking at the bits in d(t, this),
        // and pulling from the buckets corresponding to the 1 bits,
        // in most to least significant order, then looking at this,
        // then going over the 0 bits in least to most significant order.
        let mut distance = self.this_node.id.distance(target);
        let mut n_distance = !distance;
        let mut to_take = k;
        while distance != 0 && to_take > 0 {
            let i = distance.leading_zeros();
            let bucket = i as usize;
            to_take -= self.buckets[bucket].k_closest(&mut buf, target, to_take);
            distance ^= 1 << (KEY_SIZE as u32 - i);
        }
        if to_take > 0 {
            buf.push(self.this_node);
            to_take -= 1;
        }
        while n_distance != 0 && to_take > 0 {
            let i = n_distance.trailing_zeros();
            let bucket = KEY_SIZE - 1 - i as usize;
            to_take -= self.buckets[bucket].k_closest(&mut buf, target, to_take);
            n_distance ^= 1 << i;
        }
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::BitKey;

    fn make_node(id: u128) -> Node {
        Node {
            id: BitKey(id),
            udp_addr: "0.0.0.0:10".parse().unwrap(),
        }
    }

    #[test]
    fn kbucket_can_insert_max_size() {
        let max_size = 20;
        let mut bucket = KBucket::new(max_size);
        for x in 0..max_size {
            let node = make_node(x as u128);
            assert_eq!(KBucketInsert::Inserted, bucket.insert(node));
        }
    }

    #[test]
    fn kbucket_pings_first_inserted() {
        let max_size = 20;
        let mut bucket = KBucket::new(max_size);
        for x in 0..max_size {
            let node = Node {
                id: BitKey(x as u128),
                udp_addr: "0.0.0.0:10".parse().unwrap(),
            };
            bucket.insert(node);
        }
        assert_eq!(
            KBucketInsert::Ping(make_node(0)),
            bucket.insert(make_node(max_size as u128))
        );
    }

    #[test]
    fn kbucket_remove_replaces_waiting() {
        let max_size = 20;
        let mut bucket = KBucket::new(max_size);
        for x in 0..max_size {
            let node = make_node(x as u128);
            bucket.insert(node);
        }
        bucket.insert(make_node(max_size as u128));
        bucket.remove(BitKey(0));
        assert_eq!(Some(make_node(1)), bucket.data.pop_front());
        assert_eq!(Some(make_node(max_size as u128)), bucket.data.pop_back());
    }

    #[test]
    fn routing_table_can_insert() {
        let udp_addr = "127.0.0.1:1234".parse().unwrap();
        let this_node = Node {
            id: BitKey(0),
            udp_addr,
        };
        let mut table = RoutingTable::new(this_node, 20);
        for k in 0..KEY_SIZE {
            let id = BitKey(1 << k);
            let node = Node { id, udp_addr };
            assert_eq!(KBucketInsert::Inserted, table.insert(node));
        }
    }

    #[test]
    fn routing_table_closest_is_everything_when_small() {
        let max_size = 20;
        let this_node = make_node(0);
        let mut table = RoutingTable::new(this_node, max_size);
        let mut nodes = Vec::with_capacity(max_size as usize);
        nodes.push(this_node);
        for i in 0..(max_size - 1) {
            let node = make_node(1 << i);
            nodes.push(node);
            table.insert(node);
        }
        assert_eq!(nodes, table.k_closest(this_node.id, max_size as usize));
        assert_eq!(Vec::<Node>::new(), table.k_closest(this_node.id, 0));
        assert_eq!(vec![this_node], table.k_closest(this_node.id, 1));
    }
}
