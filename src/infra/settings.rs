//! Settings persistence adapter.
//!
//! Handles loading and saving application preferences (`settings.json`).
//! Tranche 1 default: ThemeId::White + Locale::English.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::audio::clamp_speed;
use super::store_health::{HealthFlag, StoreHealth, quarantine};
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

/// Maximum allowed crossfade duration in seconds.
pub const MAX_CROSSFADE_SECONDS: u32 = 15;

/// Schema version of persisted settings.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

const fn default_schema_version() -> u32 {
    SETTINGS_SCHEMA_VERSION
}

/// Clamps crossfade duration in seconds to the valid `0..=15` range.
pub const fn clamp_crossfade_seconds(seconds: u32) -> u32 {
    if seconds > MAX_CROSSFADE_SECONDS { MAX_CROSSFADE_SECONDS } else { seconds }
}

const fn default_crossfade_seconds() -> u32 {
    0
}
const fn default_stream_cache_limit_mb() -> u32 {
    512
}

/// Persistent user settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Schema version for persistence migrations and compatibility.
    #[serde(default = "default_schema_version")]
    pub version: u32,
    /// Active UI theme.
    pub theme: ThemeId,
    /// Active custom theme name when Custom theme is selected.
    #[serde(default)]
    pub custom_theme_name: Option<String>,
    /// Active interface locale.
    pub locale: Locale,
    /// Playback volume level (0..=100).
    #[serde(default = "default_volume")]
    pub volume: u8,
    /// Playback speed factor (0.5..=2.0).
    #[serde(default = "default_playback_speed")]
    pub playback_speed: f32,
    /// Duration of crossfade transition between tracks in seconds (0..=15, 0 = disabled).
    #[serde(default = "default_crossfade_seconds")]
    pub crossfade_seconds: u32,
    /// Disk cache size limit for audio streams in megabytes (0..=10240, 0 = disabled).
    #[serde(default = "default_stream_cache_limit_mb")]
    pub stream_cache_limit_mb: u32,
    /// Preferred audio streaming quality.
    #[serde(default)]
    pub quality: AudioQuality,
    /// Whether Discord Rich Presence is enabled.
    #[serde(default = "default_false")]
    pub discord_presence_enabled: bool,
    /// Last.fm API Key.
    #[serde(default)]
    pub lastfm_api_key: String,
    /// Last.fm API Secret.
    #[serde(default)]
    pub lastfm_api_secret: String,
    /// Whether Last.fm live scrobbling is enabled.
    #[serde(default = "default_false")]
    pub lastfm_enabled: bool,
    /// Display name of the connected Last.fm account.
    #[serde(default)]
    pub lastfm_username: String,
    /// Whether secondary Last.fm live scrobbling is enabled.
    #[serde(default = "default_false")]
    pub secondary_lastfm_enabled: bool,
    /// Whether high-resolution cover artwork decoding is enabled.
    #[serde(default = "default_false")]
    pub hi_res_covers: bool,
    /// Whether only songs (dropping music videos) are preferred.
    #[serde(default = "default_false")]
    pub songs_only: bool,
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
            version: SETTINGS_SCHEMA_VERSION,
            theme: ThemeId::White,
            custom_theme_name: None,
            locale: Locale::English,
            volume: 100,
            playback_speed: 1.0,
            crossfade_seconds: 0,
            stream_cache_limit_mb: 512,
            quality: AudioQuality::Standard,
            discord_presence_enabled: false,
            lastfm_api_key: String::new(),
            lastfm_api_secret: String::new(),
            lastfm_enabled: false,
            lastfm_username: String::new(),
            secondary_lastfm_enabled: false,
            hi_res_covers: false,
            songs_only: false,
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
        Self { locale: Locale::detect(), ..Self::default() }
    }

    /// Sanitizes and clamps setting values to their valid operational bounds.
    pub fn sanitize(&mut self) {
        self.volume = self.volume.min(100);
        self.playback_speed = clamp_speed(self.playback_speed);
        self.crossfade_seconds = clamp_crossfade_seconds(self.crossfade_seconds);
        self.stream_cache_limit_mb = self.stream_cache_limit_mb.min(10240);
    }
}

