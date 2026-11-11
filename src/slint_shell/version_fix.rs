//! Transient picker state and callback wiring for audio version overrides.
//!
//! Hexagonal invariant: pure in-memory UI interaction logic delegating persistence
//! to `VersionOverrideStore` and catalog queries to the shared `Catalog` port.

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::Track;
use crate::i18n::{Locale, tr};
use crate::infra::{Catalog, CatalogError};
use crate::slint_shell::platform::dispatch_to_ui;
use crate::slint_shell::search::format_duration;
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::sync::sync_window;
use crate::slint_shell::{MoggytWindow, TrackRowData, VersionFixUi};

/// Transient state for the audio version picker.
#[derive(Debug, Default)]
pub struct VersionFixState {
    /// Catalog alternatives currently displayed, indexed by the UI row index.
    pub results: Vec<crate::domain::Track>,
    /// Monotonic generation guarding async search results.
    pub generation: u64,
}

/// Applies the chosen alternative recording at `index` to the active track.
pub fn apply_choice(state: &mut SlintAppState, index: usize) -> bool {
    let original_id = match state.queue.current() {
        Some(track) => track.id.clone(),
        None => return false,
    };
    let replacement_id = match state.version_fix.results.get(index) {
        Some(track) => track.id.clone(),
        None => return false,
    };
    if original_id == replacement_id {
        return false;
    }
    if state.version_overrides.set(&original_id, &replacement_id).is_err() {
        return false;
    }
    if let Err(err) = state.version_store.save(&state.version_overrides) {
        tracing::warn!(error = %err, "failed to persist version overrides");
    }
    state.version_fix.results.clear();
    true
}

fn track_to_row(index: usize, track: &Track) -> TrackRowData {
    TrackRowData {
        index: index as i32,
        title: track.title.clone().into(),
        artist: track.artist.clone().into(),
        album: track.album.clone().unwrap_or_default().into(),
        duration: format_duration(track.duration_secs).into(),
        kind: "track".into(),
        cover: slint::Image::default(),
    }
}

fn handle_dismiss(weak: &slint::Weak<MoggytWindow>) {
    if let Some(win) = weak.upgrade() {
        let ui = win.global::<VersionFixUi>();
        ui.set_active(false);
        ui.set_status_text("".into());
    }
}

fn handle_search_dispatch(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    generation: u64,
    locale: Locale,
    result: Result<Vec<Track>, crate::infra::CatalogError>,
) {
    if st.version_fix.generation != generation {
        return;
    }
    let ui = win.global::<VersionFixUi>();
    match result {
        Ok(tracks) => {
            let status = if tracks.is_empty() { tr(locale, "No alternatives found") } else { "" };
            ui.set_status_text(status.into());
            let rows: Vec<TrackRowData> =
                tracks.iter().enumerate().map(|(i, t)| track_to_row(i, t)).collect();
            st.version_fix.results = tracks;
            ui.set_results(ModelRc::new(VecModel::from(rows)));
        }
        Err(_) => {
            ui.set_status_text(tr(locale, "Failed to load search results").into());
        }
    }
}

fn handle_search_request(weak: &slint::Weak<MoggytWindow>, state_rc: &SharedState, query: &str) {
    let trimmed = query.trim().to_string();
    if trimmed.is_empty() {
        return;
    }
    let (generation, catalog, locale) = {
        let mut st = state_rc.borrow_mut();
        st.version_fix.generation = st.version_fix.generation.wrapping_add(1);
        (st.version_fix.generation, st.catalog.clone(), st.locale)
    };
    if let Some(win) = weak.upgrade() {
        win.global::<VersionFixUi>().set_status_text(tr(locale, "Searching...").into());
    }
    let weak_bg = weak.clone();
    std::thread::spawn(move || {
        let res =
            catalog.lock().map_err(|_| CatalogError::Transport).and_then(|c| c.search(&trimmed));
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak_bg.upgrade() else { return };
            let _ = crate::slint_shell::window::with_ui_state(|st| {
                handle_search_dispatch(&win, st, generation, locale, res);
            });
        });
    });
}

fn handle_pick_request(weak: &slint::Weak<MoggytWindow>, state_rc: &SharedState, index: i32) {
    if index < 0 {
        return;
    }
    let mut st = state_rc.borrow_mut();
    let applied = apply_choice(&mut st, index as usize);
    if let Some(win) = weak.upgrade() {
        let ui = win.global::<VersionFixUi>();
        ui.set_active(false);
        ui.set_status_text("".into());
        if applied {
            sync_window(&win, &mut st);
        }
    }
}

/// Wires Slint UI callbacks for the audio version picker modal.
pub fn wire(window: &MoggytWindow, state: SharedState) {
    let version_ui = window.global::<VersionFixUi>();

    let weak_dismiss = window.as_weak();
    version_ui.on_dismissed(move || handle_dismiss(&weak_dismiss));

    let weak_search = window.as_weak();
    let state_search = state.clone();
    version_ui.on_search_requested(move |q| handle_search_request(&weak_search, &state_search, &q));

    let weak_pick = window.as_weak();
    let state_pick = state.clone();
    version_ui.on_pick_requested(move |idx| handle_pick_request(&weak_pick, &state_pick, idx));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Track;
    use crate::infra::{MemoryVersionOverrideStore, VersionOverrideStore};
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_apply_choice_persists_to_store() {
        let mut state = test_wired();
        let store = MemoryVersionOverrideStore::new();
        state.version_store = Box::new(store.clone());

        state.queue.push(Track::new("orig_id", "Orig Title", "Orig Artist"));
        state.version_fix.results.push(Track::new("alt_id", "Alt Title", "Alt Artist"));

        let applied = apply_choice(&mut state, 0);
        assert!(applied);
        assert_eq!(state.version_overrides.get("orig_id"), Some("alt_id"));
        assert_eq!(store.load().get("orig_id"), Some("alt_id"));
        assert!(state.version_fix.results.is_empty());
    }

    #[test]
    fn test_apply_choice_index_out_of_range_returns_false() {
        let mut state = test_wired();
        state.queue.push(Track::new("orig_id", "Orig Title", "Orig Artist"));
        state.version_fix.results.clear();

        assert!(!apply_choice(&mut state, 0));
        assert!(!apply_choice(&mut state, 5));
        assert!(state.version_overrides.is_empty());
    }

    #[test]
    fn test_apply_choice_self_pick_returns_false() {
        let mut state = test_wired();
        state.queue.push(Track::new("same_id", "Same Title", "Same Artist"));
        state.version_fix.results.push(Track::new("same_id", "Same Title", "Same Artist"));

        assert!(!apply_choice(&mut state, 0));
        assert!(state.version_overrides.is_empty());
    }

    #[test]
    fn test_apply_choice_chained_pick_returns_false() {
        let mut state = test_wired();
        state.version_overrides.set("target_id", "repl_id").expect("initial setup");

        state.queue.push(Track::new("orig_id", "Orig Title", "Orig Artist"));
        state.version_fix.results.push(Track::new("target_id", "Target Title", "Target Artist"));

        assert!(!apply_choice(&mut state, 0));
        assert_eq!(state.version_overrides.get("orig_id"), None);
    }

    #[test]
    fn test_apply_choice_empty_queue_returns_false() {
        let mut state = test_wired();
        assert!(state.queue.is_empty());
        state.version_fix.results.push(Track::new("alt_id", "Alt Title", "Alt Artist"));

        assert!(!apply_choice(&mut state, 0));
        assert!(state.version_overrides.is_empty());
    }
}
