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

use crate::domain::{PlaybackQueue, RepeatMode, Track};

use super::search::{TrackDisplayRow, format_duration};

/// Formats playback position in seconds using standard mm:ss display text.
pub fn format_position(position_secs: u32) -> String {
    format_duration(Some(position_secs))
}

/// Formats optional track duration in seconds using standard mm:ss display text.
pub fn format_track_duration(duration_secs: Option<u32>) -> String {
    format_duration(duration_secs)
}

/// Flattens queue tracks into display rows for the Slint queue view.
pub fn queue_track_rows(queue: &PlaybackQueue) -> Vec<TrackDisplayRow> {
    queue
        .tracks()
        .iter()
        .map(|t| TrackDisplayRow::new(&t.title, &t.artist, format_duration(t.duration_secs)))
        .collect()
}

/// Pure in-memory playback state for the Slint UI shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackState {
    pub queue: PlaybackQueue,
    pub is_playing: bool,
    pub volume: u8,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self { queue: PlaybackQueue::new(), is_playing: false, volume: 100 }
    }
}

impl PlaybackState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_queue(queue: PlaybackQueue) -> Self {
        Self { queue, is_playing: false, volume: 100 }
    }

    pub fn current(&self) -> Option<&Track> {
        self.queue.current()
    }

    /// Selects track by index, adjusting cursor and starting playback.
    pub fn select_track(&mut self, index: usize) -> bool {
        if index >= self.queue.len() {
            return false;
        }
        let saved_repeat = self.queue.repeat();
        self.queue.set_repeat(RepeatMode::Off);
        while self.queue.cursor() < index {
            self.queue.advance();
        }
        while self.queue.cursor() > index {
            self.queue.retreat();
        }
        self.queue.set_repeat(saved_repeat);
        self.is_playing = true;
        true
    }

    /// Toggles playback: pauses if playing, or starts if current track exists.
    pub fn toggle_play(&mut self) {
        if self.is_playing {
            self.is_playing = false;
        } else if self.queue.current().is_some() {
            self.is_playing = true;
        }
    }

    /// Advances queue cursor and starts playback of the next track.
    pub fn next_track(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        self.queue.advance();
        self.is_playing = true;
    }

    /// Retreats queue cursor and starts playback of the previous track.
    pub fn prev_track(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        self.queue.retreat();
        self.is_playing = true;
    }

    /// Sets playback volume level, clamping to 0..=100.
    pub fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
    }

    /// Relative seek placeholder for future audio adapter wiring.
    pub fn seek_by(&mut self, _delta_secs: i64) {
        // Sync boundary note: seeking delegates to AudioBackend in later tranches.
    }

    pub fn current_title(&self) -> &str {
        self.current().map(|t| t.title.as_str()).unwrap_or("No track playing")
    }

    pub fn current_artist(&self) -> &str {
        self.current().map(|t| t.artist.as_str()).unwrap_or("")
    }

    pub fn current_duration_text(&self) -> String {
        format_track_duration(self.current().and_then(|t| t.duration_secs))
    }

    pub fn queue_tracks(&self) -> Vec<TrackDisplayRow> {
        queue_track_rows(&self.queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue_with(ids: &[&str], repeat: RepeatMode) -> PlaybackQueue {
        let mut q = PlaybackQueue::new();
        for &id in ids {
            q.push(
                Track::new(id, &format!("Title {id}"), &format!("Artist {id}")).with_duration(180),
            );
        }
        q.set_repeat(repeat);
        q
    }

    #[test]
    fn test_playback_state_new_and_default() {
        let state = PlaybackState::new();
        assert!(state.queue.is_empty() && !state.is_playing && state.volume == 100);
        assert_eq!(state, PlaybackState::default());
        assert_eq!(state.current(), None);
        assert_eq!(state.current_title(), "No track playing");
        assert_eq!(state.current_artist(), "");
        assert_eq!(state.current_duration_text(), "");
    }

    #[test]
    fn test_repeat_mode_advance_retreat_wrap_and_no_wrap() {
        let mut off = PlaybackState::with_queue(queue_with(&["1", "2"], RepeatMode::Off));
        off.next_track();
        assert_eq!(off.current().unwrap().id, "2");
        off.next_track();
        assert_eq!(off.current().unwrap().id, "2"); // no-wrap tail
        off.prev_track();
        assert_eq!(off.current().unwrap().id, "1");
        off.prev_track();
        assert_eq!(off.current().unwrap().id, "1"); // no-wrap head

        let mut all = PlaybackState::with_queue(queue_with(&["1", "2"], RepeatMode::All));
        all.next_track();
        all.next_track();
        assert_eq!(all.current().unwrap().id, "1"); // wrap tail to head
        all.prev_track();
        assert_eq!(all.current().unwrap().id, "2"); // wrap head to tail

        let mut one = PlaybackState::with_queue(queue_with(&["1", "2"], RepeatMode::One));
        one.next_track();
        assert_eq!(one.current().unwrap().id, "1");
        one.prev_track();
        assert_eq!(one.current().unwrap().id, "1");
    }

    #[test]
    fn test_toggle_transitions() {
        let mut empty = PlaybackState::new();
        empty.toggle_play();
        assert!(!empty.is_playing);

        let mut state = PlaybackState::with_queue(queue_with(&["1"], RepeatMode::Off));
        for expected in [true, false, true] {
            state.toggle_play();
            assert_eq!(state.is_playing, expected);
        }
    }

    #[test]
    fn test_volume_clamp_edges() {
        let mut state = PlaybackState::new();
        for (input, expected) in [(0, 0), (50, 50), (100, 100), (101, 100), (u8::MAX, 100)] {
            state.set_volume(input);
            assert_eq!(state.volume, expected);
        }
    }

    #[test]
    fn test_current_after_select_and_next_on_empty_and_filled_queue() {
        let mut empty = PlaybackState::new();
        assert_eq!(empty.current(), None);
        assert!(!empty.select_track(0) && !empty.select_track(5));
        empty.next_track();
        empty.prev_track();
        assert_eq!(empty.current(), None);

        let mut state = PlaybackState::with_queue(queue_with(&["a", "b", "c"], RepeatMode::Off));
        assert_eq!(state.current().unwrap().id, "a");
        assert!(state.select_track(2) && state.is_playing);
        assert_eq!(state.current().unwrap().id, "c");
        assert!(state.select_track(0) && !state.select_track(99));
        assert_eq!(state.current().unwrap().id, "a");
        state.next_track();
        assert_eq!(state.current().unwrap().id, "b");
        state.prev_track();
        assert_eq!(state.current().unwrap().id, "a");
    }

    #[test]
    fn test_display_string_outputs() {
        for (s, exp) in [(0, "0:00"), (9, "0:09"), (65, "1:05"), (599, "9:59"), (3600, "60:00")] {
            assert_eq!(format_position(s), exp);
        }
        assert_eq!(format_track_duration(None), "");
        assert_eq!(format_track_duration(Some(0)), "0:00");
        assert_eq!(format_track_duration(Some(243)), "4:03");

        let state = PlaybackState::with_queue(queue_with(&["x", "y"], RepeatMode::Off));
        assert_eq!(state.current_title(), "Title x");
        assert_eq!(state.current_artist(), "Artist x");
        assert_eq!(state.current_duration_text(), "3:00");
        let rows = state.queue_tracks();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "Title x");
        assert_eq!(rows[0].duration, "3:00");
    }

    #[test]
    fn test_sync_boundary_seek_noop() {
        let mut state = PlaybackState::new();
        state.seek_by(10);
        state.seek_by(-10);
        assert_eq!(state.volume, 100);
    }
}
