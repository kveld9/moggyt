//! Preservation of the last observed audio backend state.
//!
//! Hexagonal invariant: avoids UI flicker by caching the last successfully observed
//! backend state instead of fabricating literal values when the backend lock is held.

use slint::ComponentHandle;

use crate::infra::AudioQuality;
use crate::slint_shell::format::{audio_quality_to_index, format_playback_speed};
use crate::slint_shell::state::{SharedAudio, SlintAppState};
use crate::slint_shell::{MoggytWindow, PlayerUi};

/// Last successfully observed audio backend state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioSnapshot {
    pub is_playing: bool,
    pub volume: u8,
    pub speed: f32,
    pub quality: AudioQuality,
}

impl Default for AudioSnapshot {
    /// First-paint defaults, used until the backend is observed once.
    fn default() -> Self {
        Self { is_playing: false, volume: 100, speed: 1.0, quality: AudioQuality::default() }
    }
}

impl AudioSnapshot {
    /// Samples the backend without blocking; `None` while the audio thread holds the lock.
    pub fn try_observe(audio: &SharedAudio) -> Option<Self> {
        let backend = audio.try_lock().ok()?;
        Some(Self {
            is_playing: backend.is_playing(),
            volume: backend.volume(),
            speed: backend.speed(),
            quality: backend.quality(),
        })
    }

    /// Keeps the previous snapshot when the backend could not be observed.
    pub fn merge(previous: Self, observed: Option<Self>) -> Self {
        observed.unwrap_or(previous)
    }
}

/// Pushes the audio state into the window without ever painting a fabricated value.
pub(crate) fn sync_audio_snapshot(window: &MoggytWindow, state: &mut SlintAppState) {
    state.audio_snapshot =
        AudioSnapshot::merge(state.audio_snapshot, AudioSnapshot::try_observe(&state.audio));
    let snapshot = state.audio_snapshot;
    let player = window.global::<PlayerUi>();
    player.set_is_playing(snapshot.is_playing);
    player.set_volume(i32::from(snapshot.volume));
    window.set_playback_speed_text(format_playback_speed(snapshot.speed).into());
    window.set_active_audio_quality_index(audio_quality_to_index(snapshot.quality));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_previous_when_backend_is_busy() {
        let previous =
            AudioSnapshot { is_playing: true, volume: 42, speed: 1.5, quality: AudioQuality::High };
        let merged = AudioSnapshot::merge(previous, None);
        assert_eq!(merged, previous);
    }

    #[test]
    fn merge_prefers_observed_values() {
        let previous = AudioSnapshot::default();
        let observed = AudioSnapshot {
            is_playing: true,
            volume: 75,
            speed: 1.25,
            quality: AudioQuality::Compact,
        };
        let merged = AudioSnapshot::merge(previous, Some(observed));
        assert_eq!(merged, observed);
    }

    #[test]
    fn try_observe_returns_none_while_locked() {
        let state = crate::slint_shell::test_support::test_wired();
        let guard = state.audio.lock().expect("lock audio");
        assert_eq!(AudioSnapshot::try_observe(&state.audio), None);
        drop(guard);

        let observed = AudioSnapshot::try_observe(&state.audio).expect("observe audio");
        assert_eq!(observed.volume, 100);
        assert!((observed.speed - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn try_observe_reports_stub_state() {
        let state = crate::slint_shell::test_support::test_wired();
        {
            let mut audio = state.audio.lock().expect("lock audio");
            audio.set_volume(42);
            audio.set_speed(1.5);
            audio.set_quality(AudioQuality::High);
        }
        let observed = AudioSnapshot::try_observe(&state.audio).expect("observe stub state");
        assert!(!observed.is_playing);
        assert_eq!(observed.volume, 42);
        assert!((observed.speed - 1.5).abs() < f32::EPSILON);
        assert_eq!(observed.quality, AudioQuality::High);
    }
}
