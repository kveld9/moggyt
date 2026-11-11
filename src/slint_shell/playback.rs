//! Slint playback state and view-model bridge.
//!
//! Hexagonal architecture invariant: this module operates purely in-memory
//! with zero network, zero audio, zero rodio, and zero async runtimes. Shuffle
//! ordering reads recent plays through the `PlayLogStore` port only.
//!
//! Audio port boundary note:
//! In future tranches, playback controls (play, pause, seek, volume) will delegate to the
//! `crate::infra::AudioBackend` port adapter (e.g. `AudioStub` or `RodioBackend`).
//! This sync bridge provides the pure in-memory boundary holding `PlaybackQueue`,
//! play/pause status, volume, and display models without calling live audio backends.

use std::collections::HashSet;

use crate::domain::{RepeatMode, Track, smart_order};

use super::play_log::recent_video_ids;
use super::search::format_duration;
use super::state::SlintAppState;

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

/// Generates a randomized permutation of indices `0..len` using Fisher-Yates with a system-clock PRNG.
/// If `keep_first` is provided, that index is placed at position 0 so the active track stays selected.
pub fn shuffle_permutation(len: usize, keep_first: Option<usize>) -> Vec<usize> {
    if len <= 1 {
        return (0..len).collect();
    }
    let mut indices: Vec<usize> = (0..len).collect();
    if let Some(first) = keep_first.filter(|&k| k < len) {
        indices.swap(0, first);
        shuffle_slice(&mut indices[1..]);
    } else {
        shuffle_slice(&mut indices);
    }
    indices
}

fn shuffle_slice(slice: &mut [usize]) {
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    for i in (1..slice.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed as usize) % (i + 1);
        slice.swap(i, j);
    }
}

/// Builds a smart shuffle order for `tracks`, keeping `base[0]` at the front.
pub(crate) fn build_shuffle_order(
    tracks: &[Track],
    recent_ids: &[String],
    base: Vec<usize>,
) -> Vec<usize> {
    let artists: Vec<&str> = tracks.iter().map(|track| track.artist.as_str()).collect();
    let is_recent: Vec<bool> = tracks.iter().map(|track| recent_ids.contains(&track.id)).collect();
    smart_order(&artists, &is_recent, &base)
}

impl SlintAppState {
    /// Toggles queue shuffle state, permuting or restoring tracks accordingly.
    pub fn toggle_shuffle(&mut self) {
        self.is_shuffle = !self.is_shuffle;
        if self.is_shuffle {
            self.shuffle_current_queue();
        } else {
            self.restore_unshuffled_queue();
        }
    }

    /// Shuffles the current queue, preserving the currently playing track at cursor 0.
    ///
    /// Recent plays (through the play log port) are pushed toward the end and
    /// back-to-back tracks from the same artist are separated when possible.
    pub fn shuffle_current_queue(&mut self) {
        if self.queue.len() <= 1 {
            return;
        }
        if self.unshuffled_tracks.is_none() {
            self.unshuffled_tracks = Some(self.queue.tracks().to_vec());
        }
        let recent_ids = recent_video_ids(self.play_log_store.as_ref());
        let base = shuffle_permutation(self.queue.len(), Some(self.queue.cursor()));
        let order = build_shuffle_order(self.queue.tracks(), &recent_ids, base);
        self.queue.shuffle_with(&order);
    }

    /// Restores the queue to its original unshuffled order if captured.
    pub fn restore_unshuffled_queue(&mut self) {
        let Some(original) = self.unshuffled_tracks.take() else {
            return;
        };
        let current_id = self.queue.current().map(|t| t.id.clone());
        self.queue.clear();
        for track in original {
            self.queue.push(track);
        }
        if let Some(id) = current_id {
            if let Some(idx) = self.queue.tracks().iter().position(|t| t.id == id) {
                for _ in 0..idx {
                    self.queue.advance();
                }
            }
        }
    }
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

    fn shuffle_tracks() -> Vec<Track> {
        vec![
            Track::new("a1", "Alpha One", "Alpha"),
            Track::new("a2", "Alpha Two", "Alpha"),
            Track::new("b1", "Beta One", "Beta"),
            Track::new("c1", "Gamma One", "Gamma"),
        ]
    }

