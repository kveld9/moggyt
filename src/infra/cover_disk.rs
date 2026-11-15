//! Two-tier cover cache (bounded memory + quota-bounded disk).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use super::cover::{CoverCache, MemoryCoverCache};

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
static TMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Computes the 64-bit FNV-1a hash of a string, formatted as a 16-character lowercase hex string.
pub fn fnv1a_hex(input: &str) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in input.as_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// Computes the cache filename for a cover art URL.
pub fn cache_file_name(url: &str) -> String {
    format!("{}.cover", fnv1a_hex(url))
}

/// Default disk quota for cover art (256 MiB).
pub const DEFAULT_MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
struct CacheEntry {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

/// Two-tier cover cache: bounded in-memory cache plus an LRU-bounded disk tier.
///
/// The disk tier keeps `.cover` files under a byte quota. The least-recently-used
/// entries (by modification time) are pruned when a write pushes the total over the
/// quota; `get` refreshes the mtime so hot artwork survives pruning.
#[derive(Debug)]
pub struct DiskCoverCache {
    memory: MemoryCoverCache,
    dir: PathBuf,
    max_bytes: u64,
    total_bytes: u64,
}

impl DiskCoverCache {
    /// Initializes a disk cover cache at `dir` with the default 256 MiB quota.
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        Self::with_limit(dir, DEFAULT_MAX_BYTES)
    }

    /// Initializes a disk cover cache at `dir` with an explicit byte quota.
    ///
    /// Existing files are pruned to the quota on initialization. A zero quota
    /// disables the disk tier and clears any existing files.
    pub fn with_limit(dir: impl Into<PathBuf>, max_bytes: u64) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let mut cache = Self { memory: MemoryCoverCache::new(), dir, max_bytes, total_bytes: 0 };
        if let Err(err) = cache.prune_lru() {
            tracing::warn!(error = %err, "failed to prune cover cache on startup");
        }
        Ok(cache)
    }

    /// Path to the directory backing the cache.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Configured maximum byte budget for the disk tier.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Total bytes currently stored across all `.cover` files.
    pub fn total_size(&self) -> u64 {
        self.collect_entries().map(|(_, total)| total).unwrap_or(0)
    }

    fn collect_entries(&self) -> std::io::Result<(Vec<CacheEntry>, u64)> {
        let read_dir = match std::fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
            Err(err) => return Err(err),
        };

        let mut entries = Vec::new();
        let mut total_size: u64 = 0;

        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "cover") {
                let metadata = entry.metadata()?;
                let size = metadata.len();
                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                total_size = total_size.saturating_add(size);
                entries.push(CacheEntry { path, size, modified });
            }
        }

        Ok((entries, total_size))
    }

    fn prune_lru(&mut self) -> std::io::Result<()> {
        let (mut entries, mut total_size) = self.collect_entries()?;
        self.total_bytes = total_size;
        if total_size <= self.max_bytes {
            return Ok(());
        }

        entries.sort_by(|a, b| a.modified.cmp(&b.modified).then_with(|| a.path.cmp(&b.path)));

        for entry in entries {
            if total_size <= self.max_bytes {
                break;
            }
            if std::fs::remove_file(&entry.path).is_ok() {
                total_size = total_size.saturating_sub(entry.size);
            }
        }

        self.total_bytes = total_size;
        Ok(())
    }

    fn disk_len(&self, url: &str) -> u64 {
        std::fs::metadata(self.dir.join(cache_file_name(url))).map(|m| m.len()).unwrap_or(0)
    }

    fn touch_mtime(&self, path: &Path) {
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = file.set_modified(SystemTime::now());
        }
    }

    fn write_to_disk(&self, url: &str, bytes: &[u8]) -> std::io::Result<()> {
        let file_name = cache_file_name(url);
        let dest = self.dir.join(&file_name);
        let count = TMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = self.dir.join(format!("{file_name}.tmp.{}.{count}", std::process::id()));

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
}

