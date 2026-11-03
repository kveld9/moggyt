//! In-memory bounded cache for resolved audio stream URLs with TTL and capacity eviction.
//!
//! Hexagonal invariant: pure in-memory cache with injected clock timestamps,
//! zero network I/O, and zero audio dependencies.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::stream::AudioQuality;

/// Duration for which a resolved stream URL remains valid (20 minutes).
///
/// Set well below the ~6 hour googlevideo URL expiry window to ensure
/// playback streams never stall on stale pre-resolved links.
pub const STREAM_URL_TTL: Duration = Duration::from_secs(20 * 60);

/// Maximum number of resolved stream URLs retained in memory before oldest eviction.
pub const STREAM_URL_CACHE_CAPACITY: usize = 8;

#[derive(Debug, Clone)]
struct CachedUrl {
    video_id: String,
    quality: AudioQuality,
    url: String,
    stored_at: Instant,
}

/// Thread-safe in-memory cache of resolved stream URLs.
///
/// Clones share the underlying entry storage via [`Arc`].
#[derive(Debug, Clone, Default)]
pub struct StreamUrlCache {
    entries: Arc<Mutex<Vec<CachedUrl>>>,
}

fn evict_oldest(entries: &mut Vec<CachedUrl>) {
    if let Some(oldest_idx) =
        entries.iter().enumerate().min_by_key(|(_, e)| e.stored_at).map(|(i, _)| i)
    {
        entries.remove(oldest_idx);
    }
}

impl StreamUrlCache {
    /// Creates an empty stream URL cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_entries(&self) -> std::sync::MutexGuard<'_, Vec<CachedUrl>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Retrieves a cached stream URL if present, matching quality, and not expired.
    ///
    /// Drops any expired entries encountered during cache lookup.
    pub fn get(&self, video_id: &str, quality: AudioQuality, now: Instant) -> Option<String> {
        let mut entries = self.lock_entries();
        entries.retain(|e| now.saturating_duration_since(e.stored_at) < STREAM_URL_TTL);
        entries
            .iter()
            .find(|e| e.video_id == video_id && e.quality == quality)
            .map(|e| e.url.clone())
    }

    /// Inserts or replaces a resolved stream URL for the given video ID and quality.
    ///
    /// Replaces existing entries for the same video ID and quality; evicts the oldest
    /// entry when inserting a new key at capacity.
    pub fn insert(&self, video_id: &str, quality: AudioQuality, url: String, now: Instant) {
        let mut entries = self.lock_entries();
        if let Some(existing) =
            entries.iter_mut().find(|e| e.video_id == video_id && e.quality == quality)
        {
            existing.url = url;
            existing.stored_at = now;
            return;
        }
        if entries.len() >= STREAM_URL_CACHE_CAPACITY {
            evict_oldest(&mut entries);
        }
        entries.push(CachedUrl { video_id: video_id.to_string(), quality, url, stored_at: now });
    }

    /// Invalidates all cached stream URLs for `video_id` regardless of quality.
    pub fn invalidate(&self, video_id: &str) {
        let mut entries = self.lock_entries();
        entries.retain(|e| e.video_id != video_id);
    }

