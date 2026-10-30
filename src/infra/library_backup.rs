//! Library backup import and export adapter.
//!
//! Provides JSON library serialization, atomic disk persistence,
//! backup schema validation, and additive catalog restoration.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::catalog::Catalog;
use crate::domain::{Album, Playlist, Track};

/// Canonical backup schema version for library backups.
pub const CURRENT_BACKUP_VERSION: u32 = 1;

/// Serialized representation of a user's library backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryBackup {
    /// Backup schema format version (currently 1).
    pub version: u32,
    /// Unix timestamp in seconds when the backup was exported (supplied by caller).
    pub exported_at_unix: u64,
    /// Preserved playlist collections.
    pub playlists: Vec<Playlist>,
    /// Preserved liked tracks.
    pub liked_tracks: Vec<Track>,
    /// Preserved saved albums.
    pub saved_albums: Vec<Album>,
}

/// Summary of items applied or recorded from an imported backup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackupSummary {
    /// Number of liked tracks applied to the catalog.
    pub liked_applied: usize,
    /// Number of playlists recorded in the backup.
    pub playlists_recorded: usize,
    /// Number of saved albums recorded in the backup.
    pub albums_recorded: usize,
}

/// Creates an in-memory backup snapshot from the catalog with a caller-supplied timestamp.
///
/// This function is pure and does not perform I/O or access system clocks.
pub fn export_backup(catalog: &impl Catalog, exported_at_unix: u64) -> LibraryBackup {
    LibraryBackup {
        version: CURRENT_BACKUP_VERSION,
        exported_at_unix,
        playlists: catalog.playlists(),
        liked_tracks: catalog.liked_tracks(),
        saved_albums: catalog.saved_albums(),
    }
}

/// Atomically writes a backup snapshot to disk inside `dir` using a temporary file and rename.
///
/// Target filename format: `library-<exported_at_unix>.json`.
pub fn write_backup_file(dir: &Path, backup: &LibraryBackup) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create backup directory '{}'", dir.display()))?;
    let filename = format!("library-{}.json", backup.exported_at_unix);
    let target_path = dir.join(&filename);
    let temp_name = format!("{filename}.tmp.{}", std::process::id());
    let temp_path = dir.join(temp_name);

    let bytes =
        serde_json::to_vec_pretty(backup).context("failed to serialize library backup to JSON")?;
    std::fs::write(&temp_path, bytes)
        .with_context(|| format!("failed to write temporary backup '{}'", temp_path.display()))?;
    std::fs::rename(&temp_path, &target_path).with_context(|| {
        format!(
            "failed to rename temporary backup '{}' to '{}'",
            temp_path.display(),
            target_path.display()
        )
    })?;
    Ok(target_path)
}

/// Reads and validates a library backup JSON file from `path`.
///
/// Enforces version == 1; mismatched versions or corrupted JSON fail loudly.
pub fn read_backup_file(path: impl AsRef<Path>) -> anyhow::Result<LibraryBackup> {
    let path_ref = path.as_ref();
    let bytes = std::fs::read(path_ref)
        .with_context(|| format!("failed to read backup file '{}'", path_ref.display()))?;
    let backup: LibraryBackup = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse backup JSON '{}'", path_ref.display()))?;
    if backup.version != CURRENT_BACKUP_VERSION {
        anyhow::bail!(
            "unsupported backup version {} in '{}' (expected {})",
            backup.version,
            path_ref.display(),
            CURRENT_BACKUP_VERSION
        );
    }
    Ok(backup)
}

