//! View-phase derivation and window synchronization from application state.
//!
//! Hexagonal invariant: pure in-memory phase projections; no I/O.

use crate::domain::{Album, Playlist};
use crate::infra::CatalogError;
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::view_phase::{ViewId, ViewPhase};

fn derive_library_phase(state: &SlintAppState) -> ViewPhase {
    if state.library_phase == ViewPhase::Error {
        return ViewPhase::Error;
    }
    if !state.library_playlists.is_empty() || !state.library_albums.is_empty() {
        ViewPhase::Ready
    } else {
        ViewPhase::Empty
    }
}

impl SlintAppState {
    /// Applies library query outcomes to state, setting phase and preserving data on error.
    pub(crate) fn apply_library_outcome(
        &mut self,
        playlists: Result<Vec<Playlist>, CatalogError>,
        albums: Result<Vec<Album>, CatalogError>,
    ) {
        match (playlists, albums) {
            (Ok(playlists), Ok(albums)) => {
                let is_empty = playlists.is_empty() && albums.is_empty();
                self.library_playlists = playlists;
                self.library_albums = albums;
                self.library_phase = if is_empty { ViewPhase::Empty } else { ViewPhase::Ready };
            }
            _ => {
                self.library_phase = ViewPhase::Error;
            }
        }
    }
}

fn derive_search_phase(state: &SlintAppState) -> ViewPhase {
    match state.search_phase {
        ViewPhase::Error | ViewPhase::Loading => state.search_phase,
        _ if !state.search_active => ViewPhase::Idle,
        _ if state.has_search_results() => ViewPhase::Ready,
        _ => ViewPhase::Empty,
    }
}

fn derive_explore_phase(state: &SlintAppState) -> ViewPhase {
    if state.explore_phase == ViewPhase::Error {
        return ViewPhase::Error;
    }
    if state.explore_loading {
        ViewPhase::Loading
    } else if !state.explore_shelves.is_empty() {
        ViewPhase::Ready
    } else {
        ViewPhase::Empty
    }
}

fn derive_category_phase(state: &SlintAppState) -> ViewPhase {
    if state.category_phase == ViewPhase::Error {
        return ViewPhase::Error;
    }
    if state.category_loading {
        ViewPhase::Loading
    } else if !state.category_shelves.is_empty() {
        ViewPhase::Ready
    } else {
        ViewPhase::Empty
    }
}

/// Derives and stores each view phase from existing state data.
pub(crate) fn observe_view_phases(state: &mut SlintAppState) {
    state.library_phase = derive_library_phase(state);
    state.search_phase = derive_search_phase(state);
    state.explore_phase = derive_explore_phase(state);
    state.category_phase = derive_category_phase(state);
}

/// Synchronizes a view phase with the corresponding window properties.
pub(crate) fn sync_view_phase(window: &MoggytWindow, view: ViewId, phase: ViewPhase) {
    match view {
        ViewId::Explore => window.set_explore_loading(phase == ViewPhase::Loading),
        ViewId::Category => window.set_category_loading(phase == ViewPhase::Loading),
        ViewId::Search => window.set_search_phase(phase.to_index()),
        // Library and Detail already own their booleans in their sync functions
        // and receive phase-driven properties in later plan tasks.
        ViewId::Library | ViewId::Detail => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_phase_ready_when_playlists_present() {
        use crate::domain::Playlist;
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.library_playlists.push(Playlist::new("p1", "Mix", 3, None));
        observe_view_phases(&mut state);
        assert_eq!(state.library_phase, ViewPhase::Ready);
    }

    #[test]
    fn library_phase_empty_without_data() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        observe_view_phases(&mut state);
        assert_eq!(state.library_phase, ViewPhase::Empty);
    }

    #[test]
    fn library_phase_error_not_overwritten_by_derivation() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.library_phase = ViewPhase::Error;
        observe_view_phases(&mut state);
        assert_eq!(state.library_phase, ViewPhase::Error);
    }

    #[test]
    fn search_phase_error_not_overwritten_by_derivation() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.search_phase = ViewPhase::Error;
        observe_view_phases(&mut state);
        assert_eq!(state.search_phase, ViewPhase::Error);
    }

    #[test]
    fn search_phase_idle_when_inactive() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        observe_view_phases(&mut state);
        assert_eq!(state.search_phase, ViewPhase::Idle);
    }

    #[test]
    fn search_phase_empty_when_active_without_hits() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.search_active = true;
        observe_view_phases(&mut state);
        assert_eq!(state.search_phase, ViewPhase::Empty);
    }

    #[test]
    fn search_phase_loading_not_overwritten_by_derivation() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.search_active = true;
        state.search_phase = ViewPhase::Loading;
        observe_view_phases(&mut state);
        assert_eq!(state.search_phase, ViewPhase::Loading);
    }

    #[test]
    fn explore_phase_loading_while_loading() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.explore_loading = true;
        observe_view_phases(&mut state);
        assert_eq!(state.explore_phase, ViewPhase::Loading);
    }

    #[test]
    fn explore_phase_preserves_error() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.explore_phase = ViewPhase::Error;
        observe_view_phases(&mut state);
        assert_eq!(state.explore_phase, ViewPhase::Error);
    }

    #[test]
    fn category_phase_empty_without_shelves() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        observe_view_phases(&mut state);
        assert_eq!(state.category_phase, ViewPhase::Empty);
    }

    #[test]
    fn category_phase_preserves_error() {
        use crate::slint_shell::test_support::test_wired;

        let mut state = test_wired();
        state.category_phase = ViewPhase::Error;
        observe_view_phases(&mut state);
        assert_eq!(state.category_phase, ViewPhase::Error);
    }

    #[test]
    fn search_phase_indices_match_slint_contract() {
        assert_eq!(ViewPhase::Idle.to_index(), 0);
        assert_eq!(ViewPhase::Loading.to_index(), 1);
        assert_eq!(ViewPhase::Ready.to_index(), 2);
        assert_eq!(ViewPhase::Empty.to_index(), 3);
        assert_eq!(ViewPhase::Error.to_index(), 4);
    }
}
