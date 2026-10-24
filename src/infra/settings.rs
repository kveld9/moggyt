//! Settings persistence adapter.
//!
//! Handles loading and saving application preferences (`settings.json`).
//! Tranche 1 default: ThemeId::White + Locale::English.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::domain::ThemeId;
use crate::i18n::Locale;

/// Persistent user settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Active UI theme.
    pub theme: ThemeId,
    /// Active interface locale.
    pub locale: Locale,
}

impl Default for Settings {
    /// Tranche 1 default settings: White theme + English locale.
    fn default() -> Self {
        Self { theme: ThemeId::White, locale: Locale::English }
    }
}

impl Settings {
    /// Default fallback settings resolving the locale from system environment.
    pub fn fallback() -> Self {
        Self { theme: ThemeId::White, locale: Locale::detect() }
    }
}

/// Port for loading and persisting user settings.
pub trait SettingsStore: Send + Sync {
    /// Loads settings, returning default if absent or unreadable.
    fn load(&self) -> Settings;
    /// Persists settings to storage.
    fn save(&self, settings: &Settings) -> anyhow::Result<()>;
}

/// In-memory settings store for tests and headless execution.
#[derive(Debug, Clone)]
pub struct MemorySettingsStore {
    inner: Arc<Mutex<Settings>>,
}

impl Default for MemorySettingsStore {
    fn default() -> Self {
        Self { inner: Arc::new(Mutex::new(Settings::default())) }
    }
}

impl SettingsStore for MemorySettingsStore {
    fn load(&self) -> Settings {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("mutex poisoned: {e}"))?;
        *guard = settings.clone();
        Ok(())
    }
}

/// Disk-backed settings store writing to `settings.json`.
#[derive(Debug, Clone)]
pub struct DiskSettingsStore {
    config_dir: PathBuf,
}

impl DiskSettingsStore {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    pub fn file_path(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    fn temp_file_path(&self) -> PathBuf {
        let id = std::process::id();
        self.config_dir.join(format!("settings.json.tmp.{id}"))
    }

    fn write_atomic(&self, data: &[u8]) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        let tmp = self.temp_file_path();
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, self.file_path())?;
        Ok(())
    }
}

impl SettingsStore for DiskSettingsStore {
    fn load(&self) -> Settings {
        let path = self.file_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return Settings::fallback();
        };
        serde_json::from_slice(&bytes).unwrap_or_else(|_| Settings::fallback())
    }

    fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        let json = serde_json::to_vec_pretty(settings)?;
        self.write_atomic(&json)
    }
}

/// Resolves standard platform configuration directory.
pub fn default_config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("moggyt");
        }
        PathBuf::from(".").join(".moggyt")
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(xdg).join("moggyt");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".config/moggyt");
        }
        PathBuf::from(".").join(".moggyt")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_1_given_no_settings_file_when_loaded_then_returns_default() {
        let temp_dir = std::env::temp_dir().join(format!("moggyt-test-1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskSettingsStore::new(temp_dir.clone());

        let loaded = store.load();
        assert_eq!(loaded.theme, ThemeId::White);
        assert_eq!(loaded.locale, Locale::English);
        assert_eq!(loaded, Settings::default());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_4_given_damaged_settings_json_when_loading_then_returns_default_without_panic() {
        let temp_dir = std::env::temp_dir().join(format!("moggyt-test-4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store = DiskSettingsStore::new(temp_dir.clone());

        // 1. Partial/truncated JSON
        std::fs::write(store.file_path(), b"{\"theme\": \"white\", \"locale\": ").unwrap();
        assert_eq!(store.load(), Settings::default());

        // 2. Corrupt binary garbage
        std::fs::write(store.file_path(), [0xFF, 0xFE, 0x00, 0x01]).unwrap();
        assert_eq!(store.load(), Settings::default());

        // 3. Schema mismatch
        std::fs::write(store.file_path(), b"{\"theme\": 99999}").unwrap();
        assert_eq!(store.load(), Settings::default());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_5_given_settings_change_when_saved_then_round_trips_through_load_with_atomic_write() {
        let temp_dir = std::env::temp_dir().join(format!("moggyt-test-5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let store = DiskSettingsStore::new(temp_dir.clone());

        let modified = Settings { theme: ThemeId::BlackOled, locale: Locale::Spanish };

        store.save(&modified).expect("save should succeed");

        let loaded = store.load();
        assert_eq!(loaded, modified);

        // Verify file was written to disk and can be re-loaded
        assert!(store.file_path().exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_8_given_fallback_settings_when_constructed_then_locale_is_detected() {
        let fallback = Settings::fallback();
        assert_eq!(fallback.theme, ThemeId::White);
        assert_eq!(fallback.locale, Locale::detect());
    }
}
