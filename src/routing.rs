use std::collections::VecDeque;

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
    fn new(max_size: usize) -> Self {
        KBucket {
            max_size,
            waiting: None,
            data: VecDeque::with_capacity(max_size),
        }
    }

    fn insert(&mut self, item: T) -> KBucketInsert<T> {
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

    fn successful_ping(&mut self) {
        self.waiting = None;
        if let Some(item) = self.data.pop_front() {
            self.data.push_back(item);
        }
    }

    fn failed_ping(&mut self) {
        // Normally this should only be called if we requested a ping,
        // and there's an item waiting, but we can just do nothing instead.
        if let Some(item) = self.waiting.take() {
            self.data.pop_front();
            self.data.push_back(item);
        }
    }
}
