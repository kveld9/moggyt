//! Global keyboard shortcuts dispatching for [`AppState`].

use eframe::egui;

use crate::app::AppState;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Handles a global keyboard shortcut event.
    ///
    /// Returns true if the key triggered an action, false otherwise.
    pub fn apply_key(&mut self, key: egui::Key) -> bool {
        if !self.keyboard_shortcuts_enabled() {
            return false;
        }
        self.apply_playback_shortcut(key)
            || self.apply_volume_shortcut(key)
            || self.apply_queue_shortcut(key)
    }

    fn apply_playback_shortcut(&mut self, key: egui::Key) -> bool {
        match key {
            egui::Key::Space => {
                self.toggle_play();
                true
            }
            egui::Key::ArrowLeft => {
                self.seek_by(-10);
                true
            }
            egui::Key::ArrowRight => {
                self.seek_by(10);
                true
            }
            _ => false,
        }
    }

    fn apply_volume_shortcut(&mut self, key: egui::Key) -> bool {
        match key {
            egui::Key::ArrowUp => {
                let vol = self.audio.volume().saturating_add(5).min(100);
                self.set_volume(vol);
                true
            }
            egui::Key::ArrowDown => {
                let vol = self.audio.volume().saturating_sub(5);
                self.set_volume(vol);
                true
            }
            _ => false,
        }
    }

    fn apply_queue_shortcut(&mut self, key: egui::Key) -> bool {
        match key {
            egui::Key::N => self.step_next_shortcut(),
            egui::Key::P => self.step_prev_shortcut(),
            egui::Key::L => self.toggle_like_shortcut(),
            _ => false,
        }
    }

    fn step_next_shortcut(&mut self) -> bool {
        if self.queue.is_empty() || self.queue.current().is_none() {
            return false;
        }
        self.next_track();
        true
    }

    fn step_prev_shortcut(&mut self) -> bool {
        if self.queue.is_empty() || self.queue.current().is_none() {
            return false;
        }
        self.prev_track();
        true
    }

    fn toggle_like_shortcut(&mut self) -> bool {
        let Some(track) = self.queue.current() else {
            return false;
        };
        let id = track.id.clone();
        self.toggle_like(&id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::wired;
    use super::*;

    #[test]
    fn test_apply_key_handling_matrix() {
        let mut state = wired();
        state.search_text = "midnight".into();
        state.submit_search();
        state.search_text = "sunrise".into();
        state.submit_search();
        assert_eq!(state.queue.len(), 2);

        // Space toggles playback
        assert!(state.apply_key(egui::Key::Space));
        assert!(state.audio.is_playing());
        assert!(state.apply_key(egui::Key::Space));
        assert!(!state.audio.is_playing());

        // Seek forward and backward
        assert!(state.apply_key(egui::Key::ArrowRight));
        assert_eq!(state.audio.position_secs(), 10);
        assert!(state.apply_key(egui::Key::ArrowLeft));
        assert_eq!(state.audio.position_secs(), 0);

        // Volume adjustments
        state.set_volume(50);
        assert!(state.apply_key(egui::Key::ArrowUp));
        assert_eq!(state.audio.volume(), 55);
        assert!(state.apply_key(egui::Key::ArrowDown));
        assert_eq!(state.audio.volume(), 50);

        // Track progression and regression
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));
        assert!(state.apply_key(egui::Key::N));
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("sunrise"));
        assert!(state.apply_key(egui::Key::P));
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));

        // Toggle like status for current track
        let curr_id = state.queue.current().unwrap().id.clone();
        let initial_liked = state.is_liked(&curr_id);
        assert!(state.apply_key(egui::Key::L));
        assert_eq!(state.is_liked(&curr_id), !initial_liked);
        assert!(state.apply_key(egui::Key::L));
        assert_eq!(state.is_liked(&curr_id), initial_liked);

        // Unhandled keys return false
        assert!(!state.apply_key(egui::Key::A));
        assert!(!state.apply_key(egui::Key::Enter));
        assert!(!state.apply_key(egui::Key::Escape));
    }

    #[test]
    fn test_apply_key_when_shortcuts_disabled_returns_false_and_does_not_mutate() {
        let mut state = wired();
        state.search_text = "midnight".into();
        state.submit_search();
        state.set_shortcuts_enabled(false);

        let initial_pos = state.audio.position_secs();
        let initial_vol = state.audio.volume();
        let initial_playing = state.audio.is_playing();
        let initial_title = state.queue.current().map(|t| t.title.clone());

        for key in [
            egui::Key::Space,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::N,
            egui::Key::P,
            egui::Key::L,
            egui::Key::A,
        ] {
            assert!(!state.apply_key(key));
        }

        assert_eq!(state.audio.position_secs(), initial_pos);
        assert_eq!(state.audio.volume(), initial_vol);
        assert_eq!(state.audio.is_playing(), initial_playing);
        assert_eq!(state.queue.current().map(|t| t.title.clone()), initial_title);
    }

    #[test]
    fn test_apply_key_queue_dependent_keys_noop_on_empty_queue() {
        let mut state = wired();
        assert!(state.queue.is_empty());

        assert!(!state.apply_key(egui::Key::N));
        assert!(!state.apply_key(egui::Key::P));
        assert!(!state.apply_key(egui::Key::L));
        assert_eq!(state.audio.last_requested(), None);
    }

    #[test]
    fn test_apply_key_volume_clamping_at_edges() {
        let mut state = wired();

        // Upper boundary
        state.set_volume(98);
        assert!(state.apply_key(egui::Key::ArrowUp));
        assert_eq!(state.audio.volume(), 100);
        assert!(state.apply_key(egui::Key::ArrowUp));
        assert_eq!(state.audio.volume(), 100);

        // Lower boundary
        state.set_volume(2);
        assert!(state.apply_key(egui::Key::ArrowDown));
        assert_eq!(state.audio.volume(), 0);
        assert!(state.apply_key(egui::Key::ArrowDown));
        assert_eq!(state.audio.volume(), 0);
    }
}
