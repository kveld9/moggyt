//! Explore section catalog querying, async loading, and navigation models.
//!
//! Hexagonal invariant: queries catalog port asynchronously off the UI thread and dispatches
//! results back to Slint via the event loop.

use std::sync::{Arc, Mutex};

use crate::domain::{SearchResult, Shelf};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::covers::request_explore_covers;
use crate::slint_shell::platform::dispatch_to_ui;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::sync::sync_explore_shelves;
use crate::slint_shell::window::with_ui_state;

/// Queries catalog for explore shelves asynchronously off the UI thread.
pub fn execute_explore<C: Catalog>(generation: u64, catalog: &Arc<Mutex<C>>) -> (u64, Vec<Shelf>) {
    let shelves = match catalog.lock() {
        Ok(guard) => guard.explore_shelves(),
        Err(_) => Vec::new(),
    };
    (generation, shelves)
}

/// Helper resolving an item in explore shelves by linear index.
pub(crate) fn explore_item_at(shelves: &[Shelf], index: usize) -> Option<&SearchResult> {
    shelves.iter().flat_map(|s| s.items.iter()).nth(index)
}

/// Applies completed explore shelves fetch to state and UI on main thread.
pub(crate) fn apply_explore_completion<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    win: &MoggytWindow,
    st: &mut SlintAppState<C, A, S, L, K>,
    generation: u64,
    shelves: Vec<Shelf>,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    if st.explore_generation != generation {
        return;
    }
    st.explore_shelves = shelves;
    st.explore_loaded = true;
    st.explore_loading = false;
    win.set_explore_loading(false);
    request_explore_covers(st, tx);
    sync_explore_shelves(win, st);
}

/// Dispatches asynchronous explore shelves loading off the UI thread.
pub(crate) fn load_explore_async<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    state.explore_generation = state.explore_generation.wrapping_add(1);
    let generation = state.explore_generation;
    state.explore_loading = true;
    window.set_explore_loading(true);

    let cat = Arc::clone(&state.catalog);
    let weak = window.as_weak();
    let tx = worker_tx.clone();

    std::thread::spawn(move || {
        let (g, shelves) = execute_explore(generation, &cat);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else { return };
            with_ui_state::<C, A, S, L, K, _>(|st| {
                apply_explore_completion(&win, st, g, shelves, &tx);
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, Track};
    use crate::infra::CatalogStub;
    use crate::slint_shell::page::SlintPage;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_explore_state_initialization_and_selection() {
        let mut state = test_wired();
        assert!(state.explore_shelves.is_empty());
        assert!(!state.explore_loaded);
        state.select_page(SlintPage::Explore);
        assert_eq!(state.page, SlintPage::Explore);
        assert!(state.explore_loaded);
        assert_eq!(state.explore_shelves.len(), 2);
        assert_eq!(state.explore_shelves[0].title, "Charts");
        assert_eq!(state.explore_shelves[1].title, "Moods & genres");
    }

    #[test]
    fn test_execute_explore_uses_catalog_port_and_generation() {
        let cat = Arc::new(Mutex::new(CatalogStub::new()));
        let (generation, shelves) = execute_explore(42, &cat);
        assert_eq!(generation, 42);
        assert_eq!(shelves.len(), 2);
        assert_eq!(shelves[0].title, "Charts");
        assert_eq!(shelves[1].title, "Moods & genres");
    }

    #[test]
    fn test_explore_item_at_indexing() {
        let shelves = vec![
            Shelf {
                title: "Charts".to_owned(),
                items: vec![
                    SearchResult::Track(Track::new("c1", "Track 1", "Artist 1")),
                    SearchResult::Track(Track::new("c2", "Track 2", "Artist 2")),
                ],
            },
            Shelf {
                title: "New Releases".to_owned(),
                items: vec![SearchResult::Album(Album::new("a1", "Album 1", "Artist 1", None))],
            },
        ];
        assert_eq!(
            explore_item_at(&shelves, 0),
            Some(&SearchResult::Track(Track::new("c1", "Track 1", "Artist 1")))
        );
        assert_eq!(
            explore_item_at(&shelves, 1),
            Some(&SearchResult::Track(Track::new("c2", "Track 2", "Artist 2")))
        );
        assert_eq!(
            explore_item_at(&shelves, 2),
            Some(&SearchResult::Album(Album::new("a1", "Album 1", "Artist 1", None)))
        );
        assert_eq!(explore_item_at(&shelves, 3), None);
    }
}