/// Port for loading and persisting user settings.
pub trait SettingsStore: Send + Sync {
    /// Loads settings, returning default if absent or unreadable.
    fn load(&self) -> Settings;
    /// Persists settings to storage.
    fn save(&self, settings: &Settings) -> anyhow::Result<()>;
    /// Reports the health of the store after the last load.
    fn health(&self) -> StoreHealth {
        StoreHealth::Ok
    }
    /// Returns true if the store payload was corrupt and quarantined.
    fn is_quarantined(&self) -> bool {
        self.health() == StoreHealth::Quarantined
    }
    /// Clears any reported health errors on the store.
    fn clear_health(&self) {}
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
        settings.sanitize();
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
    health: HealthFlag,
}

impl DiskSettingsStore {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir, health: HealthFlag::new() }
    }

    pub fn file_path(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    pub fn clear_health(&self) {
        self.health.clear();
    }

    fn write_atomic(&self, data: &[u8]) -> anyhow::Result<()> {
        super::atomic_file::write_atomic(&self.file_path(), data)
    }

    fn quarantine_corrupt(&self, path: &Path, err: &serde_json::Error) {
        self.health.set(StoreHealth::Quarantined);
        tracing::warn!("corrupt settings file at {}: {err}", path.display());
        match quarantine(path) {
            Ok(b) => tracing::warn!("settings file quarantined to {}", b.display()),
            Err(e) => tracing::warn!("failed to quarantine settings file: {e}"),
        }
    }
}

impl SettingsStore for DiskSettingsStore {
    fn health(&self) -> StoreHealth {
        self.health.get()
    }
    fn is_quarantined(&self) -> bool {
        self.health.is_quarantined()
    }
    fn clear_health(&self) {
        self.health.clear();
    }

    fn load(&self) -> Settings {
        let path = self.file_path();
        let Ok(bytes) = std::fs::read(&path) else {
            return Settings::fallback();
        };
        let mut settings: Settings = match serde_json::from_slice(&bytes) {
            Ok(s) => s,
            Err(err) => {
                self.quarantine_corrupt(&path, &err);
                return Settings::fallback();
            }
        };
        if settings.version > SETTINGS_SCHEMA_VERSION {
            self.health.set(StoreHealth::Incompatible);
            tracing::warn!(
                "unsupported settings schema version {} in {}",
                settings.version,
                path.display()
            );
            return Settings::fallback();
        }
        self.health.set(StoreHealth::Ok);
        settings.sanitize();
        settings
    }

    fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        if self.health.get() != StoreHealth::Ok {
            anyhow::bail!("settings store is blocked until the persistence error is acknowledged");
        }
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

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("moggyt-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            let _ = std::fs::create_dir_all(&path);
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

    fn corrupt_files(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().contains(".corrupt."))
            .collect()
    }

