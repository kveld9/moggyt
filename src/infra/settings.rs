//! Settings persistence adapter.
//!
//! Handles loading and saving application preferences (`settings.json`).
//! Tranche 1 default: ThemeId::White + Locale::English.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::audio::clamp_speed;
use super::stream::AudioQuality;
use crate::domain::{PlayerBarMode, RecentSearches, ThemeId};
use crate::i18n::Locale;

const fn default_true() -> bool {
    true
}

const fn default_false() -> bool {
    false
}

const fn default_volume() -> u8 {
    100
}

const fn default_playback_speed() -> f32 {
    1.0
}

/// Persistent user settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Active UI theme.
    pub theme: ThemeId,
    /// Active interface locale.
    pub locale: Locale,
    /// Playback volume level (0..=100).
    #[serde(default = "default_volume")]
    pub volume: u8,
    /// Playback speed factor (0.5..=2.0).
    #[serde(default = "default_playback_speed")]
    pub playback_speed: f32,
    /// Preferred audio streaming quality.
    #[serde(default)]
    pub quality: AudioQuality,
    /// Whether global keyboard shortcuts are enabled.
    #[serde(default = "default_true")]
    pub keyboard_shortcuts: bool,
    /// Whether Last.fm live scrobbling is enabled.
    #[serde(default = "default_false")]
    pub lastfm_enabled: bool,
    /// Whether secondary Last.fm live scrobbling is enabled.
    #[serde(default = "default_false")]
    pub secondary_lastfm_enabled: bool,
    /// Whether high-resolution cover artwork decoding is enabled.
    #[serde(default = "default_false")]
    pub hi_res_covers: bool,
    /// Whether leading/trailing silence should be skipped during playback.
    #[serde(default = "default_false")]
    pub skip_silence: bool,
    /// Whether loudness normalization to -14 LUFS is enabled.
    #[serde(default = "default_false")]
    pub loudness_normalization: bool,
    /// Presentation mode of the playback bar.
    #[serde(default)]
    pub player_bar_mode: PlayerBarMode,
    /// Whether cover artwork thumbnail is shown in the player bar.
    #[serde(default = "default_true")]
    pub player_bar_show_artwork: bool,
    /// Whether like button toggle is shown in the player bar.
    #[serde(default = "default_true")]
    pub player_bar_show_like: bool,
    /// Whether shuffle button is shown in the player bar.
    #[serde(default = "default_true")]
    pub player_bar_show_shuffle: bool,
    /// Whether repeat mode button is shown in the player bar.
    #[serde(default = "default_true")]
    pub player_bar_show_repeat: bool,
    /// Whether skip 10s buttons are shown in the player bar.
    #[serde(default = "default_false")]
    pub player_bar_show_skip_seconds: bool,
    /// Whether queue button is shown in the player bar.
    #[serde(default = "default_true")]
    pub player_bar_show_queue: bool,
    /// Whether volume slider is shown in the player bar.
    #[serde(default = "default_true")]
    pub player_bar_show_volume: bool,
    /// Recent search queries (capped, newest first).
    #[serde(default)]
    pub recent_searches: RecentSearches,
}

impl Default for Settings {
    /// Tranche 1 default settings: White theme + English locale + volume 100 + speed 1.0 + Standard quality.
    fn default() -> Self {
        Self {
            theme: ThemeId::White,
            locale: Locale::English,
            volume: 100,
            playback_speed: 1.0,
            quality: AudioQuality::Standard,
            keyboard_shortcuts: true,
            lastfm_enabled: false,
            secondary_lastfm_enabled: false,
            hi_res_covers: false,
            skip_silence: false,
            loudness_normalization: false,
            player_bar_mode: PlayerBarMode::Classic,
            player_bar_show_artwork: true,
            player_bar_show_like: true,
            player_bar_show_shuffle: true,
            player_bar_show_repeat: true,
            player_bar_show_skip_seconds: false,
            player_bar_show_queue: true,
            player_bar_show_volume: true,
            recent_searches: RecentSearches::default(),
        }
    }
}

