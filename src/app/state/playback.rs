//! Playback control and auto-skip execution for [`AppState`].

use crate::app::state::current_unix_secs;
use crate::app::{AppState, Page};
use crate::domain::Track;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore, clamp_speed};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    pub(crate) fn stop_playback(&mut self) {
        if self.audio.is_playing() {
            self.audio.pause();
        }
    }

    /// Starts playback of the track at the head of the queue.
    pub fn play_head(&mut self) {
        if self.queue.current().is_some() {
            self.play_current_quiet();
            self.page = Page::NowPlaying;
        }
    }

    /// Seeds an autoplay radio session with `seed`, clears queue, and starts playback.
    /// Blank seed identifiers are ignored.
    pub fn start_radio(&mut self, seed: Track) {
        let seed_id = seed.id.trim();
        if seed_id.is_empty() {
            return;
        }
        let radio_tracks = self.catalog.radio(seed_id);
        let seed_key = seed.id.clone();
        self.mutate_queue(|q| {
            q.clear();
            q.push(seed);
            for track in radio_tracks {
                let trimmed = track.id.trim();
                if !trimmed.is_empty() && trimmed != seed_key {
                    q.push(track);
                }
            }
        });
        self.autoplay = true;
        self.play_current_quiet();
        self.page = Page::NowPlaying;
    }

    /// Evaluates autoplay queue continuation when the active track ends.
    pub fn continue_autoplay_if_ended(&mut self) {
        if !self.autoplay {
            return;
        }
        if self.queue.is_empty() {
            return;
        }
        let (Some(current), Some(last)) = (self.queue.current(), self.queue.tracks().last()) else {
            return;
        };
        if !std::ptr::eq(current, last) {
            return;
        }
        if self.audio.is_playing() {
            return;
        }
        if self.audio.last_requested().is_none() {
            return;
        }
        let Some(duration) = current.duration_secs else {
            return;
        };
        if self.audio.position_secs() < duration as u64 {
            return;
        }
        let current_id = current.id.clone();
        self.append_autoplay_and_advance(&current_id);
    }

    fn append_autoplay_and_advance(&mut self, current_id: &str) {
        let recommendations = self.catalog.radio(current_id);
        let mut appended = false;
        self.mutate_queue(|q| {
            for track in recommendations {
                let trimmed = track.id.trim();
                if trimmed.is_empty() || q.tracks().iter().any(|t| t.id == trimmed) {
                    continue;
                }
                q.push(track);
                appended = true;
            }
            if appended {
                q.advance();
            }
        });
        if appended {
            self.accumulate_sleep_timer_on_advance();
            self.play_current_quiet();
            self.notify_sleep_timer_track_changed();
        }
    }

    /// Plays the track currently under the queue cursor without modifying active page.
    pub fn play_current_quiet(&mut self) {
        let Some(track) = self.queue.current() else {
            return;
        };
        let id = track.id.clone();
        let artist = track.artist.clone();
        let title = track.title.clone();
        self.audio.play(&id);
        self.started_at = Some((std::time::Instant::now(), current_unix_secs()));
        self.forward_now_playing(&artist, &title);
        self.last_skip_target = None;
    }

    /// Evaluates per-frame auto-skip, advancing audio position when strictly inside a range.
    pub fn check_auto_skip(&mut self) {
        let Some(track) = self.queue.current() else {
            self.last_skip_target = None;
            return;
        };
        let current_pos = u32::try_from(self.audio.position_secs()).unwrap_or(u32::MAX);
        if self
            .last_skip_target
            .as_ref()
            .is_some_and(|(id, t)| id != &track.id || current_pos >= *t)
        {
            self.last_skip_target = None;
        }
        let Some(ranges) = self.skips.get(&track.id) else {
            return;
        };
        let Some(target) = ranges.jump_target(current_pos) else {
            return;
        };
        let already_skipped =
            self.last_skip_target.as_ref().is_some_and(|(id, t)| id == &track.id && *t == target);
        if target > current_pos && !already_skipped {
            self.last_skip_target = Some((track.id.clone(), target));
            self.audio.seek(target as u64);
        }
    }

    /// Toggles playback: pauses if currently playing, resumes if previously requested,
    /// or starts playback of current head quietly.
    pub fn toggle_play(&mut self) {
        if self.audio.is_playing() {
            self.audio.pause();
        } else if self.audio.last_requested().is_some() {
            self.audio.resume_play();
        } else {
            self.play_current_quiet();
        }
    }

    /// Advances the queue cursor and deterministically starts playback of the new head.
    pub fn next_track(&mut self) {
        self.maybe_scrobble_current();
        self.accumulate_sleep_timer_on_advance();
        self.mutate_queue(|q| q.advance());
        self.play_current_quiet();
        self.notify_sleep_timer_track_changed();
    }

    /// Retreats the queue cursor and deterministically starts playback of the new head.
    pub fn prev_track(&mut self) {
        self.maybe_scrobble_current();
        self.accumulate_sleep_timer_on_advance();
        self.mutate_queue(|q| q.retreat());
        self.play_current_quiet();
        self.notify_sleep_timer_track_changed();
    }

    /// Adjusts playback position by `delta_secs` relative to current position, clamped at zero.
    pub fn seek_by(&mut self, delta_secs: i64) {
        let current = self.audio.position_secs() as i64;
        let target = current.saturating_add(delta_secs).max(0) as u64;
        self.audio.seek(target);
    }

    /// Sets playback volume level (0..=100, clamped by audio adapter) and persists the change.
    pub fn set_volume(&mut self, volume: u8) {
        self.audio.set_volume(volume);
        self.persist_settings();
    }

    /// Sets playback speed (0.5..=2.0, clamped by audio adapter) and persists the change.
    pub fn set_playback_speed(&mut self, speed: f32) {
        let clamped = clamp_speed(speed);
        self.audio.set_speed(clamped);
        self.persist_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use super::*;
    use crate::domain::Track;
    use crate::infra::{MemorySettingsStore, SettingsStore};

    #[test]
    fn play_head_navigates_to_now_playing() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.play_head();
        assert_eq!(state.page, Page::NowPlaying);
    }

    #[test]
    fn toggle_on_empty_queue_does_nothing() {
        let mut state = wired();
        state.toggle_play();
        assert!(!state.audio.is_playing());
        assert_eq!(state.audio.last_requested(), None);
    }

    #[test]
    fn search_then_toggle_plays_head() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.toggle_play();
        assert!(state.audio.is_playing());
        assert_eq!(state.audio.last_requested(), Some("stub-1"));
        assert_eq!(state.page, Page::Queue);
    }

    #[test]
    fn next_and_prev_move_current_and_play() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.search_text = "sunrise".to_owned();
        state.submit_search();
        assert_eq!(state.queue.len(), 2);

        state.next_track();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("sunrise"));

        state.prev_track();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));
    }

    #[test]
    fn seek_by_clamps_at_zero_and_advances() {
        let mut state = wired();
        state.seek_by(-10);
        assert_eq!(state.audio.position_secs(), 0);

        state.seek_by(15);
        assert_eq!(state.audio.position_secs(), 15);

        state.seek_by(-5);
        assert_eq!(state.audio.position_secs(), 10);

        state.seek_by(-20);
        assert_eq!(state.audio.position_secs(), 0);
    }

    #[test]
    fn set_volume_clamps_via_stub() {
        let mut state = wired();
        state.set_volume(50);
        assert_eq!(state.audio.volume(), 50);

        state.set_volume(150);
        assert_eq!(state.audio.volume(), 100);
    }

    #[test]
    fn set_volume_persists_to_settings_store_with_clamping() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        state.set_volume(42);
        assert_eq!(state.audio.volume(), 42);
        assert_eq!(store.load().volume, 42);

        state.set_volume(150);
        assert_eq!(state.audio.volume(), 100);
        assert_eq!(store.load().volume, 100);
    }

    #[test]
    fn start_radio_seeds_plays_and_navigates() {
        let mut state = wired();
        let seed = Track::new("seed-1", "Seed Song", "Seed Artist").with_duration(180);
        state.start_radio(seed);
        assert_eq!(state.queue.len(), 3);
        assert_eq!(state.queue.tracks()[0].id, "seed-1");
        assert_eq!(state.queue.tracks()[1].id, "seed-1-radio-1");
        assert_eq!(state.queue.tracks()[2].id, "seed-1-radio-2");
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("seed-1"));
        assert!(state.autoplay && state.page == Page::NowPlaying && state.audio.is_playing());
        assert_eq!(state.audio.last_requested(), Some("seed-1"));
    }

    #[test]
    fn start_radio_blank_id_is_noop() {
        let mut state = wired();
        state.queue.push(Track::new("kept", "Kept Song", "Artist"));
        state.start_radio(Track::new("", "Empty", "Artist"));
        assert_eq!(state.queue.len(), 1);
        assert!(!state.autoplay && state.page == Page::Search);
        state.start_radio(Track::new("   ", "Spaces", "Artist"));
        assert_eq!(state.queue.len(), 1);
        assert!(!state.autoplay && state.page == Page::Search);
    }

    #[test]
    fn continue_autoplay_if_ended_appends_and_advances_at_end() {
        let mut state = wired();
        state.start_radio(Track::new("seed-1", "Seed", "Artist").with_duration(180));
        state.next_track();
        state.next_track();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("seed-1-radio-2"));

        state.audio.seek(210);
        state.audio.pause();
        state.continue_autoplay_if_ended();

        assert_eq!(state.queue.len(), 5);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("seed-1-radio-2-radio-1"));
        assert!(state.audio.is_playing());
        assert_eq!(state.audio.last_requested(), Some("seed-1-radio-2-radio-1"));
    }

    #[test]
    fn continue_autoplay_if_ended_noops() {
        let mut state = wired();
        state.start_radio(Track::new("seed-1", "Seed", "Artist").with_duration(180));

        // Mid-queue: not final track
        state.audio.seek(180);
        state.audio.pause();
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 3);

        // Move to last track
        state.next_track();
        state.next_track();

        // While playing
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 3);

        // Paused mid-track
        state.audio.seek(100);
        state.audio.pause();
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 3);

        // Autoplay off
        state.audio.seek(210);
        state.autoplay = false;
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 3);

        // Unknown duration
        state.autoplay = true;
        state.queue.push(Track::new("unknown-dur", "Unknown", "Artist"));
        state.next_track();
        state.audio.seek(500);
        state.audio.pause();
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 4);
    }

    #[test]
    fn continue_autoplay_deduplicates_and_skips_existing_tracks() {
        let mut state = wired();
        state.start_radio(Track::new("seed-1", "Seed", "Artist").with_duration(180));
        state.next_track();
        state.next_track();
        state.queue.push(Track::new("seed-1-radio-2-radio-1", "R1", "Stub Artist"));
        state.queue.push(Track::new("seed-1-radio-2-radio-2", "R2", "Stub Artist"));
        state.next_track();
        state.next_track();
        let len_before = state.queue.len();

        state.audio.seek(210);
        state.audio.pause();
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), len_before);
    }

    #[test]
    fn test_auto_skip_advances_past_marked_intro_and_does_not_loop() {
        let mut state = wired();
        let track = Track::new("intro-vid", "Intro Song", "Artist");
        state.queue.push(track);
        state.play_head();

        state.mark_segment_start_at(0);
        assert!(state.mark_segment_end_at(15));
        assert_eq!(state.audio.position_secs(), 0);

        // First frame: inside 0..15 range -> auto-skip seeks to 15
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 15);

        // Second frame: at boundary (15) -> does not seek again
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 15);

        // Advance playback past range -> does not seek
        state.audio.seek(25);
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 25);
    }

    #[test]
    fn test_auto_skip_does_not_seek_backwards_or_outside_ranges() {
        let mut state = wired();
        let track = Track::new("seg-vid", "Segment Song", "Artist");
        state.queue.push(track);
        state.play_head();

        state.mark_segment_start_at(20);
        assert!(state.mark_segment_end_at(30));

        // Position 10 is outside -> no jump
        state.audio.seek(10);
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 10);

        // Position 20 is at start -> jumps to 30
        state.audio.seek(20);
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 30);

        // Position 30 is at end -> no jump
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 30);
    }

    #[test]
    fn test_set_playback_speed_forwards_clamped_and_persists() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        assert_eq!(state.audio.speed(), 1.0);

        // In-range preset
        state.set_playback_speed(1.25);
        assert_eq!(state.audio.speed(), 1.25);
        assert_eq!(store.load().playback_speed, 1.25);

        // Clamp above max 2.0
        state.set_playback_speed(5.0);
        assert_eq!(state.audio.speed(), 2.0);
        assert_eq!(store.load().playback_speed, 2.0);

        // Clamp below min 0.5
        state.set_playback_speed(0.1);
        assert_eq!(state.audio.speed(), 0.5);
        assert_eq!(store.load().playback_speed, 0.5);

        // Non-finite falls back to 1.0
        state.set_playback_speed(f32::NAN);
        assert_eq!(state.audio.speed(), 1.0);
        assert_eq!(store.load().playback_speed, 1.0);
    }
}
