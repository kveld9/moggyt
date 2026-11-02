//! Custom theme JSON file watcher and loader.
//!
//! Loads user-defined palette overrides from `custom-theme.json` in the configuration
//! directory and detects file modifications via timestamp inspection.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Context;

use crate::domain::Palette;

/// Standard file name for the user-defined custom theme JSON file.
pub const CUSTOM_THEME_FILE_NAME: &str = "custom-theme.json";

/// File watcher and parser for `custom-theme.json`.
#[derive(Debug)]
pub struct CustomThemeFile {
    path: PathBuf,
    loaded_modified: Option<SystemTime>,
}

impl CustomThemeFile {
    /// Creates a new custom theme handle for the given configuration directory.
    pub fn new(config_dir: &Path) -> Self {
        Self { path: config_dir.join(CUSTOM_THEME_FILE_NAME), loaded_modified: None }
    }

    /// Returns the configured path to the custom theme file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Checks if the file has changed on disk since the last load attempt.
    pub fn has_changed(&self) -> bool {
        let current_modified = std::fs::metadata(&self.path).ok().and_then(|m| m.modified().ok());
        current_modified != self.loaded_modified
    }

    /// Loads and parses the custom theme palette, updating the tracked modification timestamp.
    pub fn load(&mut self) -> anyhow::Result<Palette> {
        self.loaded_modified = std::fs::metadata(&self.path).ok().and_then(|m| m.modified().ok());

        let raw = std::fs::read_to_string(&self.path).with_context(|| {
            format!("failed to read custom theme file at {}", self.path.display())
        })?;

        let map: BTreeMap<String, String> = serde_json::from_str(&raw).with_context(|| {
            format!("failed to parse custom theme JSON at {}", self.path.display())
        })?;

        Palette::from_custom_entries(map.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .map_err(|err| anyhow::anyhow!("{err}"))
            .with_context(|| format!("invalid custom theme definition in {}", self.path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("moggyt-test-custom-theme-{tag}-{}", std::process::id()))
    }

    #[test]
    fn test_custom_theme_load_valid_file() {
        let temp_dir = test_temp_dir("valid");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let json = r##"{
            "mode": "dark",
            "window": "#121318",
            "panel": "#1e1f25",
            "surface": "#282a2f",
            "text": "#e2e2e9",
            "secondary": "#c5c6d0",
            "accent": "#b1c5ff",
            "border": "#44464f",
            "hover": "#33343a",
            "overlay": "#38393f"
        }"##;
        let mut custom = CustomThemeFile::new(&temp_dir);
        assert_eq!(custom.path(), temp_dir.join(CUSTOM_THEME_FILE_NAME));
        std::fs::write(custom.path(), json).unwrap();
        assert!(custom.has_changed());

        let palette = custom.load().expect("load valid theme");
        assert!(palette.dark);
        assert_eq!(palette.window, [0x12, 0x13, 0x18]);
        assert_eq!(palette.accent, [0xb1, 0xc5, 0xff]);
        assert!(!custom.has_changed());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_custom_theme_missing_file_fails_and_clears_changed() {
        let temp_dir = test_temp_dir("missing");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut custom = CustomThemeFile::new(&temp_dir);
        assert!(!custom.has_changed());

        assert!(custom.load().is_err());
        assert!(!custom.has_changed());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_custom_theme_rewriting_with_different_mtime_triggers_has_changed() {
        let temp_dir = test_temp_dir("mtime");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut custom = CustomThemeFile::new(&temp_dir);
        let path = custom.path().to_path_buf();

        std::fs::write(&path, r##"{"mode": "light", "window": "#ffffff"}"##).unwrap();
        let palette = custom.load().expect("load light theme");
        assert!(!palette.dark);
        assert!(!custom.has_changed());

        std::fs::write(&path, r##"{"mode": "dark", "window": "#000000"}"##).unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        let future_time = SystemTime::now() + std::time::Duration::from_secs(3600);
        file.set_modified(future_time).unwrap();
        drop(file);

        assert!(custom.has_changed());

        let updated = custom.load().expect("reload updated theme");
        assert!(updated.dark);
        assert_eq!(updated.window, [0, 0, 0]);
        assert!(!custom.has_changed());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
