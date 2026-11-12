//! Liked-track toggling with settings persistence.
//!
//! Hexagonal invariant: the module mutates an in-memory set and delegates persistence to the
//! injected settings store; it performs no direct file access.

use std::collections::HashSet;

use crate::infra::Settings;
use crate::slint_shell::playback::toggle_like;
use crate::slint_shell::state::SlintAppState;

/// Builds the in-memory liked set from persisted settings.
pub(crate) fn load_liked_ids(settings: &Settings) -> HashSet<String> {
    settings.liked_ids.iter().cloned().collect()
}

/// Toggles the like flag for `track_id`, persists the set, and returns the new state.
pub(crate) fn toggle_and_persist(state: &mut SlintAppState, track_id: &str) -> bool {
    let liked = toggle_like(&mut state.liked_ids, track_id);
    let mut settings = state.settings_store.load();
    settings.liked_ids = sorted_liked_ids(state);
    let _ = state.settings_store.save(&settings);
    liked
}

/// Returns the liked identifiers in a deterministic order for persistence.
fn sorted_liked_ids(state: &SlintAppState) -> Vec<String> {
    let mut ids: Vec<String> = state.liked_ids.iter().cloned().collect();
    ids.sort_unstable();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn toggle_and_persist_writes_the_liked_id() {
        let mut state = test_wired();
        assert!(toggle_and_persist(&mut state, "t1"));
        assert_eq!(state.settings_store.load().liked_ids, vec!["t1"]);
    }

    #[test]
    fn toggle_and_persist_removes_the_liked_id() {
        let mut state = test_wired();
        assert!(toggle_and_persist(&mut state, "t1"));
        assert!(!toggle_and_persist(&mut state, "t1"));
        assert!(state.settings_store.load().liked_ids.is_empty());
    }

    #[test]
    fn persisted_liked_ids_are_sorted() {
        let mut state = test_wired();
        assert!(toggle_and_persist(&mut state, "b"));
        assert!(toggle_and_persist(&mut state, "a"));
        assert_eq!(state.settings_store.load().liked_ids, vec!["a", "b"]);
    }

    #[test]
    fn load_liked_ids_deduplicates_and_accepts_empty_settings() {
        let mut settings = Settings::default();
        assert!(load_liked_ids(&settings).is_empty());

        settings.liked_ids = vec!["a".into(), "a".into(), "b".into()];
        let loaded = load_liked_ids(&settings);
        assert_eq!(loaded.len(), 2);
        assert!(loaded.contains("a"));
        assert!(loaded.contains("b"));
    }
}
