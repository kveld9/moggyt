//! Local play log storage adapter for listening history.
//!
//! Provides persistence for playback records in JSON Lines format (`play-log.jsonl`).

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::domain::PlayRecord;

/// Standard file name for the append-only playback log in JSON Lines format.
pub const PLAY_LOG_FILE_NAME: &str = "play-log.jsonl";

/// Storage port for loading and appending play log records.
pub trait PlayLogStore: Send + Sync {
    /// Loads all recorded play entries from storage in chronological order.
    fn load(&self) -> anyhow::Result<Vec<PlayRecord>>;

    /// Appends a new play record to storage.
    fn append(&self, record: &PlayRecord) -> anyhow::Result<()>;
}

/// In-memory fake play log store for testing and headless execution.
#[derive(Debug, Clone, Default)]
pub struct MemoryPlayLogStore {
    inner: Arc<Mutex<Vec<PlayRecord>>>,
}

impl MemoryPlayLogStore {
    /// Creates a new empty in-memory play log store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl PlayLogStore for MemoryPlayLogStore {
    fn load(&self) -> anyhow::Result<Vec<PlayRecord>> {
        let guard = self.inner.lock().map_err(|e| anyhow::anyhow!("mutex poisoned: {e}"))?;
        Ok(guard.clone())
    }

    fn append(&self, record: &PlayRecord) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("mutex poisoned: {e}"))?;
        guard.push(record.clone());
        Ok(())
    }
}

/// Disk-backed play log store appending records to `play-log.jsonl` in the data directory.
#[derive(Debug, Clone)]
pub struct DiskPlayLogStore {
    dir: PathBuf,
}

impl DiskPlayLogStore {
    /// Creates a disk store rooted in `dir`.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Full path to the `play-log.jsonl` file.
    pub fn file_path(&self) -> PathBuf {
        self.dir.join(PLAY_LOG_FILE_NAME)
    }
}

// NOTE(clippy): tracing macro expansion inflates cognitive complexity.
#[allow(clippy::cognitive_complexity)]
fn parse_log_line(line_number: usize, raw_line: &str) -> Option<PlayRecord> {
    let trimmed = raw_line.trim();
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str::<PlayRecord>(trimmed) {
        Ok(record) => Some(record),
        Err(_) => {
            tracing::warn!(line = line_number, "skipping malformed play log entry");
            None
        }
    }
}

impl PlayLogStore for DiskPlayLogStore {
    fn load(&self) -> anyhow::Result<Vec<PlayRecord>> {
        let file = match std::fs::File::open(self.file_path()) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };

        let reader = BufReader::new(file);
        let mut records = Vec::new();
        for (index, line) in reader.lines().enumerate() {
            if let Some(record) = parse_log_line(index + 1, &line?) {
                records.push(record);
            }
        }
        Ok(records)
    }

    fn append(&self, record: &PlayRecord) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let mut file = OpenOptions::new().create(true).append(true).open(self.file_path())?;
        let line = format!("{}\n", serde_json::to_string(record)?);
        file.write_all(line.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(id: &str, title: &str, listened_secs: u64) -> PlayRecord {
        PlayRecord {
            video_id: id.to_string(),
            title: title.to_string(),
            artist: "Artist".to_string(),
            album: Some("Album".to_string()),
            started_at_unix: 1_700_000_000,
            listened_secs,
            duration_secs: Some(200),
        }
    }

    #[test]
    fn test_memory_store_round_trip() {
        let store = MemoryPlayLogStore::new();
        assert_eq!(store.load().expect("load succeeds").len(), 0);

        let rec1 = sample_record("v1", "Song 1", 50);
        let rec2 = sample_record("v2", "Song 2", 120);

        store.append(&rec1).expect("append succeeds");
        store.append(&rec2).expect("append succeeds");

        let loaded = store.load().expect("load succeeds");
        assert_eq!(loaded, vec![rec1, rec2]);
    }

    #[test]
    fn test_disk_store_missing_file_returns_empty() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-playlog-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskPlayLogStore::new(temp_dir.clone());

        let loaded = store.load().expect("missing file returns empty list");
        assert!(loaded.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_disk_store_append_twice_and_load_returns_both() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-playlog-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskPlayLogStore::new(temp_dir.clone());

        let rec1 = sample_record("v1", "Track 1", 45);
        let rec2 = sample_record("v2", "Track 2", 90);

        store.append(&rec1).expect("append first");
        store.append(&rec2).expect("append second");

        assert!(store.file_path().exists());

        let loaded = store.load().expect("load succeeds");
        assert_eq!(loaded, vec![rec1, rec2]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_disk_store_skips_malformed_and_blank_lines() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-playlog-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let store = DiskPlayLogStore::new(temp_dir.clone());

        let rec1 = sample_record("v1", "Track 1", 60);
        let rec2 = sample_record("v2", "Track 2", 120);

        let json1 = serde_json::to_string(&rec1).unwrap();
        let json2 = serde_json::to_string(&rec2).unwrap();

        let content = format!("{json1}\n\n   \n{{not valid json\n{json2}\n");
        std::fs::write(store.file_path(), content).expect("write test file");

        let loaded = store.load().expect("load skips malformed line");
        assert_eq!(loaded, vec![rec1, rec2]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
