use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq)]
enum KBucketInsert<T> {
    Inserted,
    Ping(T),
}

struct KBucket<T> {
    max_size: usize,
    waiting: Option<T>,
    data: VecDeque<T>,
}

impl<T: Clone + PartialEq> KBucket<T> {
    pub fn new(max_size: usize) -> Self {
        KBucket {
            max_size,
            waiting: None,
            data: VecDeque::with_capacity(max_size),
        }
    }

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

    pub fn successful_ping(&mut self) {
        self.waiting = None;
        if let Some(item) = self.data.pop_front() {
            self.data.push_back(item);
        }
    }

    pub fn failed_ping(&mut self) {
        // Normally this should only be called if we requested a ping,
        // and there's an item waiting, but we can just do nothing instead.
        if let Some(item) = self.waiting.take() {
            self.data.pop_front();
            self.data.push_back(item);
        }
    }
}

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