impl Settings {
    /// Default fallback settings resolving the locale from system environment.
    pub fn fallback() -> Self {
        Self {
            theme: ThemeId::White,
            locale: Locale::detect(),
            volume: 100,
            playback_speed: 1.0,
            quality: AudioQuality::Standard,
            keyboard_shortcuts: true,
            lastfm_enabled: false,
            secondary_lastfm_enabled: false,
            hi_res_covers: false,
            skip_silence: false,
            loudness_normalization: false,
            player_bar_mode: PlayerBarMode::Classic,
            player_bar_show_artwork: true,
            player_bar_show_like: true,
            player_bar_show_shuffle: true,
            player_bar_show_repeat: true,
            player_bar_show_skip_seconds: false,
            player_bar_show_queue: true,
            player_bar_show_volume: true,
            recent_searches: RecentSearches::default(),
        }
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
        let mut settings = self.inner.lock().map(|g| g.clone()).unwrap_or_default();
        settings.volume = settings.volume.min(100);
        settings.playback_speed = clamp_speed(settings.playback_speed);
        settings
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
        let mut settings: Settings =
            serde_json::from_slice(&bytes).unwrap_or_else(|_| Settings::fallback());
        settings.volume = settings.volume.min(100);
        settings.playback_speed = clamp_speed(settings.playback_speed);
        settings
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
        assert_eq!(loaded.volume, 100);
        assert_eq!(loaded.playback_speed, 1.0);
        assert_eq!(loaded.quality, AudioQuality::Standard);
        assert!(loaded.keyboard_shortcuts);
        assert!(!loaded.lastfm_enabled);
        assert!(!loaded.secondary_lastfm_enabled);
        assert!(!loaded.hi_res_covers);
        assert!(!loaded.skip_silence);
        assert!(!loaded.loudness_normalization);
        assert_eq!(loaded.player_bar_mode, PlayerBarMode::Classic);
        assert!(loaded.player_bar_show_artwork);
        assert!(loaded.player_bar_show_like);
        assert!(loaded.player_bar_show_shuffle);
        assert!(loaded.player_bar_show_repeat);
        assert!(!loaded.player_bar_show_skip_seconds);
        assert!(loaded.player_bar_show_queue);
        assert!(loaded.player_bar_show_volume);
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

        let modified = Settings {
            theme: ThemeId::BlackOled,
            locale: Locale::Spanish,
            volume: 75,
            playback_speed: 1.25,
            quality: AudioQuality::High,
            keyboard_shortcuts: false,
            lastfm_enabled: true,
            secondary_lastfm_enabled: true,
            hi_res_covers: true,
            skip_silence: true,
            loudness_normalization: true,
            player_bar_mode: PlayerBarMode::Floating,
            player_bar_show_artwork: false,
            player_bar_show_like: false,
            player_bar_show_shuffle: false,
            player_bar_show_repeat: false,
            player_bar_show_skip_seconds: true,
            player_bar_show_queue: false,
            player_bar_show_volume: false,
            recent_searches: RecentSearches::default(),
        };

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
        assert_eq!(fallback.volume, 100);
        assert_eq!(fallback.playback_speed, 1.0);
        assert_eq!(fallback.quality, AudioQuality::Standard);
        assert!(fallback.keyboard_shortcuts);
        assert!(!fallback.lastfm_enabled);
        assert!(!fallback.secondary_lastfm_enabled);
        assert!(!fallback.hi_res_covers);
        assert!(!fallback.skip_silence);
        assert!(!fallback.loudness_normalization);
        assert_eq!(fallback.player_bar_mode, PlayerBarMode::Classic);
        assert!(fallback.player_bar_show_artwork);
        assert!(fallback.player_bar_show_like);
        assert!(fallback.player_bar_show_shuffle);
        assert!(fallback.player_bar_show_repeat);
        assert!(!fallback.player_bar_show_skip_seconds);
        assert!(fallback.player_bar_show_queue);
        assert!(fallback.player_bar_show_volume);
    }

