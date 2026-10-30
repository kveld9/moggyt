//! Slint playback state and view-model bridge.
//!
//! Hexagonal architecture invariant: this module operates purely in-memory
//! with zero I/O, zero network, zero audio, zero rodio, and zero async runtimes.
//!
//! Audio port boundary note:
//! In future tranches, playback controls (play, pause, seek, volume) will delegate to the
//! `crate::infra::AudioBackend` port adapter (e.g. `AudioStub` or `RodioBackend`).
//! This sync bridge provides the pure in-memory boundary holding `PlaybackQueue`,
//! play/pause status, volume, and display models without calling live audio backends.

use std::collections::HashSet;

use crate::domain::RepeatMode;

use super::search::format_duration;

/// Cycles repeat mode in the canonical sequence: Off -> All -> One -> Off.
pub fn cycle_repeat_mode(mode: RepeatMode) -> RepeatMode {
    match mode {
        RepeatMode::Off => RepeatMode::All,
        RepeatMode::All => RepeatMode::One,
        RepeatMode::One => RepeatMode::Off,
    }
}

/// Converts RepeatMode to integer code for UI binding (0: Off, 1: All, 2: One).
pub fn repeat_mode_to_int(mode: RepeatMode) -> i32 {
    match mode {
        RepeatMode::Off => 0,
        RepeatMode::All => 1,
        RepeatMode::One => 2,
    }
}

/// Converts integer code from UI binding back to RepeatMode.
pub fn repeat_mode_from_int(code: i32) -> RepeatMode {
    match code {
        1 => RepeatMode::All,
        2 => RepeatMode::One,
        _ => RepeatMode::Off,
    }
}

/// Toggles track like status in the set of liked track IDs.
/// Returns true if the track is now liked, false if unliked.
pub fn toggle_like(liked: &mut HashSet<String>, track_id: &str) -> bool {
    if liked.contains(track_id) {
        liked.remove(track_id);
        false
    } else {
        liked.insert(track_id.to_string());
        true
    }
}

/// Adds a track ID to the set of liked track IDs.
/// Returns true if the track was newly added, false if already present.
pub fn add_like(liked: &mut HashSet<String>, track_id: &str) -> bool {
    liked.insert(track_id.to_string())
}

/// Removes a track ID from the set of liked track IDs.
/// Returns true if the track was removed, false if not present.
pub fn remove_like(liked: &mut HashSet<String>, track_id: &str) -> bool {
    liked.remove(track_id)
}

/// Checks whether a track ID is present in the liked set.
pub fn is_track_liked(liked: &HashSet<String>, track_id: &str) -> bool {
    liked.contains(track_id)
}

/// Computes playback progress fraction in 0.0..=1.0, guarding against divide-by-zero.
pub fn progress_fraction(position_secs: u32, duration_secs: u32) -> f32 {
    if duration_secs == 0 {
        return 0.0;
    }
    (position_secs as f32 / duration_secs as f32).clamp(0.0, 1.0)
}

/// Checks whether a requested seek position differs sufficiently from current position.
/// Returns true if the difference exceeds 1 second, false if within 1-second feedback threshold.
pub fn should_seek(target_secs: u64, current_secs: u64) -> bool {
    (target_secs as i64 - current_secs as i64).abs() > 1
}

/// Formats playback position in seconds using standard mm:ss display text.
pub fn format_position(position_secs: u32) -> String {
    format_duration(Some(position_secs))
}

/// Formats optional track duration in seconds using standard mm:ss display text.
pub fn format_track_duration(duration_secs: Option<u32>) -> String {
    format_duration(duration_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_string_outputs() {
        for (s, exp) in [(0, "0:00"), (9, "0:09"), (65, "1:05"), (599, "9:59"), (3600, "60:00")] {
            assert_eq!(format_position(s), exp);
        }
        assert_eq!(format_track_duration(None), "");
        assert_eq!(format_track_duration(Some(0)), "0:00");
        assert_eq!(format_track_duration(Some(243)), "4:03");
    }

    #[test]
    fn test_cycle_repeat_mode_transitions() {
        assert_eq!(cycle_repeat_mode(RepeatMode::Off), RepeatMode::All);
        assert_eq!(cycle_repeat_mode(RepeatMode::All), RepeatMode::One);
        assert_eq!(cycle_repeat_mode(RepeatMode::One), RepeatMode::Off);

        assert_eq!(repeat_mode_to_int(RepeatMode::Off), 0);
        assert_eq!(repeat_mode_to_int(RepeatMode::All), 1);
        assert_eq!(repeat_mode_to_int(RepeatMode::One), 2);

        assert_eq!(repeat_mode_from_int(0), RepeatMode::Off);
        assert_eq!(repeat_mode_from_int(1), RepeatMode::All);
        assert_eq!(repeat_mode_from_int(2), RepeatMode::One);
        assert_eq!(repeat_mode_from_int(99), RepeatMode::Off);
    }

    #[test]
    fn test_like_helpers_add_remove_toggle_contains() {
        let mut liked = HashSet::new();
        assert!(!is_track_liked(&liked, "t1"));
        assert!(!remove_like(&mut liked, "t1"));

        assert!(add_like(&mut liked, "t1"));
        assert!(!add_like(&mut liked, "t1"));
        assert!(is_track_liked(&liked, "t1"));

        assert!(!toggle_like(&mut liked, "t1"));
        assert!(!is_track_liked(&liked, "t1"));

        assert!(toggle_like(&mut liked, "t1"));
        assert!(is_track_liked(&liked, "t1"));

        assert!(remove_like(&mut liked, "t1"));
        assert!(!is_track_liked(&liked, "t1"));
    }

    #[test]
    fn test_progress_fraction_bounds_and_division_by_zero() {
        assert_eq!(progress_fraction(0, 0), 0.0);
        assert_eq!(progress_fraction(10, 0), 0.0);
        assert_eq!(progress_fraction(0, 100), 0.0);
        assert_eq!(progress_fraction(50, 100), 0.5);
        assert_eq!(progress_fraction(100, 100), 1.0);
        assert_eq!(progress_fraction(150, 100), 1.0);
    }

    #[test]
    fn test_should_seek_guard_condition() {
        assert!(!should_seek(10, 10));
        assert!(!should_seek(10, 9));
        assert!(!should_seek(10, 11));
        assert!(should_seek(10, 8));
        assert!(should_seek(10, 12));
        assert!(should_seek(0, 50));
    }
}
