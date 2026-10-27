//! Non-music segment skip ranges storage adapter.
//!
//! Provides persistence for per-track skip ranges (`skips.json`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::domain::SkipRanges;

/// Storage port for loading and persisting skip ranges keyed by video id.
pub trait SkipStore: Send + Sync {
    /// Loads persisted skip ranges mapping video id to segments.
    fn load(&self) -> HashMap<String, SkipRanges>;
    /// Persists skip ranges to storage.
    fn save(&self, map: &HashMap<String, SkipRanges>) -> anyhow::Result<()>;
}

/// In-memory fake skip store for headless execution and unit tests.
#[derive(Debug, Clone, Default)]
pub struct MemorySkipStore {
    inner: Arc<Mutex<HashMap<String, SkipRanges>>>,
}

impl MemorySkipStore {
    /// Creates a new empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl SkipStore for MemorySkipStore {
    fn load(&self) -> HashMap<String, SkipRanges> {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn save(&self, map: &HashMap<String, SkipRanges>) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("mutex poisoned: {e}"))?;
        *guard = map.clone();
        Ok(())
    }
}

/// Disk-backed skip store persisting to `skips.json` in the configuration directory.
#[derive(Debug, Clone)]
pub struct DiskSkipStore {
    config_dir: PathBuf,
}

impl DiskSkipStore {
    /// Creates a disk store rooted in `config_dir`.
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    /// Full path to the `skips.json` file.
    pub fn file_path(&self) -> PathBuf {
        self.config_dir.join("skips.json")
    }

    fn temp_file_path(&self) -> PathBuf {
        let id = std::process::id();
        self.config_dir.join(format!("skips.json.tmp.{id}"))
    }

    fn write_atomic(&self, data: &[u8]) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        let tmp = self.temp_file_path();
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, self.file_path())?;
        Ok(())
    }
}

impl SkipStore for DiskSkipStore {
    fn load(&self) -> HashMap<String, SkipRanges> {
        let Ok(bytes) = std::fs::read(self.file_path()) else {
            return HashMap::new();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    fn save(&self, map: &HashMap<String, SkipRanges>) -> anyhow::Result<()> {
        let json = serde_json::to_vec_pretty(map)?;
        self.write_atomic(&json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SkipSegment;

    #[test]
    fn test_memory_store_round_trip() {
        let store = MemorySkipStore::new();
        assert!(store.load().is_empty());

        let mut ranges = SkipRanges::new();
        ranges.insert(SkipSegment::new(10, 25).unwrap());
        let mut map = HashMap::new();
        map.insert("video-123".to_string(), ranges);

        store.save(&map).expect("save succeeds");
        let loaded = store.load();
        assert_eq!(loaded, map);
    }

    #[test]
    fn test_disk_store_missing_file_returns_empty_map() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-skips-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskSkipStore::new(temp_dir.clone());

        assert_eq!(store.load(), HashMap::new());
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_disk_store_corrupt_fallback_without_panic() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-skips-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store = DiskSkipStore::new(temp_dir.clone());

        // 1. Truncated JSON
        std::fs::write(store.file_path(), b"{\"video-1\": [{\"start_secs\": ").unwrap();
        assert_eq!(store.load(), HashMap::new());

        // 2. Binary garbage
        std::fs::write(store.file_path(), [0xFF, 0xFE, 0x00, 0x01]).unwrap();
        assert_eq!(store.load(), HashMap::new());

        // 3. Schema mismatch
        std::fs::write(store.file_path(), b"{\"video-1\": 12345}").unwrap();
        assert_eq!(store.load(), HashMap::new());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_disk_store_save_load_round_trip_atomic() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-skips-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskSkipStore::new(temp_dir.clone());

        let mut ranges = SkipRanges::new();
        ranges.insert(SkipSegment::new(0, 15).unwrap());
        ranges.insert(SkipSegment::new(120, 140).unwrap());

        let mut map = HashMap::new();
        map.insert("video-abc".to_string(), ranges);

        store.save(&map).expect("save to disk succeeds");
        assert!(store.file_path().exists());

        let loaded = store.load();
        assert_eq!(loaded, map);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
