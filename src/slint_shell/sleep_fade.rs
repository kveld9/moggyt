//! Sleep timer polling and the volume fade applied before it fires.

use std::sync::Arc;

use crate::domain::{SLEEP_FADE_WINDOW_SECS, faded_volume};
use crate::infra::AudioBackend;
use crate::slint_shell::state::SlintAppState;

impl SlintAppState {
    /// Restores user volume if a progressive sleep fade-out was active, without persisting.
    pub fn restore_sleep_fade_volume(&mut self) {
        let Some(vol) = self.sleep_fade_base_volume.take() else {
            return;
        };
        if let Ok(mut audio) = self.audio.lock() {
            audio.set_volume(vol);
        }
    }

    /// Evaluates sleep timer duration, applies progressive volume fade-out, and pauses audio on expiration.
    pub fn poll_sleep_timer(&mut self) {
        let audio_arc = Arc::clone(&self.audio);
        let Ok(mut audio) = audio_arc.try_lock() else {
            return;
        };
        if !self.is_sleep_timer_active(audio.is_playing()) {
            return;
        }
        let elapsed = audio.position_secs();
        let effective = self
            .sleep_timer_accumulated
            .saturating_add(elapsed.saturating_sub(self.sleep_timer_base_offset));
        if self.sleep_timer.poll_elapsed(effective) {
            audio.pause();
            restore_captured_volume(&mut **audio, &mut self.sleep_fade_base_volume);
            drop(audio);
            self.sync_presence_playing(false);
            return;
        }
        apply_sleep_fade(
            &mut **audio,
            &mut self.sleep_fade_base_volume,
            self.sleep_timer.remaining_secs(),
        );
    }

    /// Handles sleep timer elapsed time accumulation and StopAfterTrack transition when a track changes.
    /// Returns `true` if the sleep timer expired and playback should halt instead of playing the new track.
    pub fn on_track_changing_sleep_timer(&mut self, next_track_id: &str) -> bool {
        if !self.sleep_timer.is_armed() {
            return false;
        }
        if let Ok(audio) = self.audio.lock() {
            let pos = audio.position_secs();
            let delta = pos.saturating_sub(self.sleep_timer_base_offset);
            self.sleep_timer_accumulated = self.sleep_timer_accumulated.saturating_add(delta);
            self.sleep_timer_base_offset = 0;
        }
        if self.sleep_timer.notify_track_changed(next_track_id) {
            if let Ok(mut audio) = self.audio.lock() {
                audio.pause();
                restore_captured_volume(&mut **audio, &mut self.sleep_fade_base_volume);
            }
            self.sync_presence_playing(false);
            return true;
        }
        false
    }

    fn is_sleep_timer_active(&self, is_playing: bool) -> bool {
        self.sleep_timer.is_armed() && is_playing
    }
}

fn restore_captured_volume(audio: &mut dyn AudioBackend, base: &mut Option<u8>) {
    if let Some(vol) = base.take() {
        audio.set_volume(vol);
    }
}

