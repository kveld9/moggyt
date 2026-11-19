//! Buffering flag projection from the playback phase and the position watchdog.
//!
//! Hexagonal invariant: pure in-memory projection; the window layer only mirrors it.

use slint::ComponentHandle;

use crate::domain::{BufferingWatch, PlaybackPhase};
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::state::SlintAppState;

/// Returns whether the player is loading or stalled and must render the spinner.
pub fn is_buffering(phase: PlaybackPhase, watch: BufferingWatch) -> bool {
    phase == PlaybackPhase::Resolving || (phase == PlaybackPhase::Playing && watch.is_buffering())
}

/// Advances the watchdog with a new audible position sample.
///
/// Only a playing track feeds the watchdog: pausing freezes the position by design and must
/// never look like buffering.
pub fn observe_buffering(state: &mut SlintAppState, position_secs: u64) {
    state.buffering_watch = match state.playback_phase {
        PlaybackPhase::Playing => state.buffering_watch.observe(position_secs),
        _ => state.buffering_watch.reset(),
    };
}

/// Mirrors the buffering flag into the Slint player global.
pub fn sync_buffering(window: &MoggytWindow, state: &SlintAppState) {
    let flag = is_buffering(state.playback_phase, state.buffering_watch);
    window.global::<crate::slint_shell::PlayerUi>().set_is_buffering(flag);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::BUFFERING_STALL_SAMPLES;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_is_buffering_resolving_with_any_watchdog() {
        assert!(is_buffering(PlaybackPhase::Resolving, BufferingWatch::Flowing));
        let stalled_initial = BufferingWatch::Stalled { position_secs: 5, stalled_samples: 1 };
        assert!(is_buffering(PlaybackPhase::Resolving, stalled_initial));
        let stalled_max = BufferingWatch::Stalled { position_secs: 5, stalled_samples: 10 };
        assert!(is_buffering(PlaybackPhase::Resolving, stalled_max));
    }

    #[test]
    fn test_is_buffering_playing_with_fresh_watchdog() {
        assert!(!is_buffering(PlaybackPhase::Playing, BufferingWatch::Flowing));
        let fresh_stall = BufferingWatch::Flowing.observe(10);
        assert!(!is_buffering(PlaybackPhase::Playing, fresh_stall));
    }

    #[test]
    fn test_is_buffering_playing_after_four_frozen_samples() {
        let mut watch = BufferingWatch::Flowing;
        for _ in 0..BUFFERING_STALL_SAMPLES {
            watch = watch.observe(15);
        }
        assert!(is_buffering(PlaybackPhase::Playing, watch));
    }

    #[test]
    fn test_is_buffering_paused_with_stalled_watchdog() {
        let mut watch = BufferingWatch::Flowing;
        for _ in 0..BUFFERING_STALL_SAMPLES {
            watch = watch.observe(20);
        }
        assert!(watch.is_buffering());
        assert!(!is_buffering(PlaybackPhase::Paused, watch));
        assert!(!is_buffering(PlaybackPhase::Empty, watch));
    }

    #[test]
    fn test_observe_buffering_playing_advances_and_paused_resets() {
        let mut state = test_wired();
        state.playback_phase = PlaybackPhase::Playing;
        observe_buffering(&mut state, 10);
        assert_eq!(
            state.buffering_watch,
            BufferingWatch::Stalled { position_secs: 10, stalled_samples: 1 }
        );

        for _ in 1..BUFFERING_STALL_SAMPLES {
            observe_buffering(&mut state, 10);
        }
        assert!(state.buffering_watch.is_buffering());

        state.playback_phase = PlaybackPhase::Paused;
        observe_buffering(&mut state, 10);
        assert_eq!(state.buffering_watch, BufferingWatch::Flowing);
        assert!(!state.buffering_watch.is_buffering());
    }
}
