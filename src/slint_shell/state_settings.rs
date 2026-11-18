//! Persisted settings mutations for `SlintAppState`.

use crate::domain::{PlayerBarMode, ThemeId};
use crate::infra::CoverCache;
use crate::slint_shell::state::SlintAppState;

impl SlintAppState {
    /// Updates active UI theme and persists preference to disk settings store.
    pub fn set_theme(&mut self, theme: ThemeId) {
        self.theme = theme;
        let mut settings = self.settings_store.load();
        settings.theme = theme;
        let _ = self.settings_store.save(&settings);
    }

    /// Toggles the compact detail track list preference and persists it to disk.
    pub fn toggle_detail_compact_tracks(&mut self) {
        self.detail_compact_tracks = !self.detail_compact_tracks;
        let mut settings = self.settings_store.load();
        settings.detail_compact_tracks = self.detail_compact_tracks;
        let _ = self.settings_store.save(&settings);
    }

    /// Sets or clears the preset accent override and persists it.
    pub fn set_accent_override(&mut self, accent: Option<[u8; 3]>) {
        self.accent_override = accent;
        let mut settings = self.settings_store.load();
        settings.accent_override = accent.map(crate::domain::format_hex_rgb);
        let _ = self.settings_store.save(&settings);
    }

    /// Updates the crossfade duration setting and persists preference to disk settings store.
    pub fn set_crossfade_seconds(&mut self, seconds: u32) {
        let clamped = crate::infra::clamp_crossfade_seconds(seconds);
        self.crossfade_seconds = clamped;
        let mut settings = self.settings_store.load();
        settings.crossfade_seconds = clamped;
        let _ = self.settings_store.save(&settings);
    }

    /// Updates the stream cache size limit and persists preference to disk settings store.
    pub fn set_stream_cache_limit_mb(&mut self, limit_mb: u32) {
        let clamped = limit_mb.min(10240);
        self.stream_cache_limit_mb = clamped;
        let mut settings = self.settings_store.load();
        settings.stream_cache_limit_mb = clamped;
        let _ = self.settings_store.save(&settings);
    }

    /// Updates the cover cache size limit, applies it to the live disk cache, and persists it.
    pub fn set_cover_cache_limit_mb(&mut self, limit_mb: u32) {
        let clamped = limit_mb.min(10240);
        self.cover_cache_limit_mb = clamped;
        if let Ok(mut covers) = self.covers.lock() {
            covers.set_limit(u64::from(clamped) * 1024 * 1024);
        }
        let mut settings = self.settings_store.load();
        settings.cover_cache_limit_mb = clamped;
        let _ = self.settings_store.save(&settings);
    }

    /// Updates the player bar layout mode and persists to settings store.
    pub fn set_player_bar_mode(&mut self, mode: PlayerBarMode) {
        self.player_bar.mode = mode;
        let mut settings = self.settings_store.load();
        self.player_bar.apply_to_settings(&mut settings);
        let _ = self.settings_store.save(&settings);
    }

    /// Updates the floating player bar width cap and persists it to the settings store.
    pub fn set_player_bar_floating_width(&mut self, width: u32) {
        self.player_bar.floating_width = crate::infra::clamp_player_bar_floating_width(width);
        let mut settings = self.settings_store.load();
        self.player_bar.apply_to_settings(&mut settings);
        let _ = self.settings_store.save(&settings);
    }

    /// Toggles an individual player bar element option by index and persists to settings store.
    pub fn toggle_player_bar_option(&mut self, index: i32) {
        self.player_bar = self.player_bar.toggled(index);
        let mut settings = self.settings_store.load();
        self.player_bar.apply_to_settings(&mut settings);
        let _ = self.settings_store.save(&settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_settings_persist_to_store_and_state() {
        let mut state = test_wired();
        assert_eq!(state.theme, ThemeId::White);
        state.set_theme(ThemeId::BlackOled);
        assert_eq!(state.settings_store.load().theme, ThemeId::BlackOled);
        state.set_theme(ThemeId::Mono);
        assert_eq!(state.settings_store.load().theme, ThemeId::Mono);
        assert_eq!(state.player_bar.mode, PlayerBarMode::Classic);
        state.set_player_bar_mode(PlayerBarMode::Floating);
        assert_eq!(state.settings_store.load().player_bar_mode, PlayerBarMode::Floating);
        state.set_player_bar_mode(PlayerBarMode::Classic);
        assert_eq!(state.settings_store.load().player_bar_mode, PlayerBarMode::Classic);
        assert!(state.player_bar.show_artwork && !state.player_bar.show_skip_seconds);
        state.toggle_player_bar_option(0);
        assert!(!state.settings_store.load().player_bar_show_artwork);
        state.toggle_player_bar_option(4);
        assert!(state.settings_store.load().player_bar_show_skip_seconds);
        assert!(!state.detail_compact_tracks);
        state.toggle_detail_compact_tracks();
        assert!(state.detail_compact_tracks);
        assert!(state.settings_store.load().detail_compact_tracks);
        state.hi_res_covers = false;
        assert_eq!(state.cover_max_px(), 512);
        state.hi_res_covers = true;
        assert_eq!(state.cover_max_px(), 1024);
        state.set_crossfade_seconds(5);
        assert_eq!(state.settings_store.load().crossfade_seconds, 5);
        state.set_crossfade_seconds(20);
        assert_eq!(state.settings_store.load().crossfade_seconds, 15);
        state.set_stream_cache_limit_mb(1024);
        assert_eq!(state.settings_store.load().stream_cache_limit_mb, 1024);
        state.set_stream_cache_limit_mb(20000);
        assert_eq!(state.settings_store.load().stream_cache_limit_mb, 10240);
        state.set_cover_cache_limit_mb(128);
        assert_eq!(state.cover_cache_limit_mb, 128);
        assert_eq!(state.settings_store.load().cover_cache_limit_mb, 128);
        state.set_cover_cache_limit_mb(20000);
        assert_eq!(state.cover_cache_limit_mb, 10240);
        state.set_player_bar_floating_width(1200);
        assert_eq!(state.player_bar.floating_width, 1200);
        assert_eq!(state.settings_store.load().player_bar_floating_width, 1200);
        state.set_player_bar_floating_width(5000);
        assert_eq!(state.settings_store.load().player_bar_floating_width, 1600);
        state.set_player_bar_floating_width(100);
        assert_eq!(state.settings_store.load().player_bar_floating_width, 800);
    }

    #[test]
    fn test_accent_override_persists_to_store_and_state() {
        let mut state = test_wired();
        state.set_accent_override(Some([0x12, 0x34, 0x56]));
        assert_eq!(state.accent_override, Some([0x12, 0x34, 0x56]));
        assert_eq!(state.settings_store.load().accent_override.as_deref(), Some("#123456"));
        state.set_accent_override(None);
        assert_eq!(state.accent_override, None);
        assert_eq!(state.settings_store.load().accent_override, None);
    }
}
