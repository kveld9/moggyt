//! Level-meter projection from the audio backend into the Slint window.
//!
//! Hexagonal invariant: non-blocking backend read; the window layer only mirrors the history.

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::PlaybackPhase;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::{MeterUi, MoggytWindow};

/// Samples the backend level, appends it to the history, and mirrors it into the window.
///
/// A track that is not audibly playing contributes `0.0`, so a paused or resolved track never
/// leaves a stale loudness reading on screen.
pub fn push_level(window: &MoggytWindow, state: &mut SlintAppState) {
    let measured = state.audio.try_lock().ok().and_then(|audio| audio.output_level());
    let level =
        if state.playback_phase == PlaybackPhase::Playing { measured.unwrap_or(0.0) } else { 0.0 };
    state.level_history.push(level);
    let samples: Vec<f32> = state.level_history.samples().collect();
    window.global::<MeterUi>().set_levels(ModelRc::new(VecModel::from(samples)));
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use slint::Model;

    use super::*;
    use crate::infra::AudioStub;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn push_level_playing_with_measured_level() {
        let Ok(window) = MoggytWindow::new() else {
            return;
        };
        let mut state = test_wired();
        let mut stub = AudioStub::new();
        stub.set_output_level(Some(0.4));
        state.audio = Arc::new(Mutex::new(Box::new(stub)));
        state.playback_phase = PlaybackPhase::Playing;

        push_level(&window, &mut state);

        assert_eq!(state.level_history.latest(), 0.4);
        let levels = window.global::<MeterUi>().get_levels();
        assert_eq!(levels.row_count(), 1);
        assert_eq!(levels.row_data(0), Some(0.4));
    }

    #[test]
    fn push_level_paused_pushes_zero() {
        let Ok(window) = MoggytWindow::new() else {
            return;
        };
        let mut state = test_wired();
        let mut stub = AudioStub::new();
        stub.set_output_level(Some(0.4));
        state.audio = Arc::new(Mutex::new(Box::new(stub)));
        state.playback_phase = PlaybackPhase::Paused;

        push_level(&window, &mut state);

        assert_eq!(state.level_history.latest(), 0.0);
        let levels = window.global::<MeterUi>().get_levels();
        assert_eq!(levels.row_count(), 1);
        assert_eq!(levels.row_data(0), Some(0.0));
    }

    #[test]
    fn push_level_none_backend_pushes_zero() {
        let Ok(window) = MoggytWindow::new() else {
            return;
        };
        let mut state = test_wired();
        let mut stub = AudioStub::new();
        stub.set_output_level(None);
        state.audio = Arc::new(Mutex::new(Box::new(stub)));
        state.playback_phase = PlaybackPhase::Playing;

        push_level(&window, &mut state);

        assert_eq!(state.level_history.latest(), 0.0);
        let levels = window.global::<MeterUi>().get_levels();
        assert_eq!(levels.row_count(), 1);
        assert_eq!(levels.row_data(0), Some(0.0));
    }
}