    #[test]
    fn test_1_given_no_settings_file_when_loaded_then_returns_default() {
        let temp = TempDir::new("test-1");
        let store = DiskSettingsStore::new(temp.path());
        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn test_damaged_settings_json_is_quarantined_not_overwritten() {
        let temp = TempDir::new("test-dmg");
        let store = DiskSettingsStore::new(temp.path());
        let payload = b"{\"theme\": \"white\", \"locale\": ";
        std::fs::write(store.file_path(), payload).unwrap();

        assert_eq!(store.load(), Settings::default());
        assert!(store.is_quarantined());
        assert!(!store.file_path().exists());

        let corrupt = corrupt_files(&temp.path());
        assert_eq!(corrupt.len(), 1);
        assert_eq!(std::fs::read(&corrupt[0]).unwrap(), payload);
        assert!(!store.file_path().exists());
    }

    #[test]
    fn second_corruption_preserves_first_backup() {
        let temp = TempDir::new("test-dmg2");
        let store = DiskSettingsStore::new(temp.path());
        let payload1 = b"corrupt payload 1";
        std::fs::write(store.file_path(), payload1).unwrap();
        assert_eq!(store.load(), Settings::default());

        let payload2 = b"corrupt payload 2";
        std::fs::write(store.file_path(), payload2).unwrap();
        assert_eq!(store.load(), Settings::default());

        let files = corrupt_files(&temp.path());
        assert_eq!(files.len(), 2);
        let contents: Vec<_> = files.iter().map(|p| std::fs::read(p).unwrap()).collect();
        assert!(contents.contains(&payload1.to_vec()) && contents.contains(&payload2.to_vec()));
    }

    #[test]
    fn newer_settings_schema_is_incompatible_not_empty() {
        let temp = TempDir::new("test-newer");
        let store = DiskSettingsStore::new(temp.path());
        let payload = br#"{"version": 2, "theme": "white", "locale": "en"}"#;
        std::fs::write(store.file_path(), payload).unwrap();

        assert_eq!(store.load(), Settings::default());
        assert_eq!(store.health(), StoreHealth::Incompatible);
        assert!(!store.is_quarantined());
        assert_eq!(std::fs::read(store.file_path()).unwrap(), payload);
    }

    #[test]
    fn settings_without_version_still_loads() {
        let temp = TempDir::new("test-nover");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), br#"{"theme": "black-oled", "locale": "en"}"#).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.theme, ThemeId::BlackOled);
        assert_eq!(store.health(), StoreHealth::Ok);
    }

    #[test]
    fn save_writes_current_schema_version() {
        let temp = TempDir::new("test-save-ver");
        let store = DiskSettingsStore::new(temp.path());
        store.save(&Settings::default()).expect("save succeeds");

        let bytes = std::fs::read(store.file_path()).expect("read file");
        let val: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
        assert_eq!(val["version"], SETTINGS_SCHEMA_VERSION);
    }

