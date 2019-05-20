use std::collections::VecDeque;

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
    /// [successful_ping](struct.KBucket.html#method.succcessful_ping),
    /// otherwise we call
    /// [failed_ping](struct.KBucket.html#method.failed_ping).
    Ping(T),
}

/// This represents a KBucket used in the Kademlia DHT.
/// 
/// Each KBucket is used to store a fixed size set of nodes.
/// New nodes can be added into the bucket until it fills up, at which
/// point new nodes will only be added if a node dies.
/// We have this preference for long-lived nodes, since the longer a node
/// lives, the longer it tends to stay alive as well.
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
    /// If the bucket still has room left, we just insert the element
    /// directly, and `Inserted` is returned. If we can't insert the element,
    /// then we return an element that needs to be pinged to check if it's
    /// still alive. After performing that check, one of the ping
    /// methods on this struct should be called to finalize insertion.
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
