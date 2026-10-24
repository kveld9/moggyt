//! Two-tier cover cache (bounded memory + persistent disk).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// Two-tier cover cache: bounded in-memory cache plus unbounded on-disk cache.
///
/// Disk persistence policy: disk entries are never evicted in v1.
/// Hot entries are held in bounded memory (capacity 64), while disk cache
/// acts as persistent local backing. Full disk eviction policies are deferred.
pub struct DiskCoverCache {
    memory: MemoryCoverCache,
    dir: PathBuf,
}

impl DiskCoverCache {
    /// Initializes a disk cover cache at `dir`, creating directories if needed.
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { memory: MemoryCoverCache::new(), dir })
    }

    /// Path to the directory backing the cache.
    pub fn dir(&self) -> &Path {
        &self.dir
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
        std::fs::read(path).ok()
    }

    fn put(&mut self, url: &str, bytes: Vec<u8>) {
        self.memory.put(url, bytes.clone());
        if let Err(err) = self.write_to_disk(url, &bytes) {
            tracing::warn!(url = %url, error = %err, "failed to persist cover art to disk");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

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
