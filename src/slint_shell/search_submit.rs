//! Asynchronous catalog search submission and UI synchronization.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use slint::ComponentHandle;

use crate::domain::{SearchFilter, SearchQuery};
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::covers::request_shelf_covers;
use crate::slint_shell::platform::dispatch_to_ui;
use crate::slint_shell::playback_worker::{PlaybackWorkerCommand, execute_search};
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::sync::{
    observe_view_phases, persist_recent_searches, sync_recent_searches, sync_shelves_and_queue,
    sync_view_phase,
};
use crate::slint_shell::view_phase::{ViewId, ViewPhase};
use crate::slint_shell::window::{apply_async_search_outcome, with_ui_state};

fn search_filter_index(filter: SearchFilter) -> i32 {
    match filter {
        SearchFilter::All => 0,
        SearchFilter::Tracks => 1,
        SearchFilter::Albums => 2,
        SearchFilter::Artists => 3,
        SearchFilter::Playlists => 4,
    }
}

/// Dispatches asynchronous search query with a category filter off the UI thread.
pub(crate) fn submit_search_with_filter_async(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    query: &str,
    filter: SearchFilter,
    worker_tx: &Sender<PlaybackWorkerCommand>,
) {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return;
    }
    state.search_active = true;
    state.recent_searches.record(trimmed);
    persist_recent_searches(state);
    sync_recent_searches(window, state);

    state.search_query = query.to_string();
    state.search_filter = filter;
    state.search_top_result = None;
    state.search_phase = ViewPhase::Loading;
    state.search_generation = state.search_generation.wrapping_add(1);
    let generation = state.search_generation;

    window.set_query(query.into());
    let filter_index = if state.lyrics_search { 5 } else { search_filter_index(filter) };
    window.set_search_filter(filter_index);
    sync_shelves_and_queue(window, state);
    sync_view_phase(window, ViewId::Search, state.search_phase);

    let Some(parsed_query) =
        SearchQuery::parse(trimmed).map(|q| q.with_filter(filter).with_lyrics(state.lyrics_search))
    else {
        state.search_phase = ViewPhase::Empty;
        sync_view_phase(window, ViewId::Search, state.search_phase);
        return;
    };
    let cat = Arc::clone(&state.catalog);
    let weak = window.as_weak();
    let tx = worker_tx.clone();
    let loc = state.locale;
    let songs_only = state.songs_only;

    std::thread::spawn(move || {
        let (g, q_res, shelves) = execute_search(generation, &parsed_query, &cat, loc, songs_only);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else { return };
            with_ui_state(|st| {
                if apply_async_search_outcome(st, g, q_res, shelves) {
                    request_shelf_covers(st, &tx);
                }
                win.set_query(st.search_query.as_str().into());
                observe_view_phases(st);
                sync_shelves_and_queue(&win, st);
                sync_view_phase(&win, ViewId::Search, st.search_phase);
            });
        });
    });
}

/// Dispatches asynchronous search query off the UI thread (defaults to All filter).
pub(crate) fn submit_search_async(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    query: &str,
    worker_tx: &Sender<PlaybackWorkerCommand>,
) {
    submit_search_with_filter_async(window, state, query, SearchFilter::All, worker_tx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_filter_index_mapping() {
        assert_eq!(search_filter_index(SearchFilter::All), 0);
        assert_eq!(search_filter_index(SearchFilter::Tracks), 1);
        assert_eq!(search_filter_index(SearchFilter::Albums), 2);
        assert_eq!(search_filter_index(SearchFilter::Artists), 3);
        assert_eq!(search_filter_index(SearchFilter::Playlists), 4);
    }
}
