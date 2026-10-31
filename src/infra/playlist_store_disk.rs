//! Disk-backed local playlist persistence adapter.
//!
//! Handles loading and saving imported Spotify playlists (`playlists.json`).
//! Preserves track list, metadata, and match confidence across restarts.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::playlist_store::{PlaylistStore, sort_playlists};
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
}

impl DiskPlaylistStore {
    /// Creates a disk store rooted in `config_dir`.
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    /// Full path to the `playlists.json` file.
    pub fn file_path(&self) -> PathBuf {
        self.config_dir.join(PLAYLISTS_FILE)
    }

    /// Loads all playlists from disk. Returns an empty list if absent, unreadable, or corrupt.
    pub fn load_all(&self) -> Vec<LocalPlaylist> {
        let path = self.file_path();
        let Ok(bytes) = read_file_bytes(&path) else {
            return Vec::new();
        };
        parse_playlist_bytes(&bytes, &path)
    }

    fn temp_file_path(&self) -> PathBuf {
        let id = std::process::id();
        self.config_dir.join(format!("{PLAYLISTS_FILE}.tmp.{id}"))
    }

    fn write_atomic(&self, data: &[u8]) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.config_dir).with_context(|| {
            format!("failed to create config dir {}", self.config_dir.display())
        })?;
        let tmp = self.temp_file_path();
        std::fs::write(&tmp, data)
            .with_context(|| format!("failed to write temporary file {}", tmp.display()))?;
        std::fs::rename(&tmp, self.file_path()).with_context(|| {
            format!("failed to rename {} to {}", tmp.display(), self.file_path().display())
        })?;
        Ok(())
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

fn read_file_bytes(path: &Path) -> Result<Vec<u8>, ()> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(()),
        Err(err) => {
            log_read_error(path, &err);
            Err(())
        }
    }
}

fn parse_envelope(bytes: &[u8], path: &Path) -> Result<PlaylistFile, ()> {
    match serde_json::from_slice::<PlaylistFile>(bytes) {
        Ok(envelope) => Ok(envelope),
        Err(err) => {
            log_parse_error(path, &err);
            Err(())
        }
    }
}

fn parse_playlist_bytes(bytes: &[u8], path: &Path) -> Vec<LocalPlaylist> {
    let Ok(envelope) = parse_envelope(bytes, path) else {
        return Vec::new();
    };
    if envelope.version > PLAYLISTS_SCHEMA_VERSION {
        log_version_error(path, envelope.version);
        return Vec::new();
    }
    envelope.playlists
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
        let mut playlists = self.load_all();
        upsert_playlist(&mut playlists, playlist);
        self.persist_playlists(&playlists)
    }

    fn delete(&self, id: &str) -> anyhow::Result<()> {
        let mut playlists = self.load_all();
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
    fn test_disk_store_corrupt_json_yields_empty_and_preserves_file() {
        let temp = TempDir::new("playlist-corrupt");
        std::fs::create_dir_all(temp.path()).unwrap();
        let store = DiskPlaylistStore::new(temp.path());

        let corrupt_payload = "{not json";
        std::fs::write(store.file_path(), corrupt_payload).unwrap();

        assert!(store.list().is_empty());
        assert_eq!(store.get("local:any"), None);

        let content_after = std::fs::read_to_string(store.file_path()).unwrap();
        assert_eq!(content_after, corrupt_payload);
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
}
