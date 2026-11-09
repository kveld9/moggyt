//! Playback queue persistence adapter.
//!
//! Handles loading and saving the playback queue (`queue.json`).
//! Preserves track list, cursor index, and repeat mode across restarts (SDD FR-13).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::store_health::{HealthFlag, StoreHealth, quarantine};
use crate::domain::PlaybackQueue;

/// Current schema version of persisted playback queue.
pub const QUEUE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueueFile {
    version: u32,
    queue: PlaybackQueue,
}

/// Port for loading and persisting the playback queue.
pub trait QueueStore: Send + Sync {
    /// Loads the saved playback queue, returning an empty queue if absent or unreadable.
    fn load(&self) -> PlaybackQueue;
    /// Persists the playback queue to storage.
    fn save(&self, queue: &PlaybackQueue) -> anyhow::Result<()>;
    /// Reports the health of the store after the last load.
    fn health(&self) -> StoreHealth {
        StoreHealth::Ok
    }
    /// Returns true if the store payload was corrupt and quarantined.
    fn is_quarantined(&self) -> bool {
        self.health() == StoreHealth::Quarantined
    }
}

/// In-memory queue store for tests and headless execution.
#[derive(Debug, Clone)]
pub struct MemoryQueueStore {
    inner: Arc<Mutex<PlaybackQueue>>,
}

impl Default for MemoryQueueStore {
    fn default() -> Self {
        Self { inner: Arc::new(Mutex::new(PlaybackQueue::new())) }
    }
}

impl MemoryQueueStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl QueueStore for MemoryQueueStore {
    fn load(&self) -> PlaybackQueue {
        let mut queue =
            self.inner.lock().map(|g| g.clone()).unwrap_or_else(|_| PlaybackQueue::new());
        queue.clamp_cursor();
        queue
    }

    fn save(&self, queue: &PlaybackQueue) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("mutex poisoned: {e}"))?;
        *guard = queue.clone();
        Ok(())
    }
}

/// Disk-backed queue store writing to `queue.json`.
#[derive(Debug, Clone)]
pub struct DiskQueueStore {
    config_dir: PathBuf,
    health: HealthFlag,
}

impl DiskQueueStore {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir, health: HealthFlag::new() }
    }

    pub fn file_path(&self) -> PathBuf {
        self.config_dir.join("queue.json")
    }

    pub fn clear_health(&self) {
        self.health.clear();
    }

    fn write_atomic(&self, data: &[u8]) -> anyhow::Result<()> {
        super::atomic_file::write_atomic(&self.file_path(), data)
    }

    fn quarantine_corrupt(&self, path: &Path) {
        self.health.set(StoreHealth::Quarantined);
        tracing::warn!("corrupt queue file at {}", path.display());
        match quarantine(path) {
            Ok(backup) => {
                tracing::warn!(
                    "corrupt queue file at {} quarantined to {}",
                    path.display(),
                    backup.display()
                );
            }
            Err(err) => {
                tracing::warn!("failed to quarantine queue file at {}: {err}", path.display());
            }
        }
    }
}

enum ParsedQueue {
    Valid(PlaybackQueue),
    Incompatible,
    Corrupt,
}

fn parse_queue(bytes: &[u8]) -> ParsedQueue {
    if let Ok(envelope) = serde_json::from_slice::<QueueFile>(bytes) {
        if envelope.version > QUEUE_SCHEMA_VERSION {
            return ParsedQueue::Incompatible;
        }
        return ParsedQueue::Valid(envelope.queue);
    }
    if let Ok(queue) = serde_json::from_slice::<PlaybackQueue>(bytes) {
        return ParsedQueue::Valid(queue);
    }
    ParsedQueue::Corrupt
}

impl QueueStore for DiskQueueStore {
    fn health(&self) -> StoreHealth {
        self.health.get()
    }

    fn is_quarantined(&self) -> bool {
        self.health.is_quarantined()
    }

