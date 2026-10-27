//! Settings management and persistence for [`AppState`].

use crate::app::AppState;
use crate::domain::ThemeId;
use crate::i18n::Locale;
use crate::infra::{
    AudioBackend, AudioQuality, Catalog, CoverCache, LyricsSource, SessionStore, Settings,
    SettingsStore,
};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Injects an alternative settings store implementation and synchronizes state.
    pub fn set_settings_store(&mut self, store: Box<dyn SettingsStore>) {
        let loaded = store.load();
        self.theme = loaded.theme;
        self.locale = loaded.locale;
        self.quality = loaded.quality;
        self.shortcuts_enabled = loaded.keyboard_shortcuts;
        self.lastfm_enabled = loaded.lastfm_enabled;
        self.secondary_lastfm_enabled = loaded.secondary_lastfm_enabled;
        self.hi_res_covers = loaded.hi_res_covers;
        self.skip_silence = loaded.skip_silence;
        self.loudness_normalization = loaded.loudness_normalization;
        self.audio.set_volume(loaded.volume);
        self.audio.set_quality(loaded.quality);
        self.audio.set_skip_silence(loaded.skip_silence);
        self.audio.set_loudness_normalization(loaded.loudness_normalization);
        self.audio.set_speed(loaded.playback_speed);
        self.settings_store = store;
        self.sync_lastfm_scrobbler();
        self.sync_secondary_lastfm_scrobbler();
    }

    /// Sets the active UI theme and persists the change.
    pub fn set_theme(&mut self, theme: ThemeId) {
        self.theme = theme;
        self.persist_settings();
    }

    /// Sets the active interface locale and persists the change.
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
        self.persist_settings();
    }

    /// Sets the preferred audio streaming quality, forwards to audio backend, and persists the change.
    pub fn set_quality(&mut self, quality: AudioQuality) {
        self.quality = quality;
        self.audio.set_quality(quality);
        self.persist_settings();
    }

    /// Sets whether keyboard shortcuts are enabled and persists the preference.
    pub fn set_shortcuts_enabled(&mut self, enabled: bool) {
        self.shortcuts_enabled = enabled;
        self.persist_settings();
    }

    /// Returns whether keyboard shortcuts are enabled.
    pub fn keyboard_shortcuts_enabled(&self) -> bool {
        self.shortcuts_enabled
    }

    /// Sets whether skip-silence processing is enabled, forwards to audio backend, and persists.
    pub fn set_skip_silence(&mut self, enabled: bool) {
        self.skip_silence = enabled;
        self.audio.set_skip_silence(enabled);
        self.persist_settings();
    }

    /// Sets whether loudness normalization is enabled, forwards to audio backend, and persists.
    pub fn set_loudness_normalization(&mut self, enabled: bool) {
        self.loudness_normalization = enabled;
        self.audio.set_loudness_normalization(enabled);
        self.persist_settings();
    }

    /// Sets whether high-resolution cover artwork is enabled and persists the preference.
    pub fn set_hi_res_covers(&mut self, enabled: bool) {
        if self.hi_res_covers == enabled {
            return;
        }
        self.hi_res_covers = enabled;
        self.clear_cover_texture();
        self.persist_settings();
    }

    pub(crate) fn persist_settings(&self) {
        let settings = Settings {
            theme: self.theme,
            locale: self.locale,
            volume: self.audio.volume(),
            playback_speed: self.audio.speed(),
            quality: self.quality,
            keyboard_shortcuts: self.shortcuts_enabled,
            lastfm_enabled: self.lastfm_enabled,
            secondary_lastfm_enabled: self.secondary_lastfm_enabled,
            hi_res_covers: self.hi_res_covers,
            skip_silence: self.skip_silence,
            loudness_normalization: self.loudness_normalization,
        };
        if let Err(err) = self.settings_store.save(&settings) {
            tracing::warn!("failed to persist settings: {err:#}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{QualityTrackingAudio, wired};
    use super::*;
    use crate::infra::{
        COVER_HIRES_MAX_PX, COVER_MAX_PX, CatalogStub, LrclibStub, MemoryCoverCache,
        MemorySessionStore, MemorySettingsStore,
    };

    #[test]
    fn theme_and_locale_setters_mutate_state_and_persist() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        assert_eq!(state.theme, ThemeId::White);
        assert_eq!(state.locale, Locale::English);
        assert_eq!(state.quality, AudioQuality::Standard);
        assert_eq!(state.audio.volume(), 100);

        state.set_theme(ThemeId::BlackOled);
        assert_eq!(state.theme, ThemeId::BlackOled);
        assert_eq!(store.load().theme, ThemeId::BlackOled);

        state.set_locale(Locale::Spanish);
        assert_eq!(state.locale, Locale::Spanish);
        assert_eq!(store.load().locale, Locale::Spanish);
    }

    #[test]
    fn set_quality_persists_and_forwards_to_audio_backend() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        state.set_quality(AudioQuality::High);
        assert_eq!(state.quality, AudioQuality::High);
        assert_eq!(store.load().quality, AudioQuality::High);

        state.set_quality(AudioQuality::Compact);
        assert_eq!(state.quality, AudioQuality::Compact);
        assert_eq!(store.load().quality, AudioQuality::Compact);
    }

    #[test]
    fn set_settings_store_applies_volume_and_quality_to_stub_backend() {
        let mut state = AppState::new(
            CatalogStub::new(),
            QualityTrackingAudio::default(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );

        let custom_settings = Settings {
            theme: ThemeId::Mono,
            locale: Locale::Spanish,
            volume: 64,
            quality: AudioQuality::Compact,
            keyboard_shortcuts: false,
            lastfm_enabled: false,
            secondary_lastfm_enabled: false,
            hi_res_covers: false,
            playback_speed: 1.0,
            skip_silence: false,
            loudness_normalization: false,
        };
        let store = MemorySettingsStore::default();
        store.save(&custom_settings).unwrap();

        state.set_settings_store(Box::new(store));

        assert_eq!(state.theme, ThemeId::Mono);
        assert_eq!(state.locale, Locale::Spanish);
        assert_eq!(state.quality, AudioQuality::Compact);
        assert!(!state.shortcuts_enabled);
        assert_eq!(state.audio.volume(), 64);
        assert_eq!(state.audio.applied_quality, AudioQuality::Compact);
    }

    #[test]
    fn test_set_shortcuts_enabled_persists_flag() {
        let mut state = wired();
        assert!(state.shortcuts_enabled);
        assert!(state.keyboard_shortcuts_enabled());

        state.set_shortcuts_enabled(false);
        assert!(!state.shortcuts_enabled);
        assert!(!state.keyboard_shortcuts_enabled());
        assert!(!state.settings_store.load().keyboard_shortcuts);

        state.set_shortcuts_enabled(true);
        assert!(state.shortcuts_enabled);
        assert!(state.keyboard_shortcuts_enabled());
        assert!(state.settings_store.load().keyboard_shortcuts);
    }

    #[test]
    fn test_set_settings_store_syncs_shortcuts_flag() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        store
            .save(&Settings {
                theme: ThemeId::White,
                locale: Locale::English,
                volume: 80,
                playback_speed: 1.0,
                quality: AudioQuality::Standard,
                keyboard_shortcuts: false,
                lastfm_enabled: false,
                secondary_lastfm_enabled: false,
                hi_res_covers: false,
                skip_silence: false,
                loudness_normalization: false,
            })
            .unwrap();

        state.set_settings_store(Box::new(store));
        assert!(!state.shortcuts_enabled);
    }

    #[test]
    fn test_hi_res_covers_setting_toggles_decode_bound_and_persists() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        assert!(!state.hi_res_covers);
        assert_eq!(state.cover_max_px(), COVER_MAX_PX);

        state.set_hi_res_covers(true);
        assert!(state.hi_res_covers);
        assert_eq!(state.cover_max_px(), COVER_HIRES_MAX_PX);
        assert!(store.load().hi_res_covers);

        state.set_hi_res_covers(false);
        assert!(!state.hi_res_covers);
        assert_eq!(state.cover_max_px(), COVER_MAX_PX);
        assert!(!store.load().hi_res_covers);
    }

    #[test]
    fn test_dsp_settings_toggle_forward_to_audio_backend_and_persist() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        assert!(!state.skip_silence);
        assert!(!state.loudness_normalization);
        assert!(!state.audio.skip_silence());
        assert!(!state.audio.loudness_normalization());

        state.set_skip_silence(true);
        assert!(state.skip_silence);
        assert!(state.audio.skip_silence());
        assert!(store.load().skip_silence);

        state.set_loudness_normalization(true);
        assert!(state.loudness_normalization);
        assert!(state.audio.loudness_normalization());
        assert!(store.load().loudness_normalization);

        state.set_skip_silence(false);
        assert!(!state.skip_silence);
        assert!(!state.audio.skip_silence());
        assert!(!store.load().skip_silence);

        state.set_loudness_normalization(false);
        assert!(!state.loudness_normalization);
        assert!(!state.audio.loudness_normalization());
        assert!(!store.load().loudness_normalization);
    }

    #[test]
    fn test_dsp_settings_forward_on_set_settings_store() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        let custom_settings =
            Settings { skip_silence: true, loudness_normalization: true, ..Default::default() };
        store.save(&custom_settings).expect("save custom settings");

        state.set_settings_store(Box::new(store));
        assert!(state.skip_silence);
        assert!(state.loudness_normalization);
        assert!(state.audio.skip_silence());
        assert!(state.audio.loudness_normalization());
    }

    #[test]
    fn test_startup_applies_stored_playback_speed() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        let settings = Settings { playback_speed: 1.5, ..Default::default() };
        store.save(&settings).unwrap();

        state.set_settings_store(Box::new(store));
        assert_eq!(state.audio.speed(), 1.5);
    }
}
