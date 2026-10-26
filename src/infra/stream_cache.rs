//! Persistent disk stream cache with LRU eviction by file access time (mtime).
//!
//! # Collision Policy
//! Keys are derived using a 64-bit FNV-1a hash formatted as a 16-character hex string.
//! For expected stream cache sizes (hundreds to thousands of audio tracks), the statistical
//! collision probability under 64-bit hashing is approximately 1 in 10^12. In the extraordinarily
//! unlikely event of an FNV-1a digest collision between two distinct streaming URLs, the cache
//! will overwrite or serve the collocated cached stream until normal LRU eviction or session expiry.
//! Because streaming URLs from YouTube and yt-dlp are ephemeral signed URLs that rotate per session,
//! collisions carry zero risk of persistent database corruption and self-heal automatically upon eviction.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use super::cover_disk::fnv1a_hex;

/// Default stream cache storage budget in megabytes (512 MB).
///
/// At typical bitrates (128-256 kbps, ~1-2 MB per minute, ~4-8 MB per track),
/// 512 MB accommodates approximately 60 to 100 complete audio tracks for instant offline replay.
pub const DEFAULT_STREAM_CACHE_MB: u32 = 512;

/// Maximum permissible stream cache budget in megabytes (50 GB).
pub const STREAM_CACHE_MAX_MB: u32 = 50_000;

static TMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Computes the cache filename for an audio stream URL.
pub fn stream_cache_file_name(url: &str) -> String {
    format!("{}.stream", fnv1a_hex(url))
}

fn touch_path(path: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

struct CacheEntry {
    path: PathBuf,
    size: u64,
    mtime: SystemTime,
}

fn read_entry_info(entry: &std::fs::DirEntry) -> Option<CacheEntry> {
    let path = entry.path();
    if path.extension().and_then(|ext| ext.to_str()) != Some("stream") {
        return None;
    }
    let meta = entry.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    Some(CacheEntry { path, size: meta.len(), mtime })
}

fn collect_entries(dir: &Path) -> std::io::Result<Vec<CacheEntry>> {
    let read_dir = std::fs::read_dir(dir)?;
    let mut entries = Vec::new();
    for entry in read_dir {
        let Ok(entry) = entry else {
            continue;
        };
        if let Some(info) = read_entry_info(&entry) {
            entries.push(info);
        }
    }
    Ok(entries)
}

fn write_atomic(dir: &Path, file_name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let dest = dir.join(file_name);
    let count = TMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!("{file_name}.tmp.{}.{count}", std::process::id()));

    if let Err(err) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }

    if let Err(err) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }

    Ok(())
}

/// Disk stream cache managing persistent audio payloads with LRU eviction.
pub struct DiskStreamCache {
    dir: PathBuf,
    budget_bytes: u64,
}

