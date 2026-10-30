//! Pure, I/O-free player bar presentation preferences and option-index model.
//!
//! Hexagonal architecture invariant: this module operates strictly in-memory
//! with zero I/O, zero network, zero window dependencies, and zero async runtimes.

use crate::domain::PlayerBarMode;
use crate::infra::Settings;

/// Number of customizable player bar element toggles.
pub const PLAYER_BAR_OPTION_COUNT: usize = 7;

/// Customizable player bar preferences cached in the app state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerBarPrefs {
    /// Active layout mode: docked full-width classic or rounded centered floating.
    pub mode: PlayerBarMode,
    /// Whether track cover artwork is displayed.
    pub show_artwork: bool,
    /// Whether the favorite/like heart toggle is displayed.
    pub show_like: bool,
    /// Whether the shuffle toggle button is displayed.
    pub show_shuffle: bool,
    /// Whether the repeat cycle button is displayed.
    pub show_repeat: bool,
    /// Whether the -10s / +10s seek jump buttons are displayed.
    pub show_skip_seconds: bool,
    /// Whether the queue inspection button is displayed.
    pub show_queue: bool,
    /// Whether the speaker icon and volume slider are displayed.
    pub show_volume: bool,
}

impl Default for PlayerBarPrefs {
    fn default() -> Self {
        Self {
            mode: PlayerBarMode::Classic,
            show_artwork: true,
            show_like: true,
            show_shuffle: true,
            show_repeat: true,
            show_skip_seconds: false,
            show_queue: true,
            show_volume: true,
        }
    }
}

impl PlayerBarPrefs {
    /// Extracts player bar preferences from persisted application settings.
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            mode: settings.player_bar_mode,
            show_artwork: settings.player_bar_show_artwork,
            show_like: settings.player_bar_show_like,
            show_shuffle: settings.player_bar_show_shuffle,
            show_repeat: settings.player_bar_show_repeat,
            show_skip_seconds: settings.player_bar_show_skip_seconds,
            show_queue: settings.player_bar_show_queue,
            show_volume: settings.player_bar_show_volume,
        }
    }

    /// Writes player bar preferences back into application settings.
    pub fn apply_to_settings(&self, settings: &mut Settings) {
        settings.player_bar_mode = self.mode;
        settings.player_bar_show_artwork = self.show_artwork;
        settings.player_bar_show_like = self.show_like;
        settings.player_bar_show_shuffle = self.show_shuffle;
        settings.player_bar_show_repeat = self.show_repeat;
        settings.player_bar_show_skip_seconds = self.show_skip_seconds;
        settings.player_bar_show_queue = self.show_queue;
        settings.player_bar_show_volume = self.show_volume;
    }

    /// Checks if a player bar option is enabled by its shared index.
    ///
    /// Indices: 0 = artwork, 1 = like, 2 = shuffle, 3 = repeat,
    /// 4 = skip-seconds, 5 = queue, 6 = volume.
    pub fn is_enabled(&self, index: i32) -> bool {
        match index {
            0 => self.show_artwork,
            1 => self.show_like,
            2 => self.show_shuffle,
            3 => self.show_repeat,
            4 => self.show_skip_seconds,
            5 => self.show_queue,
            6 => self.show_volume,
            _ => false,
        }
    }

    /// Returns a new copy with exactly one option flipped by index.
    ///
    /// Out-of-range indices return `*self` unchanged without panicking.
    pub fn toggled(&self, index: i32) -> Self {
        let mut copy = *self;
        match index {
            0 => copy.show_artwork = !copy.show_artwork,
            1 => copy.show_like = !copy.show_like,
            2 => copy.show_shuffle = !copy.show_shuffle,
            3 => copy.show_repeat = !copy.show_repeat,
            4 => copy.show_skip_seconds = !copy.show_skip_seconds,
            5 => copy.show_queue = !copy.show_queue,
            6 => copy.show_volume = !copy.show_volume,
            _ => return *self,
        }
        copy
    }
}

/// Converts PlayerBarMode to 0-based integer index (Classic = 0, Floating = 1).
pub fn mode_to_index(mode: PlayerBarMode) -> i32 {
    match mode {
        PlayerBarMode::Classic => 0,
        PlayerBarMode::Floating => 1,
    }
}

