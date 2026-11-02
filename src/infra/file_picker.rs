//! Native file dialog integration and supported import file validation.

use std::path::{Path, PathBuf};

/// Supported file extensions for Spotify import sources.
pub const IMPORT_FILE_EXTENSIONS: [&str; 2] = ["csv", "json"];

/// Trait abstracting native file dialog selection.
pub trait FilePicker: Send + Sync {
    /// Opens a native file dialog to select a Spotify export file.
    fn pick_import_file(&self, title: &str) -> Option<PathBuf>;
}

/// Native file picker implementation using `rfd`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RfdFilePicker;

impl FilePicker for RfdFilePicker {
    fn pick_import_file(&self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title(title)
            .add_filter("Spotify export", &IMPORT_FILE_EXTENSIONS)
            .pick_file()
    }
}

/// Returns `true` if the file has a supported Spotify import extension (`csv` or `json`).
pub fn is_supported_import_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(std::ffi::OsStr::to_str) else {
        return false;
    };
    IMPORT_FILE_EXTENSIONS.iter().any(|&supported| ext.eq_ignore_ascii_case(supported))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_file_extensions_constant() {
        assert_eq!(IMPORT_FILE_EXTENSIONS, ["csv", "json"]);
    }

    #[test]
    fn test_is_supported_import_file() {
        assert!(is_supported_import_file(Path::new("playlist.csv")));
        assert!(is_supported_import_file(Path::new("playlist.CSV")));
        assert!(is_supported_import_file(Path::new("backup.json")));
        assert!(is_supported_import_file(Path::new("backup.JSON")));
        assert!(!is_supported_import_file(Path::new("playlist.txt")));
        assert!(!is_supported_import_file(Path::new("playlist")));
        assert!(!is_supported_import_file(Path::new("")));
    }
}