impl DiskStreamCache {
    /// Initializes a disk stream cache at `dir`, creating directories if needed.
    pub fn new(dir: impl Into<PathBuf>, budget_bytes: u64) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir, budget_bytes })
    }

    /// Path to the directory backing the cache.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Configured maximum storage budget in bytes.
    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// Updates the storage budget and triggers eviction if over the new limit.
    pub fn set_budget_bytes(&mut self, budget: u64) -> std::io::Result<()> {
        self.budget_bytes = budget;
        self.evict_over_budget()
    }

    /// Computes the total byte size of all cached stream files on disk.
    pub fn total_bytes(&self) -> std::io::Result<u64> {
        let entries = collect_entries(&self.dir)?;
        Ok(entries.iter().map(|e| e.size).sum())
    }

    /// Clears all cached stream files from disk.
    pub fn clear(&self) -> std::io::Result<()> {
        let entries = collect_entries(&self.dir)?;
        for entry in entries {
            let _ = std::fs::remove_file(&entry.path);
        }
        Ok(())
    }

    /// Retrieves cached audio stream payload if present, updating its mtime on hit.
    pub fn get(&self, url: &str) -> Option<Vec<u8>> {
        if self.budget_bytes == 0 {
            return None;
        }
        let path = self.dir.join(stream_cache_file_name(url));
        let bytes = std::fs::read(&path).ok()?;
        if bytes.is_empty() {
            let _ = std::fs::remove_file(&path);
            return None;
        }
        touch_path(&path);
        Some(bytes)
    }

    /// Writes audio stream payload atomically to disk and evicts oldest entries if over budget.
    pub fn put(&mut self, url: &str, bytes: &[u8]) -> std::io::Result<()> {
        if self.budget_bytes == 0 || bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() as u64 > self.budget_bytes {
            return Ok(());
        }
        let file_name = stream_cache_file_name(url);
        write_atomic(&self.dir, &file_name, bytes)?;
        self.evict_over_budget()
    }

    /// Removes a cached stream payload from disk if present.
    pub fn remove(&self, url: &str) -> std::io::Result<()> {
        let path = self.dir.join(stream_cache_file_name(url));
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Evicts oldest files by access time until total size conforms to budget.
    pub fn evict_over_budget(&self) -> std::io::Result<()> {
        let mut entries = collect_entries(&self.dir)?;
        let mut total: u64 = entries.iter().map(|e| e.size).sum();
        if total <= self.budget_bytes {
            return Ok(());
        }
        entries.sort_by_key(|e| e.mtime);
        for entry in entries {
            if total <= self.budget_bytes {
                break;
            }
            let _ = std::fs::remove_file(&entry.path);
            total = total.saturating_sub(entry.size);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_entry_mtime(&self, url: &str, time: SystemTime) {
        let path = self.dir.join(stream_cache_file_name(url));
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = file.set_modified(time);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    struct TempDirGuard(PathBuf);

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn unique_temp_dir(tag: &str) -> (PathBuf, TempDirGuard) {
        static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "moggyt-test-stream-{}-{}-{count}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::remove_file(&path);
        (path.clone(), TempDirGuard(path))
    }

    #[test]
    fn stream_cache_file_name_mapping() {
        let name = stream_cache_file_name("https://example.com/audio.webm");
        assert!(name.ends_with(".stream"));
        assert_eq!(name, format!("{}.stream", fnv1a_hex("https://example.com/audio.webm")));
    }

    #[test]
    fn put_get_round_trip_and_missing_key() {
        let (dir, _guard) = unique_temp_dir("roundtrip");
        let mut cache = DiskStreamCache::new(&dir, 1024 * 1024).expect("valid dir");

        let url = "https://example.com/stream1";
        assert_eq!(cache.get(url), None);

        let data = vec![1, 2, 3, 4, 5];
        cache.put(url, &data).expect("put succeeds");
        assert_eq!(cache.get(url), Some(data));
        assert_eq!(cache.total_bytes().expect("total"), 5);

        assert_eq!(cache.get("https://example.com/other"), None);
    }

    #[test]
    fn lru_evicts_oldest_mtime_first_when_over_budget() {
        let (dir, _guard) = unique_temp_dir("lru-mtime");
        let mut cache = DiskStreamCache::new(&dir, 30).expect("valid dir");

        let url1 = "https://example.com/s1";
        let url2 = "https://example.com/s2";
        let url3 = "https://example.com/s3";
        let url4 = "https://example.com/s4";

        cache.put(url1, &[1; 10]).expect("put s1");
        cache.put(url2, &[2; 10]).expect("put s2");
        cache.put(url3, &[3; 10]).expect("put s3");
        assert_eq!(cache.total_bytes().expect("total"), 30);

        let now = SystemTime::now();
        cache.set_entry_mtime(url1, now - Duration::from_secs(300));
        cache.set_entry_mtime(url2, now - Duration::from_secs(200));
        cache.set_entry_mtime(url3, now - Duration::from_secs(100));

        cache.put(url4, &[4; 10]).expect("put s4");

        assert_eq!(cache.get(url1), None, "oldest entry s1 must be evicted");
        assert_eq!(cache.get(url2), Some(vec![2; 10]));
        assert_eq!(cache.get(url3), Some(vec![3; 10]));
        assert_eq!(cache.get(url4), Some(vec![4; 10]));
        assert_eq!(cache.total_bytes().expect("total"), 30);
    }

    #[test]
    fn touch_on_hit_protects_recent_entries() {
        let (dir, _guard) = unique_temp_dir("touch-protect");
        let mut cache = DiskStreamCache::new(&dir, 30).expect("valid dir");

        let url1 = "https://example.com/s1";
        let url2 = "https://example.com/s2";
        let url3 = "https://example.com/s3";
        let url4 = "https://example.com/s4";

        cache.put(url1, &[1; 10]).expect("put s1");
        cache.put(url2, &[2; 10]).expect("put s2");
        cache.put(url3, &[3; 10]).expect("put s3");

        let now = SystemTime::now();
        cache.set_entry_mtime(url1, now - Duration::from_secs(300));
        cache.set_entry_mtime(url2, now - Duration::from_secs(200));
        cache.set_entry_mtime(url3, now - Duration::from_secs(100));

        // Touch url1 via get: updates its mtime to now, making url2 the oldest
        assert_eq!(cache.get(url1), Some(vec![1; 10]));

        cache.put(url4, &[4; 10]).expect("put s4");

        assert_eq!(cache.get(url1), Some(vec![1; 10]), "touched url1 must survive");
        assert_eq!(cache.get(url2), None, "url2 became oldest and must be evicted");
        assert_eq!(cache.get(url3), Some(vec![3; 10]));
        assert_eq!(cache.get(url4), Some(vec![4; 10]));
    }

    #[test]
    fn budget_zero_bypasses_disk() {
        let (dir, _guard) = unique_temp_dir("budget-zero");
        let mut cache = DiskStreamCache::new(&dir, 0).expect("valid dir");

        let url = "https://example.com/stream-zero";
        cache.put(url, &[1, 2, 3, 4]).expect("put succeeds");

        assert_eq!(cache.get(url), None);
        assert_eq!(cache.total_bytes().expect("total"), 0);

        let dest = dir.join(stream_cache_file_name(url));
        assert!(!dest.exists(), "no file should be written with budget 0");
    }

    #[test]
    fn corrupt_empty_file_treated_as_miss_and_deleted() {
        let (dir, _guard) = unique_temp_dir("corrupt-empty");
        let cache = DiskStreamCache::new(&dir, 1024).expect("valid dir");

        let url = "https://example.com/empty";
        let file_path = dir.join(stream_cache_file_name(url));
        std::fs::write(&file_path, b"").expect("write empty file");
        assert!(file_path.exists());

        assert_eq!(cache.get(url), None, "empty file must return None");
        assert!(!file_path.exists(), "empty file must be deleted");
    }

    #[test]
    fn atomic_write_leaves_no_partial_files() {
        let (dir, _guard) = unique_temp_dir("atomic-clean");
        let mut cache = DiskStreamCache::new(&dir, 1024).expect("valid dir");

        let url = "https://example.com/clean";
        cache.put(url, &[42; 64]).expect("put succeeds");

        let entries = std::fs::read_dir(&dir).expect("read dir");
        let mut stream_count = 0;
        let mut tmp_count = 0;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".stream") {
                stream_count += 1;
            }
            if name.contains(".tmp.") {
                tmp_count += 1;
            }
        }

        assert_eq!(stream_count, 1);
        assert_eq!(tmp_count, 0, "no lingering tmp files");
    }

    #[test]
    fn clear_and_remove_operations() {
        let (dir, _guard) = unique_temp_dir("clear-remove");
        let mut cache = DiskStreamCache::new(&dir, 1024).expect("valid dir");

        let url1 = "https://example.com/c1";
        let url2 = "https://example.com/c2";
        cache.put(url1, &[1; 20]).expect("put c1");
        cache.put(url2, &[2; 30]).expect("put c2");
        assert_eq!(cache.total_bytes().expect("total"), 50);

        cache.remove(url1).expect("remove c1");
        assert_eq!(cache.get(url1), None);
        assert_eq!(cache.total_bytes().expect("total"), 30);

        cache.clear().expect("clear");
        assert_eq!(cache.get(url2), None);
        assert_eq!(cache.total_bytes().expect("total"), 0);
    }

    #[test]
    fn set_budget_bytes_evicts_down_to_new_budget() {
        let (dir, _guard) = unique_temp_dir("shrink-budget");
        let mut cache = DiskStreamCache::new(&dir, 100).expect("valid dir");

        let url1 = "https://example.com/e1";
        let url2 = "https://example.com/e2";
        cache.put(url1, &[1; 30]).expect("put e1");
        cache.put(url2, &[2; 30]).expect("put e2");
        assert_eq!(cache.total_bytes().expect("total"), 60);

        let now = SystemTime::now();
        cache.set_entry_mtime(url1, now - Duration::from_secs(100));
        cache.set_entry_mtime(url2, now);

        cache.set_budget_bytes(35).expect("shrink budget");
        assert_eq!(cache.get(url1), None, "oldest entry evicted on budget reduction");
        assert_eq!(cache.get(url2), Some(vec![2; 30]));
        assert_eq!(cache.total_bytes().expect("total"), 30);
    }

    #[test]
    fn large_item_exceeding_budget_bypasses_disk() {
        let (dir, _guard) = unique_temp_dir("exceed-budget");
        let mut cache = DiskStreamCache::new(&dir, 50).expect("valid dir");

        let url = "https://example.com/giant";
        cache.put(url, &[9; 100]).expect("put oversized");
        assert_eq!(cache.get(url), None);
        assert_eq!(cache.total_bytes().expect("total"), 0);
    }
}