/// Converts integer index to PlayerBarMode (0 = Classic, anything else = Floating).
pub fn mode_from_index(index: i32) -> PlayerBarMode {
    match index {
        0 => PlayerBarMode::Classic,
        _ => PlayerBarMode::Floating,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_index_round_trip() {
        assert_eq!(mode_to_index(PlayerBarMode::Classic), 0);
        assert_eq!(mode_to_index(PlayerBarMode::Floating), 1);

        assert_eq!(mode_from_index(0), PlayerBarMode::Classic);
        assert_eq!(mode_from_index(1), PlayerBarMode::Floating);
        assert_eq!(mode_from_index(-1), PlayerBarMode::Floating);
        assert_eq!(mode_from_index(42), PlayerBarMode::Floating);
    }

    #[test]
    fn test_is_enabled_matches_fields() {
        let prefs = PlayerBarPrefs {
            mode: PlayerBarMode::Floating,
            show_artwork: true,
            show_like: false,
            show_shuffle: true,
            show_repeat: false,
            show_skip_seconds: true,
            show_queue: false,
            show_volume: true,
        };

        assert!(prefs.is_enabled(0));
        assert!(!prefs.is_enabled(1));
        assert!(prefs.is_enabled(2));
        assert!(!prefs.is_enabled(3));
        assert!(prefs.is_enabled(4));
        assert!(!prefs.is_enabled(5));
        assert!(prefs.is_enabled(6));

        assert!(!prefs.is_enabled(-1));
        assert!(!prefs.is_enabled(7));
        assert!(!prefs.is_enabled(100));
    }

    #[test]
    fn test_toggled_flips_single_option_and_preserves_others() {
        let base = PlayerBarPrefs::default();

        for i in 0..PLAYER_BAR_OPTION_COUNT as i32 {
            let toggled = base.toggled(i);
            assert_ne!(
                base.is_enabled(i),
                toggled.is_enabled(i),
                "expected index {i} to be flipped"
            );

            for other in 0..PLAYER_BAR_OPTION_COUNT as i32 {
                if other != i {
                    assert_eq!(
                        base.is_enabled(other),
                        toggled.is_enabled(other),
                        "expected index {other} to be untouched when flipping {i}"
                    );
                }
            }
            assert_eq!(base.mode, toggled.mode);
        }
    }

    #[test]
    fn test_toggled_out_of_range_is_no_op() {
        let base = PlayerBarPrefs::default();
        assert_eq!(base.toggled(-1), base);
        assert_eq!(base.toggled(7), base);
        assert_eq!(base.toggled(999), base);
    }

    #[test]
    fn test_from_and_apply_to_settings_round_trip() {
        let settings = Settings {
            player_bar_mode: PlayerBarMode::Floating,
            player_bar_show_artwork: false,
            player_bar_show_like: false,
            player_bar_show_shuffle: false,
            player_bar_show_repeat: false,
            player_bar_show_skip_seconds: true,
            player_bar_show_queue: false,
            player_bar_show_volume: false,
            ..Settings::default()
        };

        let prefs = PlayerBarPrefs::from_settings(&settings);
        assert_eq!(prefs.mode, PlayerBarMode::Floating);
        assert!(!prefs.show_artwork);
        assert!(!prefs.show_like);
        assert!(!prefs.show_shuffle);
        assert!(!prefs.show_repeat);
        assert!(prefs.show_skip_seconds);
        assert!(!prefs.show_queue);
        assert!(!prefs.show_volume);

        let mut round_trip_settings = Settings::default();
        prefs.apply_to_settings(&mut round_trip_settings);

        assert_eq!(round_trip_settings.player_bar_mode, settings.player_bar_mode);
        assert_eq!(round_trip_settings.player_bar_show_artwork, settings.player_bar_show_artwork);
        assert_eq!(round_trip_settings.player_bar_show_like, settings.player_bar_show_like);
        assert_eq!(round_trip_settings.player_bar_show_shuffle, settings.player_bar_show_shuffle);
        assert_eq!(round_trip_settings.player_bar_show_repeat, settings.player_bar_show_repeat);
        assert_eq!(
            round_trip_settings.player_bar_show_skip_seconds,
            settings.player_bar_show_skip_seconds
        );
        assert_eq!(round_trip_settings.player_bar_show_queue, settings.player_bar_show_queue);
        assert_eq!(round_trip_settings.player_bar_show_volume, settings.player_bar_show_volume);
    }
}