    #[test]
    fn clear_health_allows_future_loads() {
        let temp = TempDir::new("test-clr-health");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), b"broken payload").unwrap();
        let _ = store.load();
        assert!(store.is_quarantined());

        store.clear_health();
        assert_eq!(store.health(), StoreHealth::Ok);
    }

    #[test]
    fn test_quarantined_settings_block_writes_until_ack() {
        let temp = TempDir::new("test-block-ack");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), b"broken payload").unwrap();
        let _ = store.load();
        assert!(store.is_quarantined());

        assert!(store.save(&Settings::default()).is_err());
        store.clear_health();
        assert!(store.save(&Settings::default()).is_ok());
    }

    #[test]
    fn test_5_given_settings_change_when_saved_then_round_trips_through_load_with_atomic_write() {
        let temp = TempDir::new("test-5");
        let store = DiskSettingsStore::new(temp.path());

        let modified = Settings {
            version: SETTINGS_SCHEMA_VERSION,
            theme: ThemeId::BlackOled,
            custom_theme_name: Some("rose-pine".to_string()),
            locale: Locale::Spanish,
            volume: 75,
            playback_speed: 1.25,
            crossfade_seconds: 5,
            stream_cache_limit_mb: 1024,
            quality: AudioQuality::High,
            discord_presence_enabled: true,
            lastfm_api_key: "api-key-abc".to_string(),
            lastfm_api_secret: "api-secret-xyz".to_string(),
            lastfm_enabled: true,
            lastfm_username: "testuser".to_string(),
            secondary_lastfm_enabled: true,
            hi_res_covers: true,
            songs_only: true,
            skip_silence: true,
            loudness_normalization: true,
            player_bar_mode: PlayerBarMode::Floating,
            player_bar_show_skip_seconds: true,
            player_bar_show_artwork: false,
            player_bar_show_like: false,
            player_bar_show_shuffle: false,
            player_bar_show_repeat: false,
            player_bar_show_queue: false,
            player_bar_show_volume: false,
            ..Settings::default()
        };

        store.save(&modified).expect("save should succeed");
        assert_eq!(store.load(), modified);
        assert!(store.file_path().exists());
    }

    #[test]
    fn test_8_given_fallback_settings_when_constructed_then_locale_is_detected() {
        let fallback = Settings::fallback();
        assert_eq!(fallback.theme, ThemeId::White);
        assert_eq!(fallback.locale, Locale::detect());
        assert_eq!(fallback.volume, 100);
        assert_eq!(fallback, Settings { locale: Locale::detect(), ..Settings::default() });
    }

    #[test]
    fn test_old_schema_settings_json_backward_compatibility() {
        let json = r#"{"theme": "white", "locale": "en"}"#;
        let loaded: Settings = serde_json::from_str(json).expect("deserialization succeeds");
        assert!(loaded.lastfm_api_key.is_empty());
        assert!(loaded.lastfm_api_secret.is_empty());
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn test_lastfm_credentials_serde_defaults_and_roundtrip() {
        let empty: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert!(empty.lastfm_api_key.is_empty() && empty.lastfm_api_secret.is_empty());
        let json = r#"{"lastfm_api_key": "my-key", "lastfm_api_secret": "my-secret"}"#;
        let loaded: Settings = serde_json::from_str(json).expect("deserialize explicit");
        assert_eq!(loaded.lastfm_api_key, "my-key");
        assert_eq!(loaded.lastfm_api_secret, "my-secret");
        let roundtrip = serde_json::to_string(&loaded).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert_eq!(decoded.lastfm_api_key, "my-key");
        assert_eq!(decoded.lastfm_api_secret, "my-secret");
    }

    #[test]
    fn test_songs_only_setting_defaults_false_and_roundtrips() {
        let default_settings = Settings::default();
        assert!(!default_settings.songs_only);
        let missing_key: Settings = serde_json::from_str("{}").expect("deserialize empty json");
        assert!(!missing_key.songs_only);
        let json_true = r#"{"songs_only": true}"#;
        let loaded: Settings = serde_json::from_str(json_true).expect("deserialize true");
        assert!(loaded.songs_only);
        let roundtrip = serde_json::to_string(&loaded).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert!(decoded.songs_only);
    }

    #[test]
    fn test_player_bar_settings_serde_defaults_and_explicit_deserialization() {
        let empty: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(empty.player_bar_mode, PlayerBarMode::Classic);
        assert!(empty.player_bar_show_artwork && empty.player_bar_show_like);
        assert!(empty.player_bar_show_shuffle && empty.player_bar_show_repeat);
        assert!(!empty.player_bar_show_skip_seconds && empty.player_bar_show_queue);
        assert!(empty.player_bar_show_volume);

        let explicit_json = r#"{
            "player_bar_mode": "floating", "player_bar_show_artwork": false,
            "player_bar_show_like": false, "player_bar_show_shuffle": false,
            "player_bar_show_repeat": false, "player_bar_show_skip_seconds": true,
            "player_bar_show_queue": false, "player_bar_show_volume": false
        }"#;
        let explicit: Settings = serde_json::from_str(explicit_json).expect("deserialize explicit");
        assert_eq!(explicit.player_bar_mode, PlayerBarMode::Floating);
        assert!(!explicit.player_bar_show_artwork && !explicit.player_bar_show_like);
        assert!(!explicit.player_bar_show_shuffle && !explicit.player_bar_show_repeat);
        assert!(explicit.player_bar_show_skip_seconds && !explicit.player_bar_show_queue);
        assert!(!explicit.player_bar_show_volume);
    }

    #[test]
    fn test_volume_clamped_on_load_from_disk() {
        let temp = TempDir::new("test-clamp");
        let store = DiskSettingsStore::new(temp.path());

        let json = r#"{"theme": "mono", "locale": "es", "volume": 150, "quality": "compact"}"#;
        std::fs::write(store.file_path(), json).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.theme, ThemeId::Mono);
        assert_eq!(loaded.locale, Locale::Spanish);
        assert_eq!(loaded.volume, 100);
        assert_eq!(loaded.playback_speed, 1.0);
        assert_eq!(loaded.quality, AudioQuality::Compact);
        assert!(!loaded.discord_presence_enabled && !loaded.lastfm_enabled);
        assert!(loaded.lastfm_username.is_empty() && !loaded.secondary_lastfm_enabled);
        assert!(!loaded.hi_res_covers && !loaded.skip_silence && !loaded.loudness_normalization);
        assert_eq!(loaded.player_bar_mode, PlayerBarMode::Classic);
        assert!(loaded.player_bar_show_artwork && loaded.player_bar_show_like);
        assert!(loaded.player_bar_show_shuffle && loaded.player_bar_show_repeat);
        assert!(!loaded.player_bar_show_skip_seconds && loaded.player_bar_show_queue);
        assert!(loaded.player_bar_show_volume);
        assert_eq!(loaded.crossfade_seconds, 0);
    }

    #[test]
    fn test_legacy_keyboard_shortcuts_key_is_ignored_on_load() {
        let legacy_json = r#"{"keyboard_shortcuts": false, "volume": 80}"#;
        let loaded: Settings = serde_json::from_str(legacy_json)
            .expect("deserialize legacy settings with keyboard_shortcuts");
        assert_eq!(loaded.volume, 80);
    }

    #[test]
    fn test_discord_presence_setting_defaults_false_and_roundtrips() {
        let default_settings = Settings::default();
        assert!(!default_settings.discord_presence_enabled);
        let missing_key: Settings = serde_json::from_str("{}").expect("deserialize empty json");
        assert!(!missing_key.discord_presence_enabled);
        let json_true = r#"{"discord_presence_enabled": true}"#;
        let loaded: Settings = serde_json::from_str(json_true).expect("deserialize true");
        assert!(loaded.discord_presence_enabled);
        let roundtrip = serde_json::to_string(&loaded).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert!(decoded.discord_presence_enabled);
    }

    #[test]
    fn test_lastfm_and_hires_covers_serde_defaults_and_explicit_deserialization() {
        let with_both_false: Settings =
            serde_json::from_str(r#"{"lastfm_enabled": false, "hi_res_covers": false}"#)
                .expect("deserialize false");
        assert!(!with_both_false.lastfm_enabled && with_both_false.lastfm_username.is_empty());
        assert!(!with_both_false.hi_res_covers);
        let with_both_true: Settings = serde_json::from_str(
            r#"{"lastfm_enabled": true, "lastfm_username": "moggy", "hi_res_covers": true}"#,
        )
        .expect("deserialize true");
        assert!(with_both_true.lastfm_enabled && with_both_true.hi_res_covers);
        assert_eq!(with_both_true.lastfm_username, "moggy");
        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert!(!omitted.lastfm_enabled && omitted.lastfm_username.is_empty());
        assert!(!omitted.hi_res_covers);
    }

    #[test]
    fn test_dsp_settings_serde_defaults_and_explicit_deserialization() {
        let with_both_false: Settings =
            serde_json::from_str(r#"{"skip_silence": false, "loudness_normalization": false}"#)
                .expect("deserialize false");
        assert!(!with_both_false.skip_silence && !with_both_false.loudness_normalization);

        let with_both_true: Settings =
            serde_json::from_str(r#"{"skip_silence": true, "loudness_normalization": true}"#)
                .expect("deserialize true");
        assert!(with_both_true.skip_silence && with_both_true.loudness_normalization);

        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert!(!omitted.skip_silence && !omitted.loudness_normalization);
    }

    #[test]
    fn test_secondary_lastfm_serde_back_compat() {
        // Old settings payload lacking secondary_lastfm_enabled
        let json = r#"{"theme": "white", "locale": "en", "lastfm_enabled": true}"#;
        let loaded: Settings = serde_json::from_str(json).expect("deserialization succeeds");
        assert!(loaded.lastfm_enabled);
        assert!(!loaded.secondary_lastfm_enabled);

        // Explicit true deserializes as true
        let loaded_true: Settings =
            serde_json::from_str(r#"{"secondary_lastfm_enabled": true}"#).unwrap();
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
        let temp = TempDir::new("test-speed-clamp");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), r#"{"playback_speed": 99.0}"#).unwrap();
        assert_eq!(store.load().playback_speed, 2.0);
        std::fs::write(store.file_path(), r#"{"playback_speed": 0.1}"#).unwrap();
        assert_eq!(store.load().playback_speed, 0.5);
        std::fs::write(store.file_path(), r#"{"playback_speed": -1.0}"#).unwrap();
        assert_eq!(store.load().playback_speed, 1.0);
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

    #[test]
    fn test_crossfade_seconds_serde_defaults_and_explicit_deserialization() {
        let explicit: Settings =
            serde_json::from_str(r#"{"crossfade_seconds": 8}"#).expect("deserialize explicit");
        assert_eq!(explicit.crossfade_seconds, 8);
        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(omitted.crossfade_seconds, 0);
        let roundtrip = serde_json::to_string(&explicit).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert_eq!(decoded.crossfade_seconds, 8);
    }

    #[test]
    fn test_crossfade_seconds_clamped_on_load_from_disk() {
        let temp = TempDir::new("test-crossfade-clamp");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), r#"{"crossfade_seconds": 99}"#).unwrap();
        assert_eq!(store.load().crossfade_seconds, MAX_CROSSFADE_SECONDS);
        std::fs::write(store.file_path(), r#"{"crossfade_seconds": 0}"#).unwrap();
        assert_eq!(store.load().crossfade_seconds, 0);
        std::fs::write(store.file_path(), r#"{"crossfade_seconds": 7}"#).unwrap();
        assert_eq!(store.load().crossfade_seconds, 7);
    }

    #[test]
    fn test_clamp_crossfade_seconds_boundary_cases() {
        assert_eq!(clamp_crossfade_seconds(0), 0);
        assert_eq!(clamp_crossfade_seconds(1), 1);
        assert_eq!(clamp_crossfade_seconds(15), 15);
        assert_eq!(clamp_crossfade_seconds(16), 15);
        assert_eq!(clamp_crossfade_seconds(100), 15);
    }

    #[test]
    fn test_stream_cache_limit_mb_serde_defaults_and_explicit_deserialization() {
        let explicit: Settings = serde_json::from_str(r#"{"stream_cache_limit_mb": 1024}"#)
            .expect("deserialize explicit");
        assert_eq!(explicit.stream_cache_limit_mb, 1024);
        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(omitted.stream_cache_limit_mb, 512);
        let roundtrip = serde_json::to_string(&explicit).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert_eq!(decoded.stream_cache_limit_mb, 1024);
    }

    #[test]
    fn test_stream_cache_limit_mb_clamped_on_sanitize_and_load() {
        let mut settings = Settings { stream_cache_limit_mb: 20000, ..Settings::default() };
        settings.sanitize();
        assert_eq!(settings.stream_cache_limit_mb, 10240);

        let temp = TempDir::new("test-stream-cache-clamp");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), r#"{"stream_cache_limit_mb": 50000}"#).unwrap();
        assert_eq!(store.load().stream_cache_limit_mb, 10240);
    }

    #[test]
    fn test_custom_theme_name_serde_defaults_and_explicit_deserialization() {
        let explicit: Settings = serde_json::from_str(r#"{"custom_theme_name": "rose-pine"}"#)
            .expect("deserialize explicit");
        assert_eq!(explicit.custom_theme_name.as_deref(), Some("rose-pine"));
        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(omitted.custom_theme_name, None);
        let roundtrip = serde_json::to_string(&explicit).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert_eq!(decoded.custom_theme_name.as_deref(), Some("rose-pine"));
    }
}
