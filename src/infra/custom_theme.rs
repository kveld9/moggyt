//! Custom theme JSON file watcher and loader.
//!
//! Loads user-defined palette overrides from `<config_dir>/themes/*.json` or
//! `custom-theme.json` in the configuration directory and detects file modifications
//! via timestamp inspection.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Context;

use crate::domain::Palette;

/// Standard file name for the user-defined custom theme JSON file.
pub const CUSTOM_THEME_FILE_NAME: &str = "custom-theme.json";

/// File watcher and parser for custom theme JSON files.
#[derive(Debug)]
pub struct CustomThemeFile {
    config_dir: PathBuf,
    themes_dir: PathBuf,
    fallback_path: PathBuf,
    active_name: Option<String>,
    active_path: PathBuf,
    loaded_modified: Option<SystemTime>,
}

fn collect_themes_from_dir(dir: &Path, themes: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                themes.push(stem.to_string());
            }
        }
    }
}

impl CustomThemeFile {
    /// Creates a new custom theme handle for the given configuration directory.
    pub fn new(config_dir: &Path) -> Self {
        let config_dir = config_dir.to_path_buf();
        let themes_dir = config_dir.join("themes");
        let fallback_path = config_dir.join(CUSTOM_THEME_FILE_NAME);
        let active_path = fallback_path.clone();
        Self {
            config_dir,
            themes_dir,
            fallback_path,
            active_name: None,
            active_path,
            loaded_modified: None,
        }
    }

    /// Creates a handle pinned to `custom-theme.json`, the output path used by the matugen template.
    pub fn for_matugen(config_dir: &Path) -> Self {
        let mut handle = Self::new(config_dir);
        handle.active_path = handle.fallback_path.clone();
        handle
    }

    /// Discovers available custom themes from `<config_dir>/themes/*.json` and `custom-theme.json`.
    pub fn available_themes(&self) -> Vec<String> {
        let mut themes = Vec::new();
        if self.themes_dir.is_dir() {
            collect_themes_from_dir(&self.themes_dir, &mut themes);
        }
        if self.fallback_path.is_file() {
            themes.push("custom-theme".to_string());
        }
        themes.sort();
        themes.dedup();
        themes
    }

    /// Switches the active theme file to the specified theme name, or the first available theme.
    pub fn select_theme(&mut self, name: Option<&str>) -> bool {
        match name {
            Some(n) => {
                if n == "custom-theme" {
                    self.active_path = self.fallback_path.clone();
                    self.active_name = Some("custom-theme".to_string());
                } else {
                    self.active_path = self.themes_dir.join(format!("{n}.json"));
                    self.active_name = Some(n.to_string());
                }
                self.loaded_modified = None;
                true
            }
            None => {
                let available = self.available_themes();
                if let Some(first) = available.first() {
                    self.select_theme(Some(first))
                } else {
                    self.active_name = None;
                    self.active_path = self.fallback_path.clone();
                    self.loaded_modified = None;
                    false
                }
            }
        }
    }

    /// Returns the configuration directory.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Returns the active custom theme name, if explicitly selected.
    pub fn active_theme_name(&self) -> Option<&str> {
        self.active_name.as_deref()
    }

    /// Returns the configured path to the active custom theme file.
    pub fn path(&self) -> &Path {
        &self.active_path
    }

    /// Checks if the active file has changed on disk since the last load attempt.
    pub fn has_changed(&self) -> bool {
        let current_modified =
            std::fs::metadata(&self.active_path).ok().and_then(|m| m.modified().ok());
        current_modified != self.loaded_modified
    }