    fn load(&self) -> PlaybackQueue {
        let path = self.file_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return PlaybackQueue::new();
        };
        match parse_queue(&bytes) {
            ParsedQueue::Valid(mut queue) => {
                self.health.set(StoreHealth::Ok);
                queue.clamp_cursor();
                queue
            }
            ParsedQueue::Incompatible => {
                self.health.set(StoreHealth::Incompatible);
                tracing::warn!("unsupported queue schema version in {}", path.display());
                PlaybackQueue::new()
            }
            ParsedQueue::Corrupt => {
                self.quarantine_corrupt(&path);
                PlaybackQueue::new()
            }
        }
    }

    fn save(&self, queue: &PlaybackQueue) -> anyhow::Result<()> {
        let envelope = QueueFile { version: QUEUE_SCHEMA_VERSION, queue: queue.clone() };
        let json = serde_json::to_vec_pretty(&envelope)?;
        self.write_atomic(&json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{RepeatMode, Track};

    #[test]
    fn test_memory_queue_store_save_load_round_trip() {
        let store = MemoryQueueStore::new();
        assert_eq!(store.load(), PlaybackQueue::new());

        let mut queue = PlaybackQueue::new();
        queue.push(Track::new("id-1", "Song A", "Artist X"));
        queue.push(Track::new("id-2", "Song B", "Artist Y"));
        queue.advance();
        queue.set_repeat(RepeatMode::All);

        store.save(&queue).expect("save succeeds");
        let loaded = store.load();
        assert_eq!(loaded, queue);
        assert_eq!(loaded.cursor(), 1);
        assert_eq!(loaded.repeat(), RepeatMode::All);
    }

    #[test]
    fn test_disk_queue_store_missing_file_returns_empty_queue() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-queue-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskQueueStore::new(temp_dir.clone());

        let loaded = store.load();
        assert_eq!(loaded, PlaybackQueue::new());
        assert!(loaded.is_empty());
        assert_eq!(loaded.cursor(), 0);
        assert_eq!(loaded.repeat(), RepeatMode::Off);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_disk_queue_store_corrupt_file_returns_empty_queue_without_panic() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-queue-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store = DiskQueueStore::new(temp_dir.clone());

        // 1. Partial/truncated JSON
        let corrupt1 = b"{\"tracks\": [{\"id\": ";
        std::fs::write(store.file_path(), corrupt1).unwrap();
        assert_eq!(store.load(), PlaybackQueue::new());
        assert!(store.is_quarantined());
        let corrupt_files: Vec<_> = std::fs::read_dir(&temp_dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().contains(".corrupt."))
            .collect();
        assert_eq!(corrupt_files.len(), 1);
        assert_eq!(std::fs::read(&corrupt_files[0]).unwrap(), corrupt1);

        // 2. Corrupt binary garbage
        std::fs::write(store.file_path(), [0xFF, 0xFE, 0x00, 0x01]).unwrap();
        assert_eq!(store.load(), PlaybackQueue::new());
        assert!(store.is_quarantined());

        // 3. Schema mismatch
        std::fs::write(store.file_path(), b"{\"tracks\": 99999}").unwrap();
        assert_eq!(store.load(), PlaybackQueue::new());
        assert!(store.is_quarantined());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn legacy_bare_queue_file_still_loads() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-queue-legacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store = DiskQueueStore::new(temp_dir.clone());

        let json =
            r#"{"tracks":[{"id":"t1","title":"T1","artist":"A1"}],"cursor":42,"repeat":"Off"}"#;
        std::fs::write(store.file_path(), json).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.cursor(), 0);
        assert_eq!(store.health(), StoreHealth::Ok);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn newer_queue_schema_is_incompatible_not_empty() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-queue-newer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store = DiskQueueStore::new(temp_dir.clone());

        let payload = br#"{"version": 2, "queue": {"tracks": [], "cursor": 0, "repeat": "Off"}}"#;
        std::fs::write(store.file_path(), payload).unwrap();

        let loaded = store.load();
        assert_eq!(loaded, PlaybackQueue::new());
        assert_eq!(store.health(), StoreHealth::Incompatible);
        assert_eq!(std::fs::read(store.file_path()).unwrap(), payload);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn save_writes_queue_envelope() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-queue-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskQueueStore::new(temp_dir.clone());

        let mut queue = PlaybackQueue::new();
        queue.push(Track::new("id-1", "Song A", "Artist X"));
        store.save(&queue).expect("save succeeds");

        let bytes = std::fs::read(store.file_path()).expect("read file");
        let val: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
        assert_eq!(val["version"], QUEUE_SCHEMA_VERSION);
        assert!(val["queue"]["tracks"].is_array());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_disk_queue_store_save_load_round_trip_with_atomic_write() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-queue-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskQueueStore::new(temp_dir.clone());

        let mut queue = PlaybackQueue::new();
        queue.push(Track::new("id-1", "Song A", "Artist X"));
        queue.push(Track::new("id-2", "Song B", "Artist Y"));
        queue.set_repeat(RepeatMode::One);

        store.save(&queue).expect("save succeeds");

        let loaded = store.load();
        assert_eq!(loaded, queue);
        assert!(store.file_path().exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_disk_queue_store_clamps_out_of_bounds_cursor() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-queue-clamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store = DiskQueueStore::new(temp_dir.clone());

        let json =
            r#"{"tracks":[{"id":"t1","title":"T1","artist":"A1"}],"cursor":42,"repeat":"Off"}"#;
        std::fs::write(store.file_path(), json).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.cursor(), 0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