    #[test]
    fn test_old_schema_settings_json_backward_compatibility() {
        let json = r#"{"theme": "white", "locale": "en"}"#;
        let loaded: Settings = serde_json::from_str(json).expect("deserialization succeeds");
        assert_eq!(loaded.theme, ThemeId::White);
        assert_eq!(loaded.locale, Locale::English);
        assert_eq!(loaded.volume, 100);
        assert_eq!(loaded.playback_speed, 1.0);
        assert_eq!(loaded.quality, AudioQuality::Standard);
        assert!(loaded.keyboard_shortcuts);
        assert!(!loaded.lastfm_enabled);
        assert!(!loaded.secondary_lastfm_enabled);
        assert!(!loaded.hi_res_covers);
        assert!(!loaded.skip_silence);
        assert!(!loaded.loudness_normalization);
        assert_eq!(loaded.player_bar_mode, PlayerBarMode::Classic);
        assert!(loaded.player_bar_show_artwork);
        assert!(loaded.player_bar_show_like);
        assert!(loaded.player_bar_show_shuffle);
        assert!(loaded.player_bar_show_repeat);
        assert!(!loaded.player_bar_show_skip_seconds);
        assert!(loaded.player_bar_show_queue);
        assert!(loaded.player_bar_show_volume);
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn test_player_bar_settings_serde_defaults_and_explicit_deserialization() {
        let empty: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(empty.player_bar_mode, PlayerBarMode::Classic);
        assert!(empty.player_bar_show_artwork);
        assert!(empty.player_bar_show_like);
        assert!(empty.player_bar_show_shuffle);
        assert!(empty.player_bar_show_repeat);
        assert!(!empty.player_bar_show_skip_seconds);
        assert!(empty.player_bar_show_queue);
        assert!(empty.player_bar_show_volume);

        let explicit_json = r#"{
            "player_bar_mode": "floating",
            "player_bar_show_artwork": false,
            "player_bar_show_like": false,
            "player_bar_show_shuffle": false,
            "player_bar_show_repeat": false,
            "player_bar_show_skip_seconds": true,
            "player_bar_show_queue": false,
            "player_bar_show_volume": false
        }"#;
        let explicit: Settings = serde_json::from_str(explicit_json).expect("deserialize explicit");
        assert_eq!(explicit.player_bar_mode, PlayerBarMode::Floating);
        assert!(!explicit.player_bar_show_artwork);
        assert!(!explicit.player_bar_show_like);
        assert!(!explicit.player_bar_show_shuffle);
        assert!(!explicit.player_bar_show_repeat);
        assert!(explicit.player_bar_show_skip_seconds);
        assert!(!explicit.player_bar_show_queue);
        assert!(!explicit.player_bar_show_volume);
    }

    #[test]
    fn test_volume_clamped_on_load_from_disk() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-test-clamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store = DiskSettingsStore::new(temp_dir.clone());