    #[test]
    fn test_build_shuffle_order_separates_same_artist_after_shuffle() {
        let tracks = shuffle_tracks();
        let order = build_shuffle_order(&tracks, &[], vec![0, 1, 2, 3]);
        assert_eq!(order[0], 0, "active track stays first");
        for pair in order.windows(2) {
            assert_ne!(tracks[pair[0]].artist, tracks[pair[1]].artist, "order: {order:?}");
        }
    }

    #[test]
    fn test_build_shuffle_order_pushes_recent_tracks_last() {
        let tracks = shuffle_tracks();
        let recent = vec!["a1".to_string()];
        let order = build_shuffle_order(&tracks, &recent, vec![2, 1, 0, 3]);
        assert_eq!(order[0], 2, "active track stays first");
        assert_eq!(order[order.len() - 1], 0, "recent track goes last");
    }

    #[test]
    fn test_build_shuffle_order_empty_queue_is_empty() {
        assert!(build_shuffle_order(&[], &[], Vec::new()).is_empty());
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

    #[test]
    fn test_shuffle_permutation_empty_and_single() {
        assert_eq!(shuffle_permutation(0, None), Vec::<usize>::new());
        assert_eq!(shuffle_permutation(0, Some(0)), Vec::<usize>::new());
        assert_eq!(shuffle_permutation(1, None), vec![0]);
        assert_eq!(shuffle_permutation(1, Some(0)), vec![0]);
        assert_eq!(shuffle_permutation(1, Some(5)), vec![0]);
    }

    #[test]
    fn test_shuffle_permutation_validity_and_no_duplicates() {
        for len in [2, 5, 10, 50] {
            let perm = shuffle_permutation(len, None);
            assert_eq!(perm.len(), len);
            let mut sorted = perm.clone();
            sorted.sort_unstable();
            let expected: Vec<usize> = (0..len).collect();
            assert_eq!(sorted, expected);
        }
    }

    #[test]
    fn test_shuffle_permutation_preserves_keep_first_at_index_0() {
        for len in [3, 8, 20] {
            for keep in [0, 1, len - 1] {
                let perm = shuffle_permutation(len, Some(keep));
                assert_eq!(perm.len(), len);
                assert_eq!(perm[0], keep);
                let mut sorted = perm.clone();
                sorted.sort_unstable();
                let expected: Vec<usize> = (0..len).collect();
                assert_eq!(sorted, expected);
            }
        }
    }

    #[test]
    fn test_toggle_shuffle_lifecycle() {
        use crate::domain::Track;
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        let t1 = Track::new("t1", "Track 1", "Artist 1");
        let t2 = Track::new("t2", "Track 2", "Artist 2");
        let t3 = Track::new("t3", "Track 3", "Artist 3");
        let t4 = Track::new("t4", "Track 4", "Artist 4");
        state.queue.push(t1.clone());
        state.queue.push(t2.clone());
        state.queue.push(t3.clone());
        state.queue.push(t4.clone());

        // Advance to t3 (cursor index 2)
        state.queue.advance();
        state.queue.advance();
        assert_eq!(state.queue.cursor(), 2);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t3"));

        // Toggle shuffle ON
        state.toggle_shuffle();
        assert!(state.is_shuffle);
        assert!(state.unshuffled_tracks.is_some());
        assert_eq!(state.queue.len(), 4);
        // The track that was active (t3) must be preserved at index 0 and cursor 0
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t3"));

        // Advance in the shuffled queue
        state.queue.advance();
        let second_track_id = state.queue.current().map(|t| t.id.clone()).unwrap();

        // Toggle shuffle OFF -> should restore original track order and keep second_track active
        state.toggle_shuffle();
        assert!(!state.is_shuffle);
        assert!(state.unshuffled_tracks.is_none());
        let original_ids: Vec<&str> = state.queue.tracks().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(original_ids, vec!["t1", "t2", "t3", "t4"]);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some(second_track_id.as_str()));
    }

    #[test]
    fn test_toggle_shuffle_single_or_empty_queue() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        // Empty queue
        state.toggle_shuffle();
        assert!(state.is_shuffle);
        assert!(state.unshuffled_tracks.is_none());
        state.toggle_shuffle();
        assert!(!state.is_shuffle);

        // Single track queue
        state.queue.push(crate::domain::Track::new("t1", "Track 1", "Artist 1"));
        state.toggle_shuffle();
        assert!(state.is_shuffle);
        assert!(state.unshuffled_tracks.is_none());
        assert_eq!(state.queue.len(), 1);
        state.toggle_shuffle();
        assert!(!state.is_shuffle);
    }
}
