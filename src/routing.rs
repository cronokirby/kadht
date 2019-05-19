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
    // When we try to insert a node into a full bucket, we need
    // to check whether or not the oldest node is still alive by
    // pinging it across the network. Since this happens after inserting,
    // we store the node awaiting insertion in this variable while we
    // wait until one of the ping methods is called again.
    waiting: Option<T>,
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
            waiting: None,
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
            self.waiting = Some(item);
            return KBucketInsert::Ping(self.data[0].clone());
        }
    }

    /// Report that the ping requested succeeded.
    /// 
    /// This should be called after being requested to ping by the insert
    /// method, and then receiving a timely response from the node.
    /// 
    /// This will clear whatever node was waiting to be inserted, since
    /// it cannot take the place of a dead node.
    pub fn successful_ping(&mut self) {
        self.waiting = None;
        if let Some(item) = self.data.pop_front() {
            self.data.push_back(item);
        }
    }

    /// Report that the ping requested failed.
    /// 
    /// This should be called after being requested to ping by the insert
    /// method, and then failing to receive a timely response from the node.
    /// 
    /// This will insert the node that was waiting to be inserted in
    /// the bucker, since it can replace the node that we knew has died.
    pub fn failed_ping(&mut self) {
        // Normally this should only be called if we requested a ping,
        // and there's an item waiting, but we can just do nothing instead.
        if let Some(item) = self.waiting.take() {
            self.data.pop_front();
            self.data.push_back(item);
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
    fn kbucket_handles_successful_pings() {
        let max_size = 20;
        let mut bucket: KBucket<usize> = KBucket::new(max_size);
        for x in 0..max_size {
            bucket.insert(x);
        }
        bucket.successful_ping();
        assert_eq!(Some(1), bucket.data.pop_front());
        assert_eq!(Some(0), bucket.data.pop_back());
    }

    #[test]
    fn kbucket_handles_failed_pings() {
        let max_size = 20;
        let mut bucket: KBucket<usize> = KBucket::new(max_size);
        for x in 0..max_size {
            bucket.insert(x);
        }
        bucket.insert(max_size);
        bucket.failed_ping();
        assert_eq!(Some(1), bucket.data.pop_front());
        assert_eq!(Some(max_size), bucket.data.pop_back());
    }
}