    /// Loads and parses the custom theme palette, updating the tracked modification timestamp.
    pub fn load(&mut self) -> anyhow::Result<Palette> {
        self.loaded_modified =
            std::fs::metadata(&self.active_path).ok().and_then(|m| m.modified().ok());

        let raw = std::fs::read_to_string(&self.active_path).with_context(|| {
            format!("failed to read theme file at {}", self.active_path.display())
        })?;

        let map: BTreeMap<String, String> = serde_json::from_str(&raw).with_context(|| {
            format!("failed to parse custom theme JSON at {}", self.active_path.display())
        })?;

        Palette::from_custom_entries(map.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .map_err(|err| anyhow::anyhow!("{err}"))
            .with_context(|| {
                format!("invalid custom theme definition in {}", self.active_path.display())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("moggyt-test-custom-theme-{tag}-{}", std::process::id()))
    }

    #[test]
    fn test_for_matugen_pinned_to_fallback_path() {
        let temp_dir = test_temp_dir("matugen");
        let handle = CustomThemeFile::for_matugen(&temp_dir);
        assert_eq!(handle.path(), temp_dir.join(CUSTOM_THEME_FILE_NAME));
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
        assert_eq!(custom.config_dir(), &temp_dir);
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

    #[test]
    fn test_available_themes_scanning_and_sorting() {
        let temp_dir = test_temp_dir("scan-sort");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let themes_dir = temp_dir.join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();

        std::fs::write(themes_dir.join("solarized.json"), "{}").unwrap();
        std::fs::write(themes_dir.join("nord.json"), "{}").unwrap();
        std::fs::write(themes_dir.join("catppuccin.json"), "{}").unwrap();
        std::fs::write(themes_dir.join("ignore.txt"), "").unwrap();
        std::fs::write(temp_dir.join(CUSTOM_THEME_FILE_NAME), "{}").unwrap();

        let custom = CustomThemeFile::new(&temp_dir);
        let available = custom.available_themes();
        assert_eq!(
            available,
            vec![
                "catppuccin".to_string(),
                "custom-theme".to_string(),
                "nord".to_string(),
                "solarized".to_string(),
            ]
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_available_themes_deduplication() {
        let temp_dir = test_temp_dir("dedup");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let themes_dir = temp_dir.join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();

        std::fs::write(themes_dir.join("custom-theme.json"), "{}").unwrap();
        std::fs::write(temp_dir.join(CUSTOM_THEME_FILE_NAME), "{}").unwrap();

        let custom = CustomThemeFile::new(&temp_dir);
        let available = custom.available_themes();
        assert_eq!(available, vec!["custom-theme".to_string()]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_select_theme_switching_between_files() {
        let temp_dir = test_temp_dir("select");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let themes_dir = temp_dir.join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();

        let mut custom = CustomThemeFile::new(&temp_dir);
        assert_eq!(custom.active_theme_name(), None);
        assert_eq!(custom.path(), temp_dir.join(CUSTOM_THEME_FILE_NAME));

        assert!(custom.select_theme(Some("rose-pine")));
        assert_eq!(custom.active_theme_name(), Some("rose-pine"));
        assert_eq!(custom.path(), themes_dir.join("rose-pine.json"));

        assert!(custom.select_theme(Some("custom-theme")));
        assert_eq!(custom.active_theme_name(), Some("custom-theme"));
        assert_eq!(custom.path(), temp_dir.join(CUSTOM_THEME_FILE_NAME));

        // When themes are available and None is passed, selects first
        std::fs::write(themes_dir.join("dracula.json"), "{}").unwrap();
        assert!(custom.select_theme(None));
        assert_eq!(custom.active_theme_name(), Some("dracula"));
        assert_eq!(custom.path(), themes_dir.join("dracula.json"));

        let _ = std::fs::remove_dir_all(&temp_dir);

        // When no themes are available and None is passed, resets to fallback and returns false
        let empty_dir = test_temp_dir("empty");
        let _ = std::fs::remove_dir_all(&empty_dir);
        std::fs::create_dir_all(&empty_dir).unwrap();
        let mut empty_custom = CustomThemeFile::new(&empty_dir);
        assert!(!empty_custom.select_theme(None));
        assert_eq!(empty_custom.active_theme_name(), None);
        assert_eq!(empty_custom.path(), empty_dir.join(CUSTOM_THEME_FILE_NAME));

        let _ = std::fs::remove_dir_all(&empty_dir);
    }

    #[test]
    fn test_has_changed_tracks_active_file() {
        let temp_dir = test_temp_dir("has-changed-active");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let themes_dir = temp_dir.join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();

        let theme_a = themes_dir.join("theme-a.json");
        let theme_b = themes_dir.join("theme-b.json");
        std::fs::write(&theme_a, r##"{"mode": "dark"}"##).unwrap();
        std::fs::write(&theme_b, r##"{"mode": "light"}"##).unwrap();

        let mut custom = CustomThemeFile::new(&temp_dir);
        custom.select_theme(Some("theme-a"));
        assert!(custom.has_changed());
        let _ = custom.load();
        assert!(!custom.has_changed());

        // Switch to theme-b: loaded_modified was reset to None, so has_changed() is true
        custom.select_theme(Some("theme-b"));
        assert!(custom.has_changed());
        let _ = custom.load();
        assert!(!custom.has_changed());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
