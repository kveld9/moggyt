//! Playback queue persistence bridge.
//!
//! Hexagonal invariant: serializes the persistable queue snapshot through the
//! injected [`crate::infra::QueueStore`] port without direct I/O.

use crate::slint_shell::state::SlintAppState;

impl SlintAppState {
    /// Persists the active playback queue to storage, excluding autoplay tracks.
    pub fn persist_queue(&self) {
        let _ = self.queue_store.save(&self.queue.persistable());
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{RepeatMode, Track};
    use crate::slint_shell::ContextSurface;
    use crate::slint_shell::context_menu::{ContextAction, ContextTarget, apply_action};
    use crate::slint_shell::playback::cycle_repeat_mode;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn persist_queue_reflects_live_queue_in_store() {
        let mut state = test_wired();
        let track1 = Track::new("t1", "Track 1", "Artist 1");
        let track2 = Track::new("t2", "Track 2", "Artist 2");
        state.queue.push(track1.clone());
        state.queue.push(track2.clone());
        state.queue.set_cursor(1);

        state.persist_queue();

        let loaded = state.queue_store.load();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.cursor(), 1);
        assert_eq!(loaded.tracks(), &[track1, track2]);
    }

    #[test]
    fn persist_queue_excludes_autoplay_tracks() {
        let mut state = test_wired();
        let context_track = Track::new("ctx-1", "Context", "Artist");
        let autoplay_track = Track::new("auto-1", "Autoplay", "Artist");
        state.queue.push(context_track.clone());
        state.queue.push_autoplay(autoplay_track);

        state.persist_queue();

        let loaded = state.queue_store.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.tracks(), &[context_track]);
        assert_eq!(loaded.current().map(|t| t.id.as_str()), Some("ctx-1"));
    }

    #[test]
    fn context_menu_add_to_queue_persists_appended_track() {
        let mut state = test_wired();
        let track = Track::new("t1", "Track 1", "Artist");
        let target = ContextTarget { surface: ContextSurface::Queue, index: 0 };

        apply_action(&mut state, target, ContextAction::AddToQueue, track.clone());

        let stored = state.queue_store.load();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored.tracks(), &[track]);
    }

    #[test]
    fn context_menu_play_next_persists_inserted_track() {
        let mut state = test_wired();
        let t1 = Track::new("t1", "Track 1", "Artist");
        let t2 = Track::new("t2", "Track 2", "Artist");
        state.queue.push(t1.clone());
        state.persist_queue();

        let target = ContextTarget { surface: ContextSurface::Queue, index: 0 };
        apply_action(&mut state, target, ContextAction::PlayNext, t2.clone());

        let stored = state.queue_store.load();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored.tracks(), &[t1, t2]);
    }

    #[test]
    fn repeat_mode_cycle_persists_new_mode() {
        let mut state = test_wired();
        assert_eq!(state.queue_store.load().repeat(), RepeatMode::Off);

        let next = cycle_repeat_mode(state.queue.repeat());
        state.queue.set_repeat(next);
        state.persist_queue();

        assert_eq!(state.queue_store.load().repeat(), RepeatMode::All);
    }
}