fn apply_sleep_fade(audio: &mut dyn AudioBackend, base: &mut Option<u8>, remaining: Option<u64>) {
    let Some(r) = remaining else {
        return;
    };
    if r >= SLEEP_FADE_WINDOW_SECS {
        return;
    }
    let base_vol = *base.get_or_insert_with(|| audio.volume());
    audio.set_volume(faded_volume(base_vol, r));
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::Track;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_sleep_timer_expiry() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.request_current_play();
        state.sleep_timer.set_preset(crate::domain::TimerPreset::Minutes15);
        assert!(state.sleep_timer.is_armed());

        state.seek_by(900);
        state.poll_sleep_timer();
        assert!(!state.audio.lock().unwrap().is_playing());
    }

    #[test]
    fn test_sleep_timer_fade_reduces_volume_inside_window() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.request_current_play();
        state.sleep_timer.set_preset(crate::domain::TimerPreset::Minutes15);
        assert!(state.sleep_timer.is_armed());

        state.seek_by(885);
        state.poll_sleep_timer();

        assert_eq!(state.audio.lock().unwrap().volume(), 50);
        assert_eq!(state.settings_store.load().volume, 100);
        assert_eq!(state.sleep_fade_base_volume, Some(100));
    }

    #[test]
    fn test_sleep_timer_fade_restores_volume_on_fire() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.request_current_play();
        state.sleep_timer.set_preset(crate::domain::TimerPreset::Minutes15);

        state.seek_by(885);
        state.poll_sleep_timer();
        assert_eq!(state.audio.lock().unwrap().volume(), 50);

        state.seek_by(15);
        state.poll_sleep_timer();

        assert!(!state.audio.lock().unwrap().is_playing());
        assert_eq!(state.audio.lock().unwrap().volume(), 100);
        assert_eq!(state.sleep_fade_base_volume, None);
    }

    #[test]
    fn test_sleep_timer_restore_on_cancel() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.request_current_play();
        state.sleep_timer.set_preset(crate::domain::TimerPreset::Minutes15);

        state.seek_by(885);
        state.poll_sleep_timer();
        assert_eq!(state.audio.lock().unwrap().volume(), 50);

        crate::slint_shell::settings_callbacks::handle_sleep_timer_selection(&mut state, 0);
        assert_eq!(state.audio.lock().unwrap().volume(), 100);
        assert_eq!(state.sleep_fade_base_volume, None);
    }

    #[test]
    fn test_sleep_timer_set_volume_during_fade_updates_base() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.request_current_play();
        state.sleep_timer.set_preset(crate::domain::TimerPreset::Minutes15);

        state.seek_by(885);
        state.poll_sleep_timer();
        assert_eq!(state.sleep_fade_base_volume, Some(100));

        state.set_volume(80);
        assert_eq!(state.sleep_fade_base_volume, Some(80));
        assert_eq!(state.settings_store.load().volume, 80);

        state.poll_sleep_timer();
        assert_eq!(state.audio.lock().unwrap().volume(), 40);

        state.seek_by(15);
        state.poll_sleep_timer();
        assert_eq!(state.audio.lock().unwrap().volume(), 80);
        assert_eq!(state.sleep_fade_base_volume, None);
    }

    #[test]
    fn test_restore_sleep_fade_volume_noop_when_not_fading() {
        let mut state = test_wired();
        state.audio.lock().unwrap().set_volume(70);
        state.restore_sleep_fade_volume();
        assert_eq!(state.audio.lock().unwrap().volume(), 70);
    }

    #[test]
    fn test_sleep_timer_stop_after_track_on_next_track_halts_playback() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));
        state.request_current_play();
        assert!(state.audio.lock().unwrap().is_playing());
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("t1"));

        state
            .sleep_timer
            .set_preset_with_track(crate::domain::TimerPreset::StopAfterTrack, Some("t1"));
        assert!(state.sleep_timer.is_armed());

        state.sleep_fade_base_volume = Some(100);
        state.audio.lock().unwrap().set_volume(40);

        state.next_track();

        assert!(!state.audio.lock().unwrap().is_playing());
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("t1"));
        assert_eq!(state.audio.lock().unwrap().volume(), 100);
        assert_eq!(state.sleep_fade_base_volume, None);
        assert!(!state.sleep_timer.is_armed());
    }

    #[test]
    fn test_sleep_timer_stop_after_track_on_play_track_at_index_halts_playback() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));
        state.request_current_play();
        assert!(state.audio.lock().unwrap().is_playing());

        state
            .sleep_timer
            .set_preset_with_track(crate::domain::TimerPreset::StopAfterTrack, Some("t1"));
        assert!(state.sleep_timer.is_armed());

        state.sleep_fade_base_volume = Some(90);
        state.audio.lock().unwrap().set_volume(30);

        state.play_track_at_index(1);

        assert!(!state.audio.lock().unwrap().is_playing());
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("t1"));
        assert_eq!(state.audio.lock().unwrap().volume(), 90);
        assert_eq!(state.sleep_fade_base_volume, None);
        assert!(!state.sleep_timer.is_armed());
    }

    #[test]
    fn test_sleep_timer_multi_track_accumulation() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(1000));
        state.request_current_play();

        state.sleep_timer.set_preset(crate::domain::TimerPreset::Minutes15);
        state.sleep_timer_accumulated = 0;
        state.sleep_timer_base_offset = 0;

        state.seek_by(100);
        assert_eq!(state.audio.lock().unwrap().position_secs(), 100);

        state.next_track();

        assert_eq!(state.sleep_timer_accumulated, 100);
        assert_eq!(state.sleep_timer_base_offset, 0);
        assert!(state.audio.lock().unwrap().is_playing());
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("t2"));
        assert_eq!(state.audio.lock().unwrap().position_secs(), 0);

        state.seek_by(800);
        assert_eq!(state.audio.lock().unwrap().position_secs(), 800);

        state.poll_sleep_timer();
        assert!(!state.audio.lock().unwrap().is_playing());
        assert!(!state.sleep_timer.is_armed());
    }
}
