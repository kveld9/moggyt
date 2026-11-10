//! App timer polling that pauses playback when the output device changes.

use crate::domain::PlaybackPhase;
use crate::infra::DeviceWatchAction;
use crate::slint_shell::state::SlintAppState;

impl SlintAppState {
    /// Polls the default output device and applies the debounced auto-pause decision.
    ///
    /// Sampling is paced to every `DEVICE_POLL_TICKS` app timer ticks.
    pub(crate) fn poll_output_device(&mut self) {
        if !self.device_poll_gate.should_sample() {
            return;
        }
        let current = crate::infra::current_output_device_identity();
        self.poll_output_device_with(current.as_deref());
    }

    /// Applies the debounced decision for an already sampled device name.
    pub(crate) fn poll_output_device_with(&mut self, current: Option<&str>) {
        let is_playing = self.audio.try_lock().map(|audio| audio.is_playing()).unwrap_or(false);
        let action = self.output_device_watch.step(current, is_playing);
        let DeviceWatchAction::Pause { previous, current } = action else {
            return;
        };
        tracing::info!(previous = %previous, current = %current, "audio output device changed; pausing playback");
        if let Ok(mut audio) = self.audio.lock() {
            audio.pause();
        }
        self.set_playback_phase(PlaybackPhase::Paused);
        self.sync_presence_playing(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn poll_output_device_with_pauses_on_second_tick_while_playing() {
        let mut state = test_wired();
        state.audio.lock().expect("audio").play("t1").expect("stub play");

        state.poll_output_device_with(Some("speakers"));
        assert!(state.audio.lock().expect("audio").is_playing());

        state.poll_output_device_with(Some("headphones"));
        assert!(state.audio.lock().expect("audio").is_playing());

        state.poll_output_device_with(Some("headphones"));
        assert_eq!(state.playback_phase, PlaybackPhase::Paused);
        assert!(!state.audio.lock().expect("audio").is_playing());
    }

    #[test]
    fn poll_output_device_with_is_idle_when_paused() {
        let mut state = test_wired();
        assert_eq!(state.playback_phase, PlaybackPhase::Empty);
        assert!(!state.audio.lock().expect("audio").is_playing());

        state.poll_output_device_with(Some("speakers"));
        state.poll_output_device_with(Some("headphones"));
        state.poll_output_device_with(Some("headphones"));

        assert_eq!(state.playback_phase, PlaybackPhase::Empty);
        assert!(!state.audio.lock().expect("audio").is_playing());
    }
}
