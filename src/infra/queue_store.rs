//! Playback queue persistence adapter.
//!
//! Handles loading and saving the playback queue (`queue.json`).
//! Preserves track list, cursor index, and repeat mode across restarts (SDD FR-13).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::domain::PlaybackQueue;

/// Port for loading and persisting the playback queue.
pub trait QueueStore: Send + Sync {
    /// Loads the saved playback queue, returning an empty queue if absent or unreadable.
    fn load(&self) -> PlaybackQueue;
    /// Persists the playback queue to storage.
    fn save(&self, queue: &PlaybackQueue) -> anyhow::Result<()>;
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
}

impl DiskQueueStore {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    pub fn file_path(&self) -> PathBuf {
        self.config_dir.join("queue.json")
    }

    fn temp_file_path(&self) -> PathBuf {
        let id = std::process::id();
        self.config_dir.join(format!("queue.json.tmp.{id}"))
    }

    fn write_atomic(&self, data: &[u8]) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        let tmp = self.temp_file_path();
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, self.file_path())?;
        Ok(())
    }
}

impl QueueStore for DiskQueueStore {
    fn load(&self) -> PlaybackQueue {
        let path = self.file_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return PlaybackQueue::new();
        };
        let mut queue: PlaybackQueue =
            serde_json::from_slice(&bytes).unwrap_or_else(|_| PlaybackQueue::new());
        queue.clamp_cursor();
        queue
    }

    fn save(&self, queue: &PlaybackQueue) -> anyhow::Result<()> {
        let json = serde_json::to_vec_pretty(queue)?;
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
        std::fs::write(store.file_path(), b"{\"tracks\": [{\"id\": ").unwrap();
        assert_eq!(store.load(), PlaybackQueue::new());

        // 2. Corrupt binary garbage
        std::fs::write(store.file_path(), [0xFF, 0xFE, 0x00, 0x01]).unwrap();
        assert_eq!(store.load(), PlaybackQueue::new());

        // 3. Schema mismatch
        std::fs::write(store.file_path(), b"{\"tracks\": 99999}").unwrap();
        assert_eq!(store.load(), PlaybackQueue::new());

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