/// Applies an imported backup to the catalog according to the library backup policy.
///
/// # Policy
/// - **Liked Tracks**: Restored additively. Every non-empty track id in `backup.liked_tracks`
///   is marked as liked via `catalog.set_liked(id, true)`. Existing liked tracks in the
///   catalog are preserved and never unliked.
/// - **Playlists and Saved Albums**: Server-side read-only in YouTube Music API.
///   These collections are preserved in the backup file for portability, metadata tracking,
///   and inspection, but are only recorded and counted in [`BackupSummary`] rather than
///   pushed to the remote server.
pub fn apply_backup(catalog: &mut impl Catalog, backup: &LibraryBackup) -> BackupSummary {
    let mut liked_applied = 0;
    for track in &backup.liked_tracks {
        let trimmed = track.id.trim();
        if !trimmed.is_empty() {
            catalog.set_liked(trimmed, true);
            liked_applied += 1;
        }
    }
    BackupSummary {
        liked_applied,
        playlists_recorded: backup.playlists.len(),
        albums_recorded: backup.saved_albums.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::CatalogStub;

    fn temp_test_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("moggyt-test-backup-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create test temp dir");
        dir
    }

    #[test]
    fn export_round_trips_all_three_collections() {
        let dir = temp_test_dir("roundtrip");
        let catalog = CatalogStub::new();
        let timestamp = 1_700_000_123;

        let backup = export_backup(&catalog, timestamp);
        assert_eq!(backup.version, CURRENT_BACKUP_VERSION);
        assert_eq!(backup.exported_at_unix, timestamp);
        assert_eq!(backup.playlists, catalog.playlists());
        assert_eq!(backup.liked_tracks, catalog.liked_tracks());
        assert_eq!(backup.saved_albums, catalog.saved_albums());

        let written_path = write_backup_file(&dir, &backup).expect("write backup");
        let loaded = read_backup_file(&written_path).expect("read backup");

        assert_eq!(backup, loaded);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn filename_embeds_timestamp() {
        let dir = temp_test_dir("filename");
        let catalog = CatalogStub::new();
        let timestamp = 1_729_999_888;

        let backup = export_backup(&catalog, timestamp);
        let written_path = write_backup_file(&dir, &backup).expect("write backup");

        assert_eq!(
            written_path.file_name().and_then(|n| n.to_str()),
            Some("library-1729999888.json")
        );
        assert!(written_path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn version_mismatch_rejected() {
        let dir = temp_test_dir("version");
        let path = dir.join("library-version2.json");
        let raw = r#"{
            "version": 2,
            "exported_at_unix": 1700000000,
            "playlists": [],
            "liked_tracks": [],
            "saved_albums": []
        }"#;
        std::fs::write(&path, raw).expect("write raw json");

        let err = read_backup_file(&path).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("unsupported backup version 2"));
        assert!(message.contains("expected 1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_json_rejected_loudly() {
        let dir = temp_test_dir("corrupt");
        let path = dir.join("corrupt.json");
        std::fs::write(&path, b"{\"version\": 1, \"playlists\": [").expect("write corrupt json");

        let err = read_backup_file(&path).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("failed to parse backup JSON"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_is_additive_and_counts_correctly() {
        let mut catalog = CatalogStub::new();
        // CatalogStub default has "stub-1" liked.
        assert_eq!(catalog.liked_tracks().len(), 1);
        assert_eq!(catalog.liked_tracks()[0].id, "stub-1");

        let backup = LibraryBackup {
            version: CURRENT_BACKUP_VERSION,
            exported_at_unix: 1700000000,
            playlists: vec![
                Playlist::new("p1", "Playlist 1", 5, None),
                Playlist::new("p2", "Playlist 2", 10, None),
            ],
            liked_tracks: vec![
                Track::new("stub-1", "Liked Song", "Stub Artist"),
                Track::new("stub-2", "B-side", "Stub Artist"),
            ],
            saved_albums: vec![Album::new("a1", "Album 1", "Artist 1", None)],
        };

        let summary = apply_backup(&mut catalog, &backup);

        assert_eq!(summary.liked_applied, 2);
        assert_eq!(summary.playlists_recorded, 2);
        assert_eq!(summary.albums_recorded, 1);

        // Pre-existing liked track (stub-1) is kept and stub-2 was added.
        let liked = catalog.liked_tracks();
        assert_eq!(liked.len(), 2);
        let ids: Vec<_> = liked.iter().map(|t| t.id.as_str()).collect();
        assert!(ids.contains(&"stub-1"));
        assert!(ids.contains(&"stub-2"));
    }

    #[test]
    fn apply_skips_blank_track_ids() {
        let mut catalog = CatalogStub::new();
        let backup = LibraryBackup {
            version: CURRENT_BACKUP_VERSION,
            exported_at_unix: 1700000000,
            playlists: Vec::new(),
            liked_tracks: vec![
                Track::new("", "Empty", "Artist"),
                Track::new("   ", "Spaces", "Artist"),
                Track::new("stub-2", "B-side", "Stub Artist"),
            ],
            saved_albums: Vec::new(),
        };

        let summary = apply_backup(&mut catalog, &backup);
        assert_eq!(summary.liked_applied, 1);
        assert!(catalog.liked_tracks().iter().any(|t| t.id == "stub-2"));
    }
}
