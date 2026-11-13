//! Log-capture tests for the playback phase transition trace.

use std::sync::{Arc, Mutex};

use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

use crate::domain::PlaybackPhase;
use crate::slint_shell::test_support::test_wired;

/// Records the level of every emitted tracing event.
#[derive(Clone, Default)]
struct RecordingSubscriber {
    levels: Arc<Mutex<Vec<Level>>>,
}

impl Subscriber for RecordingSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        self.levels.lock().expect("event recorder lock").push(*event.metadata().level());
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[test]
fn phase_change_emits_one_info_event_and_repeats_stay_silent() {
    let recorder = RecordingSubscriber::default();
    let levels = Arc::clone(&recorder.levels);
    let mut state = test_wired();

    tracing::subscriber::with_default(recorder, || {
        state.set_playback_phase(PlaybackPhase::Playing);
        state.set_playback_phase(PlaybackPhase::Playing);
        state.set_playback_phase(PlaybackPhase::Paused);
    });

    assert_eq!(*levels.lock().expect("event recorder lock"), vec![Level::INFO, Level::INFO]);
}
