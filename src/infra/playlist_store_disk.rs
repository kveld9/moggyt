//! Disk-backed local playlist persistence adapter.
//!
//! Handles loading and saving imported Spotify playlists (`playlists.json`).
//! Preserves track list, metadata, and match confidence across restarts.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::playlist_store::{PlaylistStore, sort_playlists};
use super::store_health::{HealthFlag, StoreHealth};
use crate::domain::{LocalPlaylist, is_local_playlist_id};

/// File name used inside the config directory.
pub const PLAYLISTS_FILE: &str = "playlists.json";

/// On-disk schema version.
pub const PLAYLISTS_SCHEMA_VERSION: u32 = 1;

/// Versioned JSON envelope persisted to disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistFile {
    pub version: u32,
    pub playlists: Vec<LocalPlaylist>,
}

/// Disk-backed playlist store writing `playlists.json` atomically.
#[derive(Debug, Clone)]
pub struct DiskPlaylistStore {
    config_dir: PathBuf,
    health: HealthFlag,
}

impl DiskPlaylistStore {
    /// Creates a disk store rooted in `config_dir`.
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir, health: HealthFlag::new() }
    }

    /// Full path to the `playlists.json` file.
    pub fn file_path(&self) -> PathBuf {
        self.config_dir.join(PLAYLISTS_FILE)
    }

    pub fn health(&self) -> StoreHealth {
        self.health.get()
    }

    pub fn is_quarantined(&self) -> bool {
        self.health.is_quarantined()
    }

    pub fn clear_health(&self) {
        self.health.clear();
    }

    /// Loads all playlists from disk. Returns an empty list if absent, unreadable, or corrupt.
    pub fn load_all(&self) -> Vec<LocalPlaylist> {
        let path = self.file_path();
        let Some(bytes) = self.read_bytes(&path) else {
            return Vec::new();
        };
        match parse_envelope(&bytes) {
            ParsedEnvelope::Valid(playlists) => {
                self.health.set(StoreHealth::Ok);
                playlists
            }
            ParsedEnvelope::UnsupportedVersion(version) => {
                self.health.set(StoreHealth::Incompatible);
                log_version_error(&path, version);
                Vec::new()
            }
            ParsedEnvelope::Corrupt(err) => {
                log_parse_error(&path, &err);
                self.quarantine_corrupt(&path);
                Vec::new()
            }
        }
    }

    fn read_bytes(&self, path: &Path) -> Option<Vec<u8>> {
        match read_file_optional(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                log_read_error(path, &err);
                None
            }
        }
    }

    fn load_for_write(&self) -> anyhow::Result<Vec<LocalPlaylist>> {
        let path = self.file_path();
        let Some(bytes) = read_file_optional(&path)
            .with_context(|| format!("failed to read playlist file at {}", path.display()))?
        else {
            return Ok(Vec::new());
        };
        self.parse_for_write(&bytes, &path)
    }

    fn parse_for_write(&self, bytes: &[u8], path: &Path) -> anyhow::Result<Vec<LocalPlaylist>> {
        match parse_envelope(bytes) {
            ParsedEnvelope::Valid(playlists) => {
                self.health.set(StoreHealth::Ok);
                Ok(playlists)
            }
            ParsedEnvelope::UnsupportedVersion(version) => {
                self.health.set(StoreHealth::Incompatible);
                anyhow::bail!(
                    "unsupported playlist schema version {version} in {}",
                    path.display()
                );
            }
            ParsedEnvelope::Corrupt(err) => {
                log_parse_error(path, &err);
                self.quarantine_corrupt(path);
                Ok(Vec::new())
            }
        }
    }

    fn quarantine_corrupt(&self, path: &Path) {
        self.health.set(StoreHealth::Quarantined);
        match super::store_health::quarantine(path) {
            Ok(backup_path) => log_corrupt_backup(path, &backup_path),
            Err(err) => {
                tracing::warn!(
                    "failed to quarantine corrupt playlist file at {}: {err}",
                    path.display()
                );
            }
        }
    }

    fn write_atomic(&self, data: &[u8]) -> anyhow::Result<()> {
        super::atomic_file::write_atomic(&self.file_path(), data)
    }

    fn persist_playlists(&self, playlists: &[LocalPlaylist]) -> anyhow::Result<()> {
        let envelope =
            PlaylistFile { version: PLAYLISTS_SCHEMA_VERSION, playlists: playlists.to_vec() };
        let json = serde_json::to_vec_pretty(&envelope)
            .context("failed to serialize playlist envelope")?;
        self.write_atomic(&json)
    }
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn log_read_error(path: &Path, err: &std::io::Error) {
    tracing::warn!("failed to read playlist file at {}: {err}", path.display());
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn log_parse_error(path: &Path, err: &serde_json::Error) {
    tracing::warn!("failed to parse playlist file at {}: {err}", path.display());
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn log_version_error(path: &Path, version: u32) {
    tracing::warn!("unsupported playlist schema version {version} in {}", path.display());
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn log_corrupt_backup(path: &Path, backup_path: &Path) {
    tracing::warn!(
        "corrupt playlist file at {} backed up to {}",
        path.display(),
        backup_path.display()
    );
}

enum ParsedEnvelope {
    Valid(Vec<LocalPlaylist>),
    Corrupt(serde_json::Error),
    UnsupportedVersion(u32),
}

fn read_file_optional(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn parse_envelope(bytes: &[u8]) -> ParsedEnvelope {
    let envelope = match serde_json::from_slice::<PlaylistFile>(bytes) {
        Ok(envelope) => envelope,
        Err(err) => return ParsedEnvelope::Corrupt(err),
    };
    if envelope.version > PLAYLISTS_SCHEMA_VERSION {
        return ParsedEnvelope::UnsupportedVersion(envelope.version);
    }
    ParsedEnvelope::Valid(envelope.playlists)
}

fn upsert_playlist(playlists: &mut Vec<LocalPlaylist>, playlist: &LocalPlaylist) {
    if let Some(existing) = playlists.iter_mut().find(|p| p.id == playlist.id) {
        *existing = playlist.clone();
    } else {
        playlists.push(playlist.clone());
    }
}

impl PlaylistStore for DiskPlaylistStore {
    fn list(&self) -> Vec<LocalPlaylist> {
        let mut playlists = self.load_all();
        sort_playlists(&mut playlists);
        playlists
    }

    fn get(&self, id: &str) -> Option<LocalPlaylist> {
        self.load_all().into_iter().find(|p| p.id == id)
    }

    fn save(&self, playlist: &LocalPlaylist) -> anyhow::Result<()> {
        if !is_local_playlist_id(&playlist.id) {
            anyhow::bail!("invalid local playlist id '{}'", playlist.id);
        }
        let mut playlists = self.load_for_write()?;
        upsert_playlist(&mut playlists, playlist);
        self.persist_playlists(&playlists)
    }

    fn delete(&self, id: &str) -> anyhow::Result<()> {
        let mut playlists = self.load_for_write()?;
        let initial_len = playlists.len();
        playlists.retain(|p| p.id != id);
        if playlists.len() == initial_len {
            return Ok(());
        }
        self.persist_playlists(&playlists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LocalTrack, MatchConfidence};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "moggyt-{name}-{}-{}",
                std::process::id(),
                fastrand()
            ));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }

        fn path(&self) -> PathBuf {
            self.0.clone()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fastrand() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
    }

    #[test]
    fn test_disk_store_save_load_round_trip_with_tracks_and_confidence() {
        let temp = TempDir::new("playlist-roundtrip");
        let store = DiskPlaylistStore::new(temp.path());

        let mut p1 = LocalPlaylist::new("local:1", "Playlist One").with_author("Author 1");
        let mut t1 = LocalTrack::new("v1", "Track 1", "Artist 1", MatchConfidence::High);
        t1.album = Some("Album 1".to_string());
        t1.duration_secs = Some(180);
        let t2 = LocalTrack::new("v2", "Track 2", "Artist 2", MatchConfidence::Medium);
        p1.tracks = vec![t1, t2];

        let mut p2 = LocalPlaylist::new("local:2", "Playlist Two").with_author("Author 2");
        let t3 = LocalTrack::new("v3", "Track 3", "Artist 3", MatchConfidence::Low);
        p2.tracks = vec![t3];

        store.save(&p1).expect("save p1 succeeds");
        store.save(&p2).expect("save p2 succeeds");

        let fresh_store = DiskPlaylistStore::new(temp.path());
        let loaded = fresh_store.list();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "Playlist One");
        assert_eq!(loaded[1].title, "Playlist Two");
        assert_eq!(loaded[0].tracks.len(), 2);
        assert_eq!(loaded[0].tracks[0].confidence, MatchConfidence::High);
        assert_eq!(loaded[0].tracks[1].confidence, MatchConfidence::Medium);
        assert_eq!(loaded[1].tracks[0].confidence, MatchConfidence::Low);
        assert_eq!(loaded[0].tracks[0].album.as_deref(), Some("Album 1"));
        assert_eq!(loaded[0].tracks[0].duration_secs, Some(180));
    }

    #[test]
    fn test_disk_store_save_twice_upserts_and_does_not_duplicate() {
        let temp = TempDir::new("playlist-upsert");
        let store = DiskPlaylistStore::new(temp.path());

        let p1 = LocalPlaylist::new("local:same", "Initial Title");
        let p2 = LocalPlaylist::new("local:same", "Updated Title");

        store.save(&p1).expect("first save");
        store.save(&p2).expect("second save");

        let fresh = DiskPlaylistStore::new(temp.path());
        let list = fresh.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Updated Title");
        assert_eq!(fresh.get("local:same").unwrap().title, "Updated Title");
    }

    #[test]
    fn test_disk_store_delete_removes_and_persists_empty_envelope() {
        let temp = TempDir::new("playlist-delete");
        let store = DiskPlaylistStore::new(temp.path());

        let p1 = LocalPlaylist::new("local:one", "One");
        store.save(&p1).expect("save succeeds");
        assert!(store.file_path().exists());

        // Deleting unknown id is Ok and no-op
        store.delete("local:unknown").expect("unknown delete is ok");
        assert_eq!(store.list().len(), 1);

        // Deleting the last playlist persists an empty envelope
        store.delete("local:one").expect("delete last playlist");
        assert_eq!(store.list().len(), 0);
        assert!(store.file_path().exists());

        // Verify disk content is still a valid envelope
        let bytes = std::fs::read(store.file_path()).expect("read file");
        let envelope: PlaylistFile =
            serde_json::from_slice(&bytes).expect("envelope is valid JSON");
        assert_eq!(envelope.version, PLAYLISTS_SCHEMA_VERSION);
        assert!(envelope.playlists.is_empty());
    }

    #[test]
    fn test_disk_store_missing_directory_and_file_yields_empty_list() {
        let temp = TempDir::new("playlist-missing");
        let store = DiskPlaylistStore::new(temp.path());
        assert!(store.list().is_empty());
        assert_eq!(store.get("local:p1"), None);

        // Existing empty directory without file
        std::fs::create_dir_all(temp.path()).unwrap();
        assert!(store.list().is_empty());
        assert_eq!(store.get("local:p1"), None);
    }

    #[test]
    fn test_disk_store_corrupt_read_sets_quarantine() {
        let temp = TempDir::new("playlist-corrupt-read");
        std::fs::create_dir_all(temp.path()).unwrap();
        let store = DiskPlaylistStore::new(temp.path());

        let corrupt_payload = "{not json for read";
        std::fs::write(store.file_path(), corrupt_payload).unwrap();

        assert!(store.list().is_empty());
        assert!(store.is_quarantined());
        assert!(!store.file_path().exists(), "original path must be gone");

        let entries: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().contains(".corrupt."))
            .collect();
        assert_eq!(entries.len(), 1, "exactly one .corrupt. backup must exist");
        assert_eq!(std::fs::read_to_string(&entries[0]).unwrap(), corrupt_payload);
    }

    #[test]
    fn test_disk_store_newer_schema_is_incompatible() {
        let temp = TempDir::new("playlist-newer-read");
        std::fs::create_dir_all(temp.path()).unwrap();
        let store = DiskPlaylistStore::new(temp.path());

        let future_version = PLAYLISTS_SCHEMA_VERSION + 1;
        let future_payload = format!(
            r#"{{"version": {future_version}, "playlists": [{{"id": "local:1", "title": "Future", "tracks": []}}]}}"#
        );
        std::fs::write(store.file_path(), &future_payload).unwrap();

        assert!(store.list().is_empty());
        assert_eq!(store.health(), StoreHealth::Incompatible);
        assert!(!store.is_quarantined());
        assert_eq!(std::fs::read_to_string(store.file_path()).unwrap(), future_payload);
    }

    #[test]
    fn test_disk_store_version_99_yields_empty_and_preserves_file() {
        let temp = TempDir::new("playlist-v99");
        std::fs::create_dir_all(temp.path()).unwrap();
        let store = DiskPlaylistStore::new(temp.path());

        let v99_payload =
            r#"{"version": 99, "playlists": [{"id": "local:1", "title": "Future", "tracks": []}]}"#;
        std::fs::write(store.file_path(), v99_payload).unwrap();

        assert!(store.list().is_empty());
        assert_eq!(store.get("local:1"), None);

        let content_after = std::fs::read_to_string(store.file_path()).unwrap();
        assert_eq!(content_after, v99_payload);
    }

    #[test]
    fn test_disk_store_save_rejects_non_local_id_and_preserves_file() {
        let temp = TempDir::new("playlist-reject");
        let store = DiskPlaylistStore::new(temp.path());

        let valid = LocalPlaylist::new("local:valid", "Valid Playlist");
        store.save(&valid).expect("save valid succeeds");

        let bad = LocalPlaylist::new("spotify:non_local", "Invalid Playlist");
        let err = store.save(&bad).unwrap_err();
        assert!(err.to_string().contains("spotify:non_local"), "error should contain id: {err}");

        let fresh = DiskPlaylistStore::new(temp.path());
        let list = fresh.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "local:valid");
    }

    #[test]
    fn test_disk_store_no_leftover_tmp_files_after_successful_save() {
        let temp = TempDir::new("playlist-tmp-cleanup");
        let store = DiskPlaylistStore::new(temp.path());

        let p1 = LocalPlaylist::new("local:clean", "Clean");
        store.save(&p1).expect("save succeeds");

        let entries = std::fs::read_dir(temp.path()).unwrap();
        for entry in entries {
            let entry = entry.unwrap();
            let file_name = entry.file_name().to_string_lossy().to_string();
            assert!(!file_name.contains(".tmp."), "temporary file was not cleaned up: {file_name}");
        }
    }

    #[test]
    fn test_disk_store_list_ordering_title_then_id() {
        let temp = TempDir::new("playlist-ordering");
        let store = DiskPlaylistStore::new(temp.path());

        store.save(&LocalPlaylist::new("local:3", "Zeta")).unwrap();
        store.save(&LocalPlaylist::new("local:2", "Alpha")).unwrap();
        store.save(&LocalPlaylist::new("local:1", "Alpha")).unwrap();
        store.save(&LocalPlaylist::new("local:0", "Beta")).unwrap();

        let list = store.list();
        let ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["local:1", "local:2", "local:0", "local:3"]);
    }

    #[test]
    fn test_disk_store_save_backs_up_corrupt_file_and_saves_new_playlist() {
        let temp = TempDir::new("playlist-corrupt-save");
        std::fs::create_dir_all(temp.path()).unwrap();
        let store = DiskPlaylistStore::new(temp.path());

        let corrupt_payload = "{not json";
        std::fs::write(store.file_path(), corrupt_payload).unwrap();

        let new_playlist = LocalPlaylist::new("local:new", "New Playlist");
        store.save(&new_playlist).expect("save succeeds on corrupt file by backing it up");

        let entries: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().contains(".corrupt."))
            .collect();
        assert_eq!(entries.len(), 1, "corrupt backup file must exist");
        let backup_content = std::fs::read_to_string(&entries[0]).unwrap();
        assert_eq!(backup_content, corrupt_payload);

        let list = store.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "local:new");
        assert_eq!(list[0].title, "New Playlist");
    }

    #[test]
    fn test_disk_store_save_fails_on_future_schema_version_and_preserves_file() {
        let temp = TempDir::new("playlist-future-save");
        std::fs::create_dir_all(temp.path()).unwrap();
        let store = DiskPlaylistStore::new(temp.path());

        let future_version = PLAYLISTS_SCHEMA_VERSION + 1;
        let future_payload = format!(
            r#"{{"version": {future_version}, "playlists": [{{"id": "local:existing", "title": "Existing", "tracks": []}}]}}"#
        );
        std::fs::write(store.file_path(), &future_payload).unwrap();
        let bytes_before = std::fs::read(store.file_path()).unwrap();

        let new_playlist = LocalPlaylist::new("local:new", "New Playlist");
        let result = store.save(&new_playlist);
        assert!(result.is_err(), "save must fail on future schema version");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unsupported playlist schema version"),
            "expected unsupported version error, got: {err_msg}"
        );

        let bytes_after = std::fs::read(store.file_path()).unwrap();
        assert_eq!(bytes_before, bytes_after, "file bytes must be byte-identical to before");
    }

    #[test]
    fn test_disk_store_delete_fails_on_future_schema_version_and_preserves_file() {
        let temp = TempDir::new("playlist-future-delete");
        std::fs::create_dir_all(temp.path()).unwrap();
        let store = DiskPlaylistStore::new(temp.path());

        let future_version = PLAYLISTS_SCHEMA_VERSION + 1;
        let future_payload = format!(
            r#"{{"version": {future_version}, "playlists": [{{"id": "local:x", "title": "X", "tracks": []}}]}}"#
        );
        std::fs::write(store.file_path(), &future_payload).unwrap();
        let bytes_before = std::fs::read(store.file_path()).unwrap();

        let result = store.delete("local:x");
        assert!(result.is_err(), "delete must fail on future schema version");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unsupported playlist schema version"),
            "expected unsupported version error, got: {err_msg}"
        );

        let bytes_after = std::fs::read(store.file_path()).unwrap();
        assert_eq!(bytes_before, bytes_after, "file bytes must be byte-identical to before");
    }

    #[test]
    fn test_disk_store_save_preserves_existing_corrupt_backup() {
        let temp = TempDir::new("playlist-corrupt-preserve");
        std::fs::create_dir_all(temp.path()).unwrap();
        let store = DiskPlaylistStore::new(temp.path());

        let corrupt_payload1 = "{first corrupt payload";
        std::fs::write(store.file_path(), corrupt_payload1).unwrap();
        store.save(&LocalPlaylist::new("local:new1", "New 1")).expect("first save succeeds");

        let corrupt_payload2 = "{second corrupt payload";
        std::fs::write(store.file_path(), corrupt_payload2).unwrap();
        store.save(&LocalPlaylist::new("local:new2", "New 2")).expect("second save succeeds");

        let entries: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().contains(".corrupt."))
            .collect();
        assert_eq!(entries.len(), 2, "two distinct .corrupt. files must exist");
        let contents: Vec<String> =
            entries.iter().map(|p| std::fs::read_to_string(p).unwrap()).collect();
        assert!(contents.contains(&corrupt_payload1.to_string()));
        assert!(contents.contains(&corrupt_payload2.to_string()));
    }

    #[test]
    fn test_disk_store_delete_backs_up_corrupt_file() {
        let temp = TempDir::new("playlist-corrupt-delete");
        std::fs::create_dir_all(temp.path()).unwrap();
        let store = DiskPlaylistStore::new(temp.path());

        let corrupt_payload = "{not json for delete";
        std::fs::write(store.file_path(), corrupt_payload).unwrap();

        store.delete("local:any").expect("delete handles corrupt file by backing it up");

        let entries: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|s| s.starts_with("playlists.json.corrupt."))
            })
            .collect();
        assert_eq!(entries.len(), 1, "corrupt backup file must exist after delete");
        assert_eq!(std::fs::read_to_string(&entries[0]).unwrap(), corrupt_payload);
    }

    #[test]
    fn test_disk_store_save_fails_on_read_error_other_than_not_found() {
        let temp = TempDir::new("playlist-read-err");
        std::fs::create_dir_all(temp.path()).unwrap();
        let store = DiskPlaylistStore::new(temp.path());

        // A directory at playlists.json causes std::fs::read to fail with an I/O error (EISDIR).
        std::fs::create_dir(store.file_path()).unwrap();

        let new_playlist = LocalPlaylist::new("local:new", "New Playlist");
        let result = store.save(&new_playlist);
        assert!(result.is_err(), "save must fail if file cannot be read");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains(PLAYLISTS_FILE), "error should contain file path: {err_msg}");
    }
}
