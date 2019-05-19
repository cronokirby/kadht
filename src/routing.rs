enum KBucketInsert<T> {
    Inserted,
    Ping(T)
}

struct KBucket<T> {
    max_size: usize,
    data: Vec<T>,
}

impl<T: Clone + PartialEq> KBucket<T> {
    fn new(max_size: usize) -> Self {
        KBucket {
            max_size,
            data: Vec::with_capacity(max_size),
        }
    }

    fn insert(&mut self, item: T) -> KBucketInsert<T> {
        let existing = self.data.iter().position(|x| *x == item);
        if let Some(index) = existing {
            self.data.remove(index);
        }
        if self.data.len() < self.max_size {
            self.data.push(item);
            return KBucketInsert::Inserted;
        } else {
            return KBucketInsert::Ping(self.data[0].clone());
        }
    }
}
