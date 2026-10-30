//! Explore section catalog querying, async loading, and navigation models.
//!
//! Hexagonal invariant: queries catalog port asynchronously off the UI thread and dispatches
//! results back to Slint via the event loop.

use std::sync::{Arc, Mutex};

use slint::ComponentHandle;

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

/// Re-dispatches explore cover fetches when Explore is re-entered with shelves already
/// loaded. Idempotent: `request_explore_covers` only dispatches URLs missing from the
/// UI cover cache. Returns false when there is nothing to refresh.
pub(crate) fn refresh_explore_covers_if_loaded<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    st: &SlintAppState<C, A, S, L, K>,
    tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) -> bool {
    if !st.explore_loaded || st.explore_shelves.is_empty() {
        return false;
    }
    request_explore_covers(st, tx);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, Track};
    use crate::infra::CatalogStub;
    use crate::slint_shell::page::SlintPage;
    use crate::slint_shell::playback_worker::thumbnail_variant_for_row;
    use crate::slint_shell::search::SearchRow;
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

    #[test]
    fn test_refresh_explore_covers_if_loaded_behavior() {
        let mut state = test_wired();
        let (tx, rx) = std::sync::mpsc::channel();

        // 1. Returns false and dispatches nothing when explore_loaded is false.
        assert!(!state.explore_loaded);
        assert!(!refresh_explore_covers_if_loaded(&state, &tx));
        assert!(rx.try_recv().is_err());

        // 2. Returns true and dispatches exactly two FetchCover commands when loaded with 2 artwork items.
        let url1 = "https://lh3.googleusercontent.com/track-large";
        let url2 = "https://lh3.googleusercontent.com/album-large";
        let item1 = SearchResult::Track(Track::new("t1", "Track 1", "Artist 1").with_artwork(url1));
        let item2 =
            SearchResult::Album(Album::new("a1", "Album 1", "Artist 1", Some(url2.to_string())));
        let expected_url1 =
            thumbnail_variant_for_row(SearchRow::from(&item1).artwork_url.as_deref().unwrap());
        let expected_url2 =
            thumbnail_variant_for_row(SearchRow::from(&item2).artwork_url.as_deref().unwrap());

        state.explore_loaded = true;
        state.explore_generation = 42;
        state.explore_shelves = vec![
            Shelf { title: "Shelf 1".to_string(), items: vec![item1] },
            Shelf { title: "Shelf 2".to_string(), items: vec![item2] },
        ];

        assert!(refresh_explore_covers_if_loaded(&state, &tx));

        let mut received = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            if let PlaybackWorkerCommand::FetchCover { url, generation } = cmd {
                received.push((url, generation));
            }
        }
        assert_eq!(received.len(), 2);
        assert_eq!(received[0], (expected_url1.clone(), 42));
        assert_eq!(received[1], (expected_url2.clone(), 42));

        // 3. Returns true but dispatches no command when both thumbnails are already in search_covers.
        state.search_covers.insert(expected_url1, slint::Image::default());
        state.search_covers.insert(expected_url2, slint::Image::default());

        assert!(refresh_explore_covers_if_loaded(&state, &tx));
        assert!(rx.try_recv().is_err());
    }
}
