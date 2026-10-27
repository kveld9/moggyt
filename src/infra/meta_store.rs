//! Metadata override persistence adapter.
//!
//! Handles loading and saving local track metadata overrides (`metadata.json`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::domain::MetadataOverride;

/// Port for loading and persisting local track metadata overrides.
pub trait MetaStore: Send + Sync {
    /// Loads metadata overrides, returning an empty map if absent or unreadable.
    fn load(&self) -> HashMap<String, MetadataOverride>;
    /// Persists metadata overrides to storage.
    fn save(&self, map: &HashMap<String, MetadataOverride>) -> anyhow::Result<()>;
}

/// In-memory metadata override store for tests and headless execution.
#[derive(Debug, Clone, Default)]
pub struct MemoryMetaStore {
    inner: Arc<Mutex<HashMap<String, MetadataOverride>>>,
}

impl MemoryMetaStore {
    /// Creates an empty in-memory metadata override store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl MetaStore for MemoryMetaStore {
    fn load(&self) -> HashMap<String, MetadataOverride> {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn save(&self, map: &HashMap<String, MetadataOverride>) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("mutex poisoned: {e}"))?;
        *guard = map.clone();
        Ok(())
    }
}

/// Disk-backed metadata override store writing to `metadata.json`.
#[derive(Debug, Clone)]
pub struct DiskMetaStore {
    config_dir: PathBuf,
}

impl DiskMetaStore {
    /// Creates a disk store rooted in `config_dir`.
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    /// Full path to the target `metadata.json` file.
    pub fn file_path(&self) -> PathBuf {
        self.config_dir.join("metadata.json")
    }

    fn temp_file_path(&self) -> PathBuf {
        let id = std::process::id();
        self.config_dir.join(format!("metadata.json.tmp.{id}"))
    }

    fn write_atomic(&self, data: &[u8]) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        let tmp = self.temp_file_path();
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, self.file_path())?;
        Ok(())
    }
}

impl MetaStore for DiskMetaStore {
    fn load(&self) -> HashMap<String, MetadataOverride> {
        let path = self.file_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return HashMap::new();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    fn save(&self, map: &HashMap<String, MetadataOverride>) -> anyhow::Result<()> {
        let json = serde_json::to_vec_pretty(map)?;
        self.write_atomic(&json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_meta_store_round_trip() {
        let store = MemoryMetaStore::new();
        assert!(store.load().is_empty());

        let mut map = HashMap::new();
        map.insert(
            "id-1".to_string(),
            MetadataOverride::new(Some("Custom Title"), None).expect("valid override"),
        );
        store.save(&map).expect("save should succeed");

        let loaded = store.load();
        assert_eq!(loaded, map);
    }

    #[test]
    fn disk_meta_store_missing_file_yields_empty_without_panic() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-meta-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskMetaStore::new(temp_dir.clone());

        assert!(store.load().is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn disk_meta_store_corrupt_fallback_never_panics() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-meta-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let store = DiskMetaStore::new(temp_dir.clone());

        // Truncated JSON
        std::fs::write(store.file_path(), b"{\"id\": {\"title\": ").expect("write truncated");
        assert!(store.load().is_empty());

        // Binary garbage
        std::fs::write(store.file_path(), [0xFF, 0xFE, 0x00, 0x01]).expect("write binary");
        assert!(store.load().is_empty());

        // Schema mismatch
        std::fs::write(store.file_path(), b"{\"id\": 12345}").expect("write schema mismatch");
        assert!(store.load().is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn disk_meta_store_round_trip_with_atomic_write() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-meta-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskMetaStore::new(temp_dir.clone());

        let mut map = HashMap::new();
        map.insert(
            "id-42".to_string(),
            MetadataOverride::new(Some("Custom Title"), Some("Custom Artist"))
                .expect("valid override"),
        );

        store.save(&map).expect("save should succeed");
        assert!(store.file_path().exists());

        let reloaded = store.load();
        assert_eq!(reloaded, map);

        let second_store = DiskMetaStore::new(temp_dir.clone());
        assert_eq!(second_store.load(), map);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
