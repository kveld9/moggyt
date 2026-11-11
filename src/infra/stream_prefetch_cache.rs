//! Bounded in-memory cache of the first audio blocks of prefetched tracks.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

/// Number of blocks prefetched for the next track.
pub const PREFETCH_BLOCK_COUNT: u64 = 1;
/// Cached bytes per URL (one 256 KiB block).
pub const PREFETCH_BYTES: usize = super::stream_pool::STREAM_BLOCK_BYTES;
/// Maximum number of prefetched tracks retained.
pub const PREFETCH_CAPACITY: usize = 2;

/// Bounded FIFO cache holding leading audio bytes for prefetched URLs.
#[derive(Debug, Default)]
pub struct StreamPrefetchCache {
    entries: Mutex<VecDeque<(String, Vec<u8>)>>,
}

impl Clone for StreamPrefetchCache {
    fn clone(&self) -> Self {
        let guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        Self { entries: Mutex::new(guard.clone()) }
    }
}

impl StreamPrefetchCache {
    /// Creates a new empty prefetch cache.
    pub fn new() -> Self {
        Self { entries: Mutex::new(VecDeque::with_capacity(PREFETCH_CAPACITY)) }
    }

    /// Inserts or replaces prefetched prefix bytes for `url`.
    ///
    /// Replaces an existing URL entry (moving it to the back), truncates `bytes`
    /// to `PREFETCH_BYTES`, and evicts the oldest entry when at `PREFETCH_CAPACITY`.
    /// Blank URLs are ignored.
    pub fn insert(&self, url: &str, mut bytes: Vec<u8>) {
        let url = url.trim();
        if url.is_empty() {
            return;
        }
        if bytes.len() > PREFETCH_BYTES {
            bytes.truncate(PREFETCH_BYTES);
        }
        let mut guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(pos) = guard.iter().position(|(u, _)| u == url) {
            guard.remove(pos);
        } else if guard.len() >= PREFETCH_CAPACITY {
            guard.pop_front();
        }
        guard.push_back((url.to_string(), bytes));
    }

    /// Removes and returns the prefetched prefix bytes for `url` (single use).
    pub fn take(&self, url: &str) -> Option<Vec<u8>> {
        let url = url.trim();
        if url.is_empty() {
            return None;
        }
        let mut guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let pos = guard.iter().position(|(u, _)| u == url)?;
        guard.remove(pos).map(|(_, bytes)| bytes)
    }

    /// Returns whether the cache holds an entry for `url`.
    pub fn contains(&self, url: &str) -> bool {
        let url = url.trim();
        if url.is_empty() {
            return false;
        }
        let guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        guard.iter().any(|(u, _)| u == url)
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    /// Returns true if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_take_roundtrip() {
        let cache = StreamPrefetchCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        cache.insert("https://example.com/stream1", vec![1, 2, 3, 4]);
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_empty());
        assert!(cache.contains("https://example.com/stream1"));

        let taken = cache.take("https://example.com/stream1");
        assert_eq!(taken, Some(vec![1, 2, 3, 4]));
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn take_is_single_use() {
        let cache = StreamPrefetchCache::new();
        cache.insert("https://example.com/once", vec![10, 20]);

        assert_eq!(cache.take("https://example.com/once"), Some(vec![10, 20]));
        assert_eq!(cache.take("https://example.com/once"), None);
    }

    #[test]
    fn capacity_eviction_evicts_oldest() {
        let cache = StreamPrefetchCache::new();
        cache.insert("https://example.com/1", vec![1]);
        cache.insert("https://example.com/2", vec![2]);
        assert_eq!(cache.len(), 2);

        // Third URL evicts the oldest (1)
        cache.insert("https://example.com/3", vec![3]);
        assert_eq!(cache.len(), 2);

        assert_eq!(cache.take("https://example.com/1"), None);
        assert_eq!(cache.take("https://example.com/2"), Some(vec![2]));
        assert_eq!(cache.take("https://example.com/3"), Some(vec![3]));
    }

    #[test]
    fn capacity_eviction_replaces_and_moves_to_back() {
        let cache = StreamPrefetchCache::new();
        cache.insert("https://example.com/1", vec![1]);
        cache.insert("https://example.com/2", vec![2]);

        // Re-inserting 1 updates it and moves it to the back
        cache.insert("https://example.com/1", vec![11]);
        assert_eq!(cache.len(), 2);

        // Inserting 3 now evicts 2 (since 1 was touched and moved to the back)
        cache.insert("https://example.com/3", vec![3]);
        assert_eq!(cache.len(), 2);

        assert_eq!(cache.take("https://example.com/2"), None);
        assert_eq!(cache.take("https://example.com/1"), Some(vec![11]));
        assert_eq!(cache.take("https://example.com/3"), Some(vec![3]));
    }

    #[test]
    fn byte_truncation_limits_to_prefetch_bytes() {
        let cache = StreamPrefetchCache::new();
        let oversized = vec![0xAB; PREFETCH_BYTES + 1024];
        cache.insert("https://example.com/oversized", oversized);

        let taken = cache.take("https://example.com/oversized").expect("entry present");
        assert_eq!(taken.len(), PREFETCH_BYTES);
        assert!(taken.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn poison_recovery_preserves_functionality() {
        let cache = StreamPrefetchCache::new();
        let clone = cache.clone();

        let _ = std::thread::spawn(move || {
            let _guard = clone.entries.lock().unwrap();
            panic!("deliberate panic to poison mutex");
        })
        .join();

        // Mutex is poisoned; operations should recover seamlessly
        cache.insert("https://example.com/poison-recovered", vec![42]);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.take("https://example.com/poison-recovered"), Some(vec![42]));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn blank_url_rejected_on_insert() {
        let cache = StreamPrefetchCache::new();
        cache.insert("", vec![1, 2, 3]);
        cache.insert("   ", vec![4, 5, 6]);

        assert!(cache.is_empty());
        assert_eq!(cache.take(""), None);
        assert_eq!(cache.take("   "), None);
        assert!(!cache.contains(""));
        assert!(!cache.contains("   "));
    }
}
