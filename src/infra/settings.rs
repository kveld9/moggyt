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
    /// Whether community SponsorBlock segments are fetched and skipped automatically.
    #[serde(default = "default_true")]
    pub sponsorblock_enabled: bool,
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
            sponsorblock_enabled: true,
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
#[path = "settings_tests.rs"]
mod tests;
