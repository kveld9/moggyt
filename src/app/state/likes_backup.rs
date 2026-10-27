//! Library likes and JSON backup export/import for [`AppState`].

use std::path::{Path, PathBuf};

use crate::app::AppState;
use crate::app::state::current_unix_secs;
use crate::infra::{
    AudioBackend, BackupSummary, Catalog, CoverCache, LyricsSource, SessionStore, apply_backup,
    default_config_dir, export_backup, read_backup_file, write_backup_file,
};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Returns true if the track id is present in the user's liked collection.
    pub fn is_liked(&self, track_id: &str) -> bool {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            return false;
        }
        self.catalog.liked_tracks().iter().any(|track| track.id == trimmed)
    }

    /// Toggles the liked status of a track in the catalog. Blank identifiers are ignored.
    pub fn toggle_like(&mut self, track_id: &str) {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            return;
        }
        let next_liked = !self.is_liked(trimmed);
        self.catalog.set_liked(trimmed, next_liked);
    }

    /// Exports the current library into a JSON backup file in the backup directory.
    pub fn export_library_backup(&mut self) -> anyhow::Result<PathBuf> {
        let dir = self.backup_dir.clone().unwrap_or_else(default_config_dir);
        self.export_library_backup_to(&dir)
    }

    /// Exports the current library into `dir` using caller-supplied directory.
    pub fn export_library_backup_to(&mut self, dir: &Path) -> anyhow::Result<PathBuf> {
        let timestamp = current_unix_secs();
        let backup = export_backup(&self.catalog, timestamp);
        let res = write_backup_file(dir, &backup);
        self.record_export_status(&res);
        res
    }

    fn record_export_status(&mut self, res: &anyhow::Result<PathBuf>) {
        match res {
            Ok(path) => self.backup_status_text = format!("Exported to: {}", path.display()),
            Err(err) => self.backup_status_text = format!("Export error: {err:#}"),
        }
    }

    /// Imports a library backup from the given filesystem `path`.
    pub fn import_library_backup(&mut self, path: &str) -> anyhow::Result<BackupSummary> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            let err = anyhow::anyhow!("backup path cannot be blank");
            self.backup_status_text = format!("Import error: {err}");
            return Err(err);
        }
        let res = self.execute_backup_import(trimmed);
        self.record_import_status(&res);
        res
    }

    fn execute_backup_import(&mut self, path: &str) -> anyhow::Result<BackupSummary> {
        let backup = read_backup_file(Path::new(path))?;
        let summary = apply_backup(&mut self.catalog, &backup);
        Ok(summary)
    }

    fn record_import_status(&mut self, res: &anyhow::Result<BackupSummary>) {
        match res {
            Ok(s) => {
                self.backup_status_text = format!(
                    "Imported: {} liked, {} playlists, {} albums",
                    s.liked_applied, s.playlists_recorded, s.albums_recorded
                );
            }
            Err(err) => {
                self.backup_status_text = format!("Import error: {err:#}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use super::*;

    #[test]
    fn toggle_like_flips_state_and_ignores_blank() {
        let mut state = wired();

        // Stub catalog defaults to "stub-1" liked.
        assert!(state.is_liked("stub-1"));
        assert!(!state.is_liked("stub-2"));

        // Toggle existing liked track off.
        state.toggle_like("stub-1");
        assert!(!state.is_liked("stub-1"));

        // Toggle back on.
        state.toggle_like("stub-1");
        assert!(state.is_liked("stub-1"));

        // Toggle unliked track on.
        state.toggle_like("stub-2");
        assert!(state.is_liked("stub-2"));

        // Blank and whitespace ids are ignored.
        state.toggle_like("");
        state.toggle_like("   ");
        assert!(state.is_liked("stub-1"));
        assert!(state.is_liked("stub-2"));
        assert!(!state.is_liked(""));
        assert!(!state.is_liked("   "));
    }

    #[test]
    fn test_blank_import_path_rejected() {
        let mut state = wired();
        for invalid in ["", "   "] {
            let res = state.import_library_backup(invalid);
            assert!(res.is_err());
            assert!(state.backup_status_text.contains("backup path cannot be blank"));
        }
    }

    #[test]
    fn test_app_state_export_clear_likes_import_restores_likes() {
        let mut state = wired();
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-test-app-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).expect("create test temp dir");
        state.backup_dir = Some(temp_dir.clone());

        assert!(state.is_liked("stub-1"));

        let path = state.export_library_backup().expect("export succeeds");
        assert!(path.exists());
        assert!(state.backup_status_text.contains("Exported to:"));

        state.catalog.set_liked("stub-1", false);
        assert!(!state.is_liked("stub-1"));
        assert!(state.catalog.liked_tracks().is_empty());

        let summary = state
            .import_library_backup(path.to_str().expect("valid utf-8 path"))
            .expect("import succeeds");
        assert_eq!(summary.liked_applied, 1);
        assert!(state.is_liked("stub-1"));
        assert!(state.backup_status_text.contains("Imported: 1 liked"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_import_library_backup_nonexistent_file_surfaces_error() {
        let mut state = wired();
        let bad_path = "/tmp/moggyt_definitely_nonexistent_backup.json";
        let res = state.import_library_backup(bad_path);
        assert!(res.is_err());
        assert!(state.backup_status_text.contains("Import error:"));
    }
}
