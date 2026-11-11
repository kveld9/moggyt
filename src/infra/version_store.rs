//! Disk-backed and in-memory persistence for audio version overrides.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::domain::VersionOverrides;

/// File name used inside the config directory.
pub const VERSION_OVERRIDES_FILE: &str = "version_overrides.json";

/// On-disk schema version.
pub const VERSION_OVERRIDES_SCHEMA_VERSION: u32 = 1;

const fn default_version() -> u32 {
    VERSION_OVERRIDES_SCHEMA_VERSION
}

/// Versioned JSON envelope persisted to disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionOverrideFile {
    #[serde(default = "default_version")]
    pub version: u32,
    pub overrides: VersionOverrides,
}

/// Port for loading and persisting audio version overrides.
pub trait VersionOverrideStore: Send + Sync {
    /// Loads stored overrides, returning empty on absent or unreadable files.
    fn load(&self) -> VersionOverrides;
    /// Persists overrides atomically.
    fn save(&self, overrides: &VersionOverrides) -> anyhow::Result<()>;
}

/// Disk-backed version override store writing `version_overrides.json` atomically.
#[derive(Debug, Clone)]
pub struct DiskVersionOverrideStore {
    config_dir: PathBuf,
}

impl DiskVersionOverrideStore {
    /// Creates a disk store rooted in `config_dir`.
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    fn path(&self) -> PathBuf {
        self.config_dir.join(VERSION_OVERRIDES_FILE)
    }

    fn read_bytes(&self, path: &Path) -> Option<Vec<u8>> {
        match read_file_optional(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!("failed to read version overrides at {}: {err}", path.display());
                None
            }
        }
    }
}

fn read_file_optional(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

impl VersionOverrideStore for DiskVersionOverrideStore {
    fn load(&self) -> VersionOverrides {
        let path = self.path();
        let Some(bytes) = self.read_bytes(&path) else {
            return VersionOverrides::default();
        };
        match serde_json::from_slice::<VersionOverrideFile>(&bytes) {
            Ok(file) => file.overrides,
            Err(err) => {
                tracing::warn!("failed to parse version overrides at {}: {err}", path.display());
                VersionOverrides::default()
            }
        }
    }

    fn save(&self, overrides: &VersionOverrides) -> anyhow::Result<()> {
        let envelope = VersionOverrideFile {
            version: VERSION_OVERRIDES_SCHEMA_VERSION,
            overrides: overrides.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&envelope)?;
        super::atomic_file::write_atomic(&self.path(), &bytes)?;
        Ok(())
    }
}

/// In-memory version override store for tests and headless execution.
#[derive(Debug, Clone, Default)]
pub struct MemoryVersionOverrideStore {
    inner: Arc<Mutex<VersionOverrides>>,
}

impl MemoryVersionOverrideStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes a snapshot of the current stored overrides.
    pub fn snapshot(&self) -> VersionOverrides {
        self.inner.lock().map(|guard| guard.clone()).unwrap_or_default()
    }
}

impl VersionOverrideStore for MemoryVersionOverrideStore {
    fn load(&self) -> VersionOverrides {
        self.snapshot()
    }

    fn save(&self, overrides: &VersionOverrides) -> anyhow::Result<()> {
        let mut guard =
            self.inner.lock().map_err(|err| anyhow::anyhow!("mutex poisoned: {err}"))?;
        *guard = overrides.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let id = std::process::id();
            let count = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("moggyt-{name}-{id}-{count}"));
            let _ = std::fs::remove_dir_all(&path);
            let _ = std::fs::create_dir_all(&path);
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_disk_round_trip() {
        let temp = TempDir::new("version-store-roundtrip");
        let store = DiskVersionOverrideStore::new(temp.path().to_path_buf());

        assert!(store.load().is_empty());

        let mut overrides = VersionOverrides::new();
        overrides.set("original_vid", "alt_vid").expect("valid override");
        store.save(&overrides).expect("save succeeds");

        assert!(temp.path().join(VERSION_OVERRIDES_FILE).exists());
        let loaded = store.load();
        assert_eq!(loaded, overrides);
        assert_eq!(loaded.resolve("original_vid"), "alt_vid");
    }

    #[test]
    fn test_corrupt_file_fallback_to_empty() {
        let temp = TempDir::new("version-store-corrupt");
        let file_path = temp.path().join(VERSION_OVERRIDES_FILE);
        std::fs::write(&file_path, b"{ not valid json }").expect("write corrupt file");

        let store = DiskVersionOverrideStore::new(temp.path().to_path_buf());
        let loaded = store.load();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_memory_store_save_load() {
        let store = MemoryVersionOverrideStore::new();
        assert!(store.load().is_empty());

        let mut overrides = VersionOverrides::new();
        overrides.set("orig", "repl").expect("valid override");
        store.save(&overrides).expect("save succeeds");

        assert_eq!(store.load(), overrides);
        assert_eq!(store.snapshot(), overrides);
    }
}
