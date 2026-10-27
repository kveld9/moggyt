//! Sleep timer configuration and polling for [`AppState`].

use crate::app::AppState;
use crate::domain::TimerPreset;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Configures the active sleep timer preset, resetting accumulated timer elapsed.
    pub fn set_sleep_timer_preset(&mut self, preset: TimerPreset) {
        let current_track_id = self.queue.current().map(|t| t.id.as_str());
        self.sleep_timer.set_preset_with_track(preset, current_track_id);
        self.sleep_timer_accumulated = 0;
        self.sleep_timer_base_offset =
            self.started_at.map_or(0, |(inst, _)| inst.elapsed().as_secs());
        self.last_sleep_timer_fire = None;
    }

    /// Disarms the active sleep timer and clears timer bookkeeping.
    pub fn cancel_sleep_timer(&mut self) {
        self.sleep_timer.cancel();
        self.sleep_timer_accumulated = 0;
        self.sleep_timer_base_offset = 0;
        self.last_sleep_timer_fire = None;
    }

    /// Evaluates the active sleep timer against playback elapsed time.
    pub fn poll_sleep_timer(&mut self) {
        if !self.sleep_timer.is_armed() {
            return;
        }
        let track_elapsed = self.started_at.map_or(0, |(inst, _)| inst.elapsed().as_secs());
        let current = track_elapsed.saturating_sub(self.sleep_timer_base_offset);
        let total = self.sleep_timer_accumulated.saturating_add(current);
        self.poll_sleep_timer_with_elapsed(total);
    }

    pub(crate) fn poll_sleep_timer_with_elapsed(&mut self, elapsed_secs: u64) {
        if !self.sleep_timer.poll_elapsed(elapsed_secs) {
            return;
        }
        self.last_sleep_timer_fire = Some(elapsed_secs);
        self.audio.pause();
    }

    pub(crate) fn accumulate_sleep_timer_on_advance(&mut self) {
        let track_elapsed = self.started_at.map_or(0, |(inst, _)| inst.elapsed().as_secs());
        let current = track_elapsed.saturating_sub(self.sleep_timer_base_offset);
        self.sleep_timer_accumulated = self.sleep_timer_accumulated.saturating_add(current);
        self.sleep_timer_base_offset = 0;
    }

    pub(crate) fn notify_sleep_timer_track_changed(&mut self) {
        let Some(track) = self.queue.current() else {
            return;
        };
        let id = track.id.clone();
        if !self.sleep_timer.notify_track_changed(&id) {
            return;
        }
        self.last_sleep_timer_fire = Some(self.audio.position_secs());
        self.audio.pause();
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use super::*;
    use crate::domain::{TimerPreset, Track};

    #[test]
    fn sleep_timer_injected_elapsed_pauses_and_counts_down() {
        let mut state = wired();
        state.queue.push(Track::new("vid-1", "Karma Police", "Radiohead"));
        state.play_current_quiet();
        assert!(state.audio.is_playing());

        state.set_sleep_timer_preset(TimerPreset::Minutes15);
        assert!(state.sleep_timer.is_armed());
        assert_eq!(state.sleep_timer.remaining_secs(), Some(900));

        // Partway through interval, playback continues and remaining counts down
        state.poll_sleep_timer_with_elapsed(500);
        assert!(state.audio.is_playing());
        assert_eq!(state.sleep_timer.remaining_secs(), Some(400));

        // Just before expiry, still playing
        state.poll_sleep_timer_with_elapsed(899);
        assert!(state.audio.is_playing());
        assert_eq!(state.sleep_timer.remaining_secs(), Some(1));

        // Exactly at expiry, playback pauses and remaining reaches 0
        state.poll_sleep_timer_with_elapsed(900);
        assert!(!state.audio.is_playing());
        assert_eq!(state.sleep_timer.remaining_secs(), Some(0));
        assert_eq!(state.last_sleep_timer_fire, Some(900));

        // Subsequent poll remains silent and stays paused
        state.poll_sleep_timer_with_elapsed(901);
        assert!(!state.audio.is_playing());
    }

    #[test]
    fn sleep_timer_cancel_disarms_in_app_state() {
        let mut state = wired();
        state.set_sleep_timer_preset(TimerPreset::Minutes30);
        assert!(state.sleep_timer.is_armed());
        assert_eq!(state.sleep_timer.remaining_secs(), Some(1800));

        state.cancel_sleep_timer();
        assert!(!state.sleep_timer.is_armed());
        assert_eq!(state.sleep_timer.preset(), TimerPreset::Off);
        assert_eq!(state.sleep_timer.remaining_secs(), None);
    }

    #[test]
    fn sleep_timer_stop_after_track_pauses_on_queue_advance() {
        let mut state = wired();
        state.queue.push(Track::new("track-1", "Track 1", "Artist"));
        state.queue.push(Track::new("track-2", "Track 2", "Artist"));
        state.play_current_quiet();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("track-1"));

        state.set_sleep_timer_preset(TimerPreset::StopAfterTrack);
        assert!(state.sleep_timer.is_armed());

        // Queue advance moves to track-2, which fires StopAfterTrack and pauses audio
        state.next_track();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("track-2"));
        assert!(!state.audio.is_playing());
        assert!(!state.sleep_timer.is_armed());
    }
}