impl CoverCache for DiskCoverCache {
    fn get(&self, url: &str) -> Option<Vec<u8>> {
        if let Some(bytes) = self.memory.get(url) {
            return Some(bytes);
        }

        let path = self.dir.join(cache_file_name(url));
        let bytes = std::fs::read(&path).ok()?;
        self.touch_mtime(&path);
        Some(bytes)
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn put(&mut self, url: &str, bytes: Vec<u8>) {
        self.memory.put(url, bytes.clone());
        if self.max_bytes == 0 {
            return;
        }
        let previous = self.disk_len(url);
        if let Err(err) = self.write_to_disk(url, &bytes) {
            tracing::warn!(url = %url, error = %err, "failed to persist cover art to disk");
            return;
        }
        self.total_bytes =
            self.total_bytes.saturating_sub(previous).saturating_add(bytes.len() as u64);
        if self.total_bytes > self.max_bytes {
            if let Err(err) = self.prune_lru() {
                tracing::warn!(error = %err, "failed to prune cover cache");
            }
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

    fn unique_temp_path(tag: &str) -> (PathBuf, TempDirGuard) {
        static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);
        let count = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "moggyt-test-cover-{}-{}-{count}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::remove_file(&path);
        (path.clone(), TempDirGuard(path))
    }

    #[test]
    fn fnv1a_hex_test_vectors() {
        let cases =
            [("", "cbf29ce484222325"), ("a", "af63dc4c8601ec8c"), ("foobar", "85944171f73967e8")];

        for (input, expected) in cases {
            assert_eq!(fnv1a_hex(input), expected, "FNV-1a mismatch for input: {input:?}");
        }
    }

    #[test]
    fn cache_file_name_mapping() {
        assert_eq!(cache_file_name("a"), "af63dc4c8601ec8c.cover");
    }

    fn write_cover_file(dir: &Path, name: &str, size: usize, modified: SystemTime) {
        let path = dir.join(name);
        std::fs::write(&path, vec![0u8; size]).expect("write cover file");
        let file = std::fs::OpenOptions::new().write(true).open(&path).expect("open cover file");
        file.set_modified(modified).expect("set cover mtime");
    }

    #[test]
    fn with_limit_prunes_oversized_directory_on_open() {
        let (dir, _guard) = unique_temp_path("quota-open");
        std::fs::create_dir_all(&dir).expect("create dir");
        let now = SystemTime::now();
        write_cover_file(&dir, "old.cover", 40, now - Duration::from_secs(120));
        write_cover_file(&dir, "mid.cover", 40, now - Duration::from_secs(60));
        write_cover_file(&dir, "new.cover", 40, now);

        let cache = DiskCoverCache::with_limit(&dir, 80).expect("valid dir");
        assert_eq!(cache.max_bytes(), 80);
        assert!(cache.total_size() <= 80);
        assert!(!dir.join("old.cover").is_file(), "oldest file must be pruned at startup");
        assert!(dir.join("new.cover").is_file());
    }

    #[test]
    fn put_prunes_least_recently_used_when_over_quota() {
        let (dir, _guard) = unique_temp_path("quota-put");
        let mut cache = DiskCoverCache::with_limit(&dir, 100).expect("valid dir");
        let a = "https://example.com/a.jpg";
        let b = "https://example.com/b.jpg";
        let c = "https://example.com/c.jpg";

        cache.put(a, vec![1; 40]);
        cache.put(b, vec![2; 40]);
        assert_eq!(cache.total_size(), 80);

        let path_a = dir.join(cache_file_name(a));
        let file_a = std::fs::OpenOptions::new().write(true).open(&path_a).expect("open a");
        file_a.set_modified(SystemTime::now() - Duration::from_secs(60)).expect("age file a");

        cache.put(c, vec![3; 40]);
        assert!(cache.total_size() <= 100);
        assert!(!path_a.is_file(), "oldest file must be evicted");
        assert!(dir.join(cache_file_name(c)).is_file());
    }

    #[test]
    fn get_touches_mtime_so_hot_entries_survive_pruning() {
        let (dir, _guard) = unique_temp_path("quota-get");
        std::fs::create_dir_all(&dir).expect("create dir");
        let now = SystemTime::now();
        let a = "https://example.com/hot.jpg";
        let b = "https://example.com/cold.jpg";
        write_cover_file(&dir, &cache_file_name(a), 40, now - Duration::from_secs(120));
        write_cover_file(&dir, &cache_file_name(b), 40, now - Duration::from_secs(120));

        let mut cache = DiskCoverCache::with_limit(&dir, 100).expect("valid dir");
        assert_eq!(cache.get(a), Some(vec![0; 40]), "disk read");
        cache.put("https://example.com/new.jpg", vec![3; 40]);

        assert!(dir.join(cache_file_name(a)).is_file(), "hot entry survives");
        assert!(!dir.join(cache_file_name(b)).is_file(), "cold entry is evicted");
    }

    #[test]
    fn zero_quota_disables_disk_writes_and_clears_existing() {
        let (dir, _guard) = unique_temp_path("quota-zero");
        let old = "https://example.com/old.jpg";
        let mut warm = DiskCoverCache::new(&dir).expect("valid dir");
        warm.put(old, vec![9; 32]);
        assert!(dir.join(cache_file_name(old)).is_file());
        drop(warm);

        let mut cache = DiskCoverCache::with_limit(&dir, 0).expect("valid dir");
        assert_eq!(cache.total_size(), 0, "startup prune clears the disk tier");
        let new = "https://example.com/new.jpg";
        cache.put(new, vec![1; 32]);
        assert_eq!(cache.get(new), Some(vec![1; 32]), "memory tier still works");
        assert!(!dir.join(cache_file_name(new)).is_file(), "zero quota skips disk writes");
    }

    #[test]
    fn overwrite_replaces_existing_file_without_double_counting() {
        let (dir, _guard) = unique_temp_path("quota-overwrite");
        let mut cache = DiskCoverCache::with_limit(&dir, 1024).expect("valid dir");
        let url = "https://example.com/same.jpg";
        cache.put(url, vec![1; 40]);
        assert_eq!(cache.total_size(), 40);
        cache.put(url, vec![2; 10]);
        assert_eq!(cache.total_size(), 10);
    }

    #[test]
    fn new_fails_when_path_blocked_by_file() {
        let (file_path, _guard) = unique_temp_path("blocked-file");
        std::fs::write(&file_path, b"not a directory").expect("write barrier file");

        let result = DiskCoverCache::new(&file_path);
        assert!(result.is_err(), "expected error when path is blocked by a file");
    }

    #[test]
    fn lifecycle_two_tier_get_put_disk_persistence_and_overwrite() {
        let (dir, _guard) = unique_temp_path("lifecycle");
        let mut cache = DiskCoverCache::new(&dir).expect("valid dir");

        let url = "https://example.com/cover1.jpg";
        assert_eq!(cache.get(url), None, "missing url returns None");

        cache.put(url, vec![1, 2, 3]);
        assert_eq!(cache.get(url), Some(vec![1, 2, 3]), "memory hit after put");

        // Verify disk file was created.
        let file_path = cache.dir().join(cache_file_name(url));
        assert!(file_path.is_file(), "cover file must exist on disk");

        // Drop in-memory cache and recreate from same directory to verify disk tier.
        drop(cache);
        let mut second = DiskCoverCache::new(&dir).expect("valid dir");
        assert_eq!(second.get(url), Some(vec![1, 2, 3]), "disk hit on clean memory instance");

        // Overwrite updates both tiers.
        second.put(url, vec![4, 5, 6]);
        assert_eq!(second.get(url), Some(vec![4, 5, 6]), "updated in memory tier");

        drop(second);
        let third = DiskCoverCache::new(&dir).expect("valid dir");
        assert_eq!(third.get(url), Some(vec![4, 5, 6]), "updated in disk tier");
    }

    #[test]
    fn put_soft_fails_to_memory_when_disk_write_fails() {
        let (dir, _guard) = unique_temp_path("soft-fail");
        let mut cache = DiskCoverCache::new(&dir).expect("valid dir");

        let url = "https://example.com/unwritable.jpg";
        let blocked_dest = cache.dir().join(cache_file_name(url));
        std::fs::create_dir_all(&blocked_dest).expect("create blocking dir");

        cache.put(url, vec![7, 8, 9]);
        assert_eq!(
            cache.get(url),
            Some(vec![7, 8, 9]),
            "memory tier caches despite disk write failure"
        );
    }
}
