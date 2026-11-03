//! Disk-backed audio stream cache with configurable quota and LRU eviction.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

static TMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Computes the cache filename for an audio stream track ID.
pub fn stream_file_name(track_id: &str) -> String {
    format!("{}.stream", super::cover_disk::fnv1a_hex(track_id))
}

#[derive(Debug)]
struct CacheEntry {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

/// Disk-backed audio stream cache enforcing a byte quota with LRU eviction.
#[derive(Debug, Clone)]
pub struct StreamDiskCache {
    dir: PathBuf,
    max_bytes: u64,
}

impl StreamDiskCache {
    /// Ensures directory exists and initializes the cache with a byte quota.
    pub fn new(dir: impl Into<PathBuf>, max_bytes: u64) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir, max_bytes })
    }

    /// Backing cache directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Configured maximum byte budget.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Returns the cached stream file path if present and valid, touching mtime for LRU freshness.
    pub fn get(&self, track_id: &str) -> Option<PathBuf> {
        if self.max_bytes == 0 {
            return None;
        }
        let path = self.dir.join(stream_file_name(track_id));
        if !path.is_file() {
            return None;
        }
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
        }
        Some(path)
    }

    fn write_atomic(&self, dest: &Path, data: &[u8]) -> std::io::Result<()> {
        let file_name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("stream");
        let count = TMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = self.dir.join(format!("{file_name}.tmp.{}.{count}", std::process::id()));

        if let Err(err) = std::fs::write(&tmp, data) {
            let _ = std::fs::remove_file(&tmp);
            return Err(err);
        }

        if let Err(err) = std::fs::rename(&tmp, dest) {
            let _ = std::fs::remove_file(&tmp);
            return Err(err);
        }

        Ok(())
    }

    /// Atomically writes stream data to disk, prunes expired entries, and returns the cached path.
    pub fn put(&self, track_id: &str, data: &[u8]) -> std::io::Result<PathBuf> {
        let file_name = stream_file_name(track_id);
        let dest = self.dir.join(&file_name);
        if self.max_bytes == 0 {
            self.prune_lru()?;
            return Ok(dest);
        }

        self.write_atomic(&dest, data)?;
        self.prune_lru()?;
        Ok(dest)
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
            if path.is_file() && path.extension().is_some_and(|ext| ext == "stream") {
                let metadata = entry.metadata()?;
                let size = metadata.len();
                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                total_size = total_size.saturating_add(size);
                entries.push(CacheEntry { path, size, modified });
            }
        }

        Ok((entries, total_size))
    }

    /// Calculates total size of `.stream` files; if size exceeds quota, evicts oldest modified files.
    pub fn prune_lru(&self) -> std::io::Result<()> {
        let (mut entries, mut total_size) = self.collect_entries()?;
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

        Ok(())
    }

    /// Total bytes currently stored across all `.stream` cache files.
    pub fn total_size(&self) -> u64 {
        let Ok(read_dir) = std::fs::read_dir(&self.dir) else {
            return 0;
        };
        let mut total: u64 = 0;
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "stream") {
                if let Ok(meta) = entry.metadata() {
                    total = total.saturating_add(meta.len());
                }
            }
        }
        total
    }

    /// Deletes all `.stream` files in the cache directory.
    pub fn clear(&self) -> std::io::Result<()> {
        let read_dir = match std::fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err),
        };
        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "stream") {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct TempDirGuard(PathBuf);

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn unique_temp_dir(prefix: &str) -> (TempDirGuard, PathBuf) {
        let id = TMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("moggyt-{prefix}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("failed to create temp test dir");
        (TempDirGuard(path.clone()), path)
    }

    #[test]
    fn put_and_get_roundtrip_preserves_content_and_size() {
        let (_guard, dir) = unique_temp_dir("stream-roundtrip");
        let cache = StreamDiskCache::new(&dir, 1024 * 1024).expect("cache init");
        assert_eq!(cache.max_bytes(), 1024 * 1024);
        assert_eq!(cache.total_size(), 0);

        let data = b"audio stream payload test 1234567890";
        let path = cache.put("track-1", data).expect("put track-1");
        assert!(path.is_file());

        let retrieved = cache.get("track-1").expect("get track-1");
        assert_eq!(retrieved, path);
        let read_bytes = std::fs::read(&retrieved).expect("read file");
        assert_eq!(read_bytes, data);
        assert_eq!(cache.total_size(), data.len() as u64);
    }

    #[test]
    fn lru_eviction_removes_oldest_accessed_entries() {
        let (_guard, dir) = unique_temp_dir("stream-lru");
        let cache = StreamDiskCache::new(&dir, 100).expect("cache init");

        cache.put("track-a", &[1; 40]).expect("put track-a");
        cache.put("track-b", &[2; 40]).expect("put track-b");
        assert_eq!(cache.total_size(), 80);

        let path_a = cache.get("track-a").expect("get track-a");
        let path_b = cache.get("track-b").expect("get track-b");

        // Set track-b to be older than track-a
        let older = SystemTime::now() - Duration::from_secs(60);
        let file_b = std::fs::OpenOptions::new().write(true).open(&path_b).expect("open b");
        file_b.set_modified(older).expect("set modified b");

        let newer = SystemTime::now();
        let file_a = std::fs::OpenOptions::new().write(true).open(&path_a).expect("open a");
        file_a.set_modified(newer).expect("set modified a");

        // Now put track-c (40 bytes), total would be 120 > 100 quota
        cache.put("track-c", &[3; 40]).expect("put track-c");

        // track-b should have been evicted (oldest mtime)
        assert!(cache.get("track-b").is_none());
        assert!(!path_b.is_file());

        // track-a and track-c should remain
        assert!(cache.get("track-a").is_some());
        assert!(cache.get("track-c").is_some());
        assert_eq!(cache.total_size(), 80);
    }

    #[test]
    fn zero_quota_disables_caching() {
        let (_guard, dir) = unique_temp_dir("stream-zero");
        let cache = StreamDiskCache::new(&dir, 0).expect("cache init");
        assert_eq!(cache.max_bytes(), 0);

        let path = cache.put("track-zero", &[42; 64]).expect("put with zero quota");
        assert!(!path.is_file());
        assert!(cache.get("track-zero").is_none());
        assert_eq!(cache.total_size(), 0);
    }

    #[test]
    fn clear_purges_all_stream_files() {
        let (_guard, dir) = unique_temp_dir("stream-clear");
        let cache = StreamDiskCache::new(&dir, 1024).expect("cache init");

        cache.put("track-x", &[1; 30]).expect("put x");
        cache.put("track-y", &[2; 40]).expect("put y");
        assert_eq!(cache.total_size(), 70);

        cache.clear().expect("clear");
        assert_eq!(cache.total_size(), 0);
        assert!(cache.get("track-x").is_none());
        assert!(cache.get("track-y").is_none());
    }

    #[test]
    fn write_atomic_cleans_up_on_invalid_destination() {
        let (_guard, dir) = unique_temp_dir("stream-atomic");
        let cache = StreamDiskCache::new(&dir, 1024).expect("cache init");

        let dest = dir.join("collision.stream");
        std::fs::create_dir_all(&dest).expect("create directory at dest path");

        // Attempting to atomic-write over a directory will fail during rename
        let result = cache.write_atomic(&dest, b"hello");
        assert!(result.is_err());

        // Verify no leftover .tmp files exist in directory
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .expect("read_dir")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(entries.is_empty(), "temporary files must be removed on error");
    }
}