    /// Returns the number of entries currently stored in the cache.
    pub fn len(&self) -> usize {
        self.lock_entries().len()
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
    fn cache_hit_returns_stored_url() {
        let cache = StreamUrlCache::new();
        let now = Instant::now();
        cache.insert("vid1", AudioQuality::Standard, "https://stream.test/1".into(), now);

        let hit = cache.get("vid1", AudioQuality::Standard, now + Duration::from_secs(10));
        assert_eq!(hit.as_deref(), Some("https://stream.test/1"));
    }

    #[test]
    fn cache_miss_on_different_quality() {
        let cache = StreamUrlCache::new();
        let now = Instant::now();
        cache.insert("vid1", AudioQuality::Standard, "https://stream.test/1".into(), now);

        assert_eq!(cache.get("vid1", AudioQuality::High, now), None);
        assert_eq!(cache.get("vid1", AudioQuality::Compact, now), None);
    }

    #[test]
    fn cache_expiry_at_ttl_and_drops_expired_entries() {
        let cache = StreamUrlCache::new();
        let now = Instant::now();
        cache.insert("vid1", AudioQuality::Standard, "https://stream.test/1".into(), now);

        // Just before TTL -> hit
        let before_ttl = now + STREAM_URL_TTL - Duration::from_millis(1);
        assert!(cache.get("vid1", AudioQuality::Standard, before_ttl).is_some());
        assert_eq!(cache.len(), 1);

        // At TTL -> expired and dropped
        let at_ttl = now + STREAM_URL_TTL;
        assert_eq!(cache.get("vid1", AudioQuality::Standard, at_ttl), None);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn cache_replace_same_key_updates_url_and_timestamp() {
        let cache = StreamUrlCache::new();
        let now = Instant::now();
        cache.insert("vid1", AudioQuality::Standard, "https://stream.test/old".into(), now);
        assert_eq!(cache.len(), 1);

        let later = now + Duration::from_secs(5);
        cache.insert("vid1", AudioQuality::Standard, "https://stream.test/new".into(), later);
        assert_eq!(cache.len(), 1);

        assert_eq!(
            cache.get("vid1", AudioQuality::Standard, later).as_deref(),
            Some("https://stream.test/new")
        );
    }

    #[test]
    fn cache_capacity_eviction_removes_oldest() {
        let cache = StreamUrlCache::new();
        let now = Instant::now();

        // Fill cache up to STREAM_URL_CACHE_CAPACITY (8 entries)
        for i in 0..STREAM_URL_CACHE_CAPACITY {
            cache.insert(
                &format!("vid{i}"),
                AudioQuality::Standard,
                format!("https://stream.test/{i}"),
                now + Duration::from_secs(i as u64),
            );
        }
        assert_eq!(cache.len(), STREAM_URL_CACHE_CAPACITY);

        // Insert 9th entry at later timestamp -> oldest (vid0) must be evicted
        let t9 = now + Duration::from_secs(STREAM_URL_CACHE_CAPACITY as u64);
        cache.insert("vid8", AudioQuality::Standard, "https://stream.test/8".into(), t9);
        assert_eq!(cache.len(), STREAM_URL_CACHE_CAPACITY);

        // vid0 is gone
        assert_eq!(cache.get("vid0", AudioQuality::Standard, t9), None);
        // vid1 through vid8 remain
        for i in 1..=STREAM_URL_CACHE_CAPACITY {
            assert!(cache.get(&format!("vid{i}"), AudioQuality::Standard, t9).is_some());
        }
    }

    #[test]
    fn cache_invalidate_removes_all_qualities_for_id() {
        let cache = StreamUrlCache::new();
        let now = Instant::now();
        cache.insert("vid1", AudioQuality::Standard, "https://stream.test/std".into(), now);
        cache.insert("vid1", AudioQuality::High, "https://stream.test/hi".into(), now);
        cache.insert("vid2", AudioQuality::Standard, "https://stream.test/2".into(), now);
        assert_eq!(cache.len(), 3);

        cache.invalidate("vid1");
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get("vid1", AudioQuality::Standard, now), None);
        assert_eq!(cache.get("vid1", AudioQuality::High, now), None);
        assert_eq!(
            cache.get("vid2", AudioQuality::Standard, now).as_deref(),
            Some("https://stream.test/2")
        );
    }

    #[test]
    fn cache_clones_share_storage() {
        let cache1 = StreamUrlCache::new();
        let cache2 = cache1.clone();
        let now = Instant::now();

        cache1.insert("vid1", AudioQuality::Standard, "https://stream.test/1".into(), now);
        assert_eq!(
            cache2.get("vid1", AudioQuality::Standard, now).as_deref(),
            Some("https://stream.test/1")
        );

        cache2.invalidate("vid1");
        assert_eq!(cache1.get("vid1", AudioQuality::Standard, now), None);
    }

    #[test]
    fn cache_poisoned_mutex_recovery() {
        let cache = StreamUrlCache::new();
        let now = Instant::now();
        cache.insert("vid1", AudioQuality::Standard, "https://stream.test/1".into(), now);

        let c_clone = cache.clone();
        let _ = std::thread::spawn(move || {
            let _guard = c_clone.lock_entries();
            panic!("poisoning mutex");
        })
        .join();

        // Mutex is poisoned, but operations recover cleanly
        assert_eq!(
            cache.get("vid1", AudioQuality::Standard, now).as_deref(),
            Some("https://stream.test/1")
        );
        cache.insert("vid2", AudioQuality::High, "https://stream.test/2".into(), now);
        assert_eq!(cache.len(), 2);
        cache.invalidate("vid1");
        assert_eq!(cache.len(), 1);
    }
}