        let json = r#"{"theme": "mono", "locale": "es", "volume": 150, "quality": "compact"}"#;
        std::fs::write(store.file_path(), json).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.theme, ThemeId::Mono);
        assert_eq!(loaded.locale, Locale::Spanish);
        assert_eq!(loaded.volume, 100);
        assert_eq!(loaded.playback_speed, 1.0);
        assert_eq!(loaded.quality, AudioQuality::Compact);
        assert!(loaded.keyboard_shortcuts);
        assert!(!loaded.lastfm_enabled);
        assert!(!loaded.secondary_lastfm_enabled);
        assert!(!loaded.hi_res_covers);
        assert!(!loaded.skip_silence);
        assert!(!loaded.loudness_normalization);
        assert_eq!(loaded.player_bar_mode, PlayerBarMode::Classic);
        assert!(loaded.player_bar_show_artwork);
        assert!(loaded.player_bar_show_like);
        assert!(loaded.player_bar_show_shuffle);
        assert!(loaded.player_bar_show_repeat);
        assert!(!loaded.player_bar_show_skip_seconds);
        assert!(loaded.player_bar_show_queue);
        assert!(loaded.player_bar_show_volume);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_keyboard_shortcuts_serde_defaults_and_explicit_deserialization() {
        let with_false: Settings =
            serde_json::from_str(r#"{"keyboard_shortcuts": false}"#).expect("deserialize false");
        assert!(!with_false.keyboard_shortcuts);

        let with_true: Settings =
            serde_json::from_str(r#"{"keyboard_shortcuts": true}"#).expect("deserialize true");
        assert!(with_true.keyboard_shortcuts);

        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert!(omitted.keyboard_shortcuts);
    }

    #[test]
    fn test_lastfm_and_hires_covers_serde_defaults_and_explicit_deserialization() {
        let with_both_false: Settings =
            serde_json::from_str(r#"{"lastfm_enabled": false, "hi_res_covers": false}"#)
                .expect("deserialize false");
        assert!(!with_both_false.lastfm_enabled);
        assert!(!with_both_false.hi_res_covers);

        let with_both_true: Settings =
            serde_json::from_str(r#"{"lastfm_enabled": true, "hi_res_covers": true}"#)
                .expect("deserialize true");
        assert!(with_both_true.lastfm_enabled);
        assert!(with_both_true.hi_res_covers);

        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert!(!omitted.lastfm_enabled);
        assert!(!omitted.hi_res_covers);
    }

    #[test]
    fn test_dsp_settings_serde_defaults_and_explicit_deserialization() {
        let with_both_false: Settings =
            serde_json::from_str(r#"{"skip_silence": false, "loudness_normalization": false}"#)
                .expect("deserialize false");
        assert!(!with_both_false.skip_silence);
        assert!(!with_both_false.loudness_normalization);

        let with_both_true: Settings =
            serde_json::from_str(r#"{"skip_silence": true, "loudness_normalization": true}"#)
                .expect("deserialize true");
        assert!(with_both_true.skip_silence);
        assert!(with_both_true.loudness_normalization);

        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert!(!omitted.skip_silence);
        assert!(!omitted.loudness_normalization);
    }

    #[test]
    fn test_secondary_lastfm_serde_back_compat() {
        // Old settings payload lacking secondary_lastfm_enabled
        let json = r#"{"theme": "white", "locale": "en", "lastfm_enabled": true}"#;
        let loaded: Settings = serde_json::from_str(json).expect("deserialization succeeds");
        assert!(loaded.lastfm_enabled);
        assert!(!loaded.secondary_lastfm_enabled);

        // Explicit true deserializes as true
        let json_true = r#"{"secondary_lastfm_enabled": true}"#;
        let loaded_true: Settings =
            serde_json::from_str(json_true).expect("deserialization succeeds");
        assert!(loaded_true.secondary_lastfm_enabled);

        // Empty object defaults to false
        let empty: Settings = serde_json::from_str("{}").expect("deserialization succeeds");
        assert!(!empty.secondary_lastfm_enabled);
    }

    #[test]
    fn test_playback_speed_serde_defaults_and_explicit_deserialization() {
        let explicit: Settings =
            serde_json::from_str(r#"{"playback_speed": 1.5}"#).expect("deserialize explicit");
        assert_eq!(explicit.playback_speed, 1.5);

        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(omitted.playback_speed, 1.0);
    }

    #[test]
    fn test_playback_speed_clamped_on_load_from_disk() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-test-speed-clamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let store = DiskSettingsStore::new(temp_dir.clone());

        // Above max -> clamped to 2.0
        std::fs::write(store.file_path(), r#"{"playback_speed": 99.0}"#).unwrap();
        assert_eq!(store.load().playback_speed, 2.0);

        // Below min -> clamped to 0.5
        std::fs::write(store.file_path(), r#"{"playback_speed": 0.1}"#).unwrap();
        assert_eq!(store.load().playback_speed, 0.5);

        // Non-positive -> clamped to 1.0
        std::fs::write(store.file_path(), r#"{"playback_speed": -1.0}"#).unwrap();
        assert_eq!(store.load().playback_speed, 1.0);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_recent_searches_missing_field_loads_empty() {
        let json = r#"{"theme": "white", "locale": "en"}"#;
        let loaded: Settings = serde_json::from_str(json).expect("deserialization succeeds");
        assert!(loaded.recent_searches.is_empty());
        assert_eq!(loaded.recent_searches.len(), 0);
    }

    #[test]
    fn test_recent_searches_round_trips_preserving_order() {
        let mut recents = RecentSearches::default();
        recents.record("first");
        recents.record("second");
        recents.record("third");

        let settings = Settings { recent_searches: recents.clone(), ..Settings::default() };
        let json = serde_json::to_string(&settings).expect("serialize settings");
        let loaded: Settings = serde_json::from_str(&json).expect("deserialize settings");
        assert_eq!(loaded.recent_searches, recents);
        assert_eq!(loaded.recent_searches.entries(), &["third", "second", "first"]);
    }
}
