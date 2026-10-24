//! Cover art cache port and bounded in-memory fake.

use std::collections::{HashMap, VecDeque};

pub const CAPACITY: usize = 64;

pub trait CoverCache {
    fn get(&self, url: &str) -> Option<Vec<u8>>;
    fn put(&mut self, url: &str, bytes: Vec<u8>);
}

#[derive(Debug, Default)]
pub struct MemoryCoverCache {
    entries: HashMap<String, Vec<u8>>,
    order: VecDeque<String>,
}

impl MemoryCoverCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn evict_oldest_if_full(&mut self) {
        if self.entries.len() >= CAPACITY {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

impl CoverCache for MemoryCoverCache {
    fn get(&self, url: &str) -> Option<Vec<u8>> {
        self.entries.get(url).cloned()
    }

    fn put(&mut self, url: &str, bytes: Vec<u8>) {
        if let Some(existing) = self.entries.get_mut(url) {
            *existing = bytes;
            return;
        }

        self.evict_oldest_if_full();
        self.order.push_back(url.to_owned());
        self.entries.insert(url.to_owned(), bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_get_put_overwrite_and_eviction() {
        let mut cache = MemoryCoverCache::new();
        assert_eq!(cache.get("k0"), None);
        cache.put("k0", vec![1]);
        cache.put("k0", vec![2]);
        assert_eq!(cache.get("k0"), Some(vec![2]));
        for i in 1..CAPACITY {
            cache.put(&format!("k{i}"), vec![i as u8]);
        }
        cache.put("k0", vec![9]);
        cache.put("k64", vec![64]);
        assert_eq!(cache.get("k0"), None);
        assert_eq!(cache.get("k1"), Some(vec![1]));
        assert_eq!(cache.get("k64"), Some(vec![64]));
    }
}
