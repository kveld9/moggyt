//! Bounded in-memory FIFO cache decorator for lyrics retrieval.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use super::lyrics::{LyricsSource, SyncedLyrics};

/// Maximum number of cached lyrics entries before oldest entries are evicted.
pub const LYRICS_CACHE_CAPACITY: usize = 32;

/// Normalized lookup key combining artist name, track title, and duration.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LyricsKey {
    artist: String,
    title: String,
    duration_secs: Option<u64>,
}

impl LyricsKey {
    fn new(artist: &str, title: &str, duration_secs: Option<u64>) -> Self {
        Self {
            artist: artist.trim().to_lowercase(),
            title: title.trim().to_lowercase(),
            duration_secs,
        }
    }
}

/// Decorator caching lyrics lookups in-memory with bounded FIFO eviction.
#[derive(Debug)]
pub struct CachedLyrics<L> {
    inner: L,
    entries: Mutex<VecDeque<(LyricsKey, SyncedLyrics)>>,
}

impl<L> CachedLyrics<L> {
    /// Wraps an existing [`LyricsSource`] with an empty bounded cache.
    pub fn new(inner: L) -> Self {
        Self { inner, entries: Mutex::new(VecDeque::with_capacity(LYRICS_CACHE_CAPACITY)) }
    }

    /// Borrows the wrapped inner [`LyricsSource`].
    pub fn inner(&self) -> &L {
        &self.inner
    }

    fn lock_entries(&self) -> std::sync::MutexGuard<'_, VecDeque<(LyricsKey, SyncedLyrics)>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn get_cached(&self, key: &LyricsKey) -> Option<SyncedLyrics> {
        let entries = self.lock_entries();
        entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    fn insert_entry(&self, key: LyricsKey, lyrics: &SyncedLyrics) {
        let mut entries = self.lock_entries();
        if entries.iter().any(|(k, _)| k == &key) {
            return;
        }
        if entries.len() >= LYRICS_CACHE_CAPACITY {
            entries.pop_front();
        }
        entries.push_back((key, lyrics.clone()));
    }
}

impl<L: LyricsSource> LyricsSource for CachedLyrics<L> {
    /// Returns cached lyrics for `(artist, title, duration_secs)` or queries the inner source.
    ///
    /// On cache hit, returns a cloned [`SyncedLyrics`] without calling inner source.
    /// On miss, queries inner source without holding the mutex; only successful (`Some`)
    /// responses are cached to prevent caching transient network failures.
    fn lookup(
        &self,
        artist: &str,
        title: &str,
        duration_secs: Option<u64>,
    ) -> Option<SyncedLyrics> {
        let key = LyricsKey::new(artist, title, duration_secs);
        if let Some(cached) = self.get_cached(&key) {
            return Some(cached);
        }
        let lyrics = self.inner.lookup(artist, title, duration_secs)?;
        self.insert_entry(key, &lyrics);
        Some(lyrics)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::infra::LyricLine;

    struct CountingLyricsSource {
        calls: AtomicUsize,
        return_none: bool,
    }

    impl CountingLyricsSource {
        fn new() -> Self {
            Self { calls: AtomicUsize::new(0), return_none: false }
        }

        fn always_none() -> Self {
            Self { calls: AtomicUsize::new(0), return_none: true }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl LyricsSource for CountingLyricsSource {
        fn lookup(
            &self,
            artist: &str,
            title: &str,
            _duration_secs: Option<u64>,
        ) -> Option<SyncedLyrics> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.return_none {
                return None;
            }
            Some(SyncedLyrics { lines: vec![LyricLine::new(0, format!("{title} by {artist}"))] })
        }
    }

    #[test]
    fn second_identical_lookup_served_from_cache() {
        let fake = CountingLyricsSource::new();
        let cache = CachedLyrics::new(fake);

        let hit1 = cache.lookup("Radiohead", "Creep", Some(236));
        assert!(hit1.is_some());
        assert_eq!(cache.inner().call_count(), 1);

        let hit2 = cache.lookup("Radiohead", "Creep", Some(236));
        assert_eq!(hit1, hit2);
        assert_eq!(cache.inner().call_count(), 1);
    }

    #[test]
    fn key_normalization_hits_cache() {
        let fake = CountingLyricsSource::new();
        let cache = CachedLyrics::new(fake);

        let _ = cache.lookup("Radiohead", "Creep", Some(236));
        assert_eq!(cache.inner().call_count(), 1);

        let _ = cache.lookup("  radiohead  ", "  CREEP  ", Some(236));
        assert_eq!(cache.inner().call_count(), 1);
    }

    #[test]
    fn none_results_are_not_cached() {
        let fake = CountingLyricsSource::always_none();
        let cache = CachedLyrics::new(fake);

        let res1 = cache.lookup("Radiohead", "Creep", Some(236));
        assert_eq!(res1, None);
        assert_eq!(cache.inner().call_count(), 1);

        let res2 = cache.lookup("Radiohead", "Creep", Some(236));
        assert_eq!(res2, None);
        assert_eq!(cache.inner().call_count(), 2);
    }

    #[test]
    fn different_durations_are_distinct_keys() {
        let fake = CountingLyricsSource::new();
        let cache = CachedLyrics::new(fake);

        let _ = cache.lookup("Radiohead", "Creep", Some(100));
        assert_eq!(cache.inner().call_count(), 1);

        let _ = cache.lookup("Radiohead", "Creep", Some(200));
        assert_eq!(cache.inner().call_count(), 2);

        let _ = cache.lookup("Radiohead", "Creep", None);
        assert_eq!(cache.inner().call_count(), 3);
    }

    #[test]
    fn capacity_plus_one_evicts_oldest() {
        let fake = CountingLyricsSource::new();
        let cache = CachedLyrics::new(fake);

        for i in 0..LYRICS_CACHE_CAPACITY {
            let title = format!("Track {i}");
            let _ = cache.lookup("Artist", &title, Some(100));
        }
        assert_eq!(cache.inner().call_count(), LYRICS_CACHE_CAPACITY);

        let newest = format!("Track {LYRICS_CACHE_CAPACITY}");
        let _ = cache.lookup("Artist", &newest, Some(100));
        assert_eq!(cache.inner().call_count(), LYRICS_CACHE_CAPACITY + 1);

        let _ = cache.lookup("Artist", &newest, Some(100));
        assert_eq!(cache.inner().call_count(), LYRICS_CACHE_CAPACITY + 1);

        let last_of_first_batch = format!("Track {}", LYRICS_CACHE_CAPACITY - 1);
        let _ = cache.lookup("Artist", &last_of_first_batch, Some(100));
        assert_eq!(cache.inner().call_count(), LYRICS_CACHE_CAPACITY + 1);

        let _ = cache.lookup("Artist", "Track 0", Some(100));
        assert_eq!(cache.inner().call_count(), LYRICS_CACHE_CAPACITY + 2);
    }

    #[test]
    fn poisoned_mutex_recovery() {
        let fake = CountingLyricsSource::new();
        let cache = CachedLyrics::new(fake);

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = cache.entries.lock().unwrap();
            panic!("poisoning mutex");
        }));

        assert!(cache.entries.is_poisoned());
        let res = cache.lookup("Radiohead", "Creep", Some(236));
        assert!(res.is_some());
        assert_eq!(cache.inner().call_count(), 1);
    }
}
