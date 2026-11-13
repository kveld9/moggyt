//! Resolve an artist or album by name and open its detail page.
//!
//! The catalog search runs off the UI thread; only the navigation happens on the UI thread.

use std::sync::{Arc, Mutex};

use slint::ComponentHandle;

use crate::domain::{DetailKind, SearchFilter, SearchQuery, SearchResult};
use crate::i18n::tr;
use crate::infra::{Catalog, CatalogError};
use crate::slint_shell::platform::dispatch_to_ui;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::toast::notify;
use crate::slint_shell::window::{open_detail_async, with_ui_state};
use crate::slint_shell::{ContextIcon, MoggytWindow};

/// Catalog filter used to resolve a container kind by name.
fn filter_for(kind: DetailKind) -> SearchFilter {
    match kind {
        DetailKind::Artist => SearchFilter::Artists,
        DetailKind::Album => SearchFilter::Albums,
        DetailKind::Playlist => SearchFilter::Playlists,
    }
}

/// Localized message shown when the container cannot be resolved.
fn missing_key(kind: DetailKind) -> &'static str {
    match kind {
        DetailKind::Artist => "No artist found",
        _ => "No album found",
    }
}

/// Returns the browse id of the first catalog item matching `kind`.
pub fn first_container_id(items: &[SearchResult], kind: DetailKind) -> Option<String> {
    items.iter().find_map(|item| match (kind, item) {
        (DetailKind::Artist, SearchResult::Artist(artist)) => Some(artist.id.clone()),
        (DetailKind::Album, SearchResult::Album(album)) => Some(album.id.clone()),
        (DetailKind::Playlist, SearchResult::Playlist(playlist)) => Some(playlist.id.clone()),
        _ => None,
    })
}

/// Executes a filtered catalog search off the UI thread.
pub fn execute_named_search<C: Catalog>(
    query: &SearchQuery,
    catalog: &Arc<Mutex<C>>,
) -> Result<Vec<SearchResult>, CatalogError> {
    catalog
        .lock()
        .map_err(|_| CatalogError::Transport)
        .and_then(|catalog| catalog.search_filtered(query))
}

/// Resolves `query` to the first `kind` item and opens its detail page.
pub(crate) fn open_named_detail_async(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    query: &str,
    kind: DetailKind,
) {
    let Some(parsed) = SearchQuery::parse(query).map(|query| query.with_filter(filter_for(kind)))
    else {
        return;
    };
    let Some(worker_tx) = state.worker_tx.clone() else {
        return;
    };
    let catalog = Arc::clone(&state.catalog);
    let weak = window.as_weak();

    std::thread::spawn(move || {
        let outcome = execute_named_search(&parsed, &catalog);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else {
                return;
            };
            with_ui_state(|st| match outcome {
                Ok(items) => match first_container_id(&items, kind) {
                    Some(browse_id) => open_detail_async(&win, st, &browse_id, kind, &worker_tx),
                    None => notify(&win, st, tr(st.locale, missing_key(kind)), ContextIcon::Close),
                },
                Err(_) => notify(&win, st, tr(st.locale, missing_key(kind)), ContextIcon::Close),
            });
        });
    });
}

/// Artist name of the track under the queue cursor, trimmed.
///
/// Returns `None` when nothing is playing or the artist field is blank.
pub(crate) fn current_artist_query(state: &SlintAppState) -> Option<String> {
    let artist = state.queue.current()?.artist.trim();
    if artist.is_empty() {
        return None;
    }
    Some(artist.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, Artist, Playlist, Track};
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn first_container_id_picks_the_first_matching_kind() {
        let items = vec![
            SearchResult::Track(Track::new("t1", "Title", "Artist")),
            SearchResult::Album(Album::new("al1", "Album", "Artist", None)),
            SearchResult::Artist(Artist::new("ar1", "Artist", None)),
            SearchResult::Album(Album::new("al2", "Other", "Artist", None)),
        ];

        assert_eq!(first_container_id(&items, DetailKind::Album), Some("al1".to_owned()));
        assert_eq!(first_container_id(&items, DetailKind::Artist), Some("ar1".to_owned()));
        assert_eq!(first_container_id(&items, DetailKind::Playlist), None);
    }

    #[test]
    fn first_container_id_returns_the_first_playlist() {
        let items = vec![SearchResult::Playlist(Playlist::new("pl1", "List", 3, None))];
        assert_eq!(first_container_id(&items, DetailKind::Playlist), Some("pl1".to_owned()));
    }

    #[test]
    fn current_artist_query_returns_the_trimmed_artist_of_the_current_track() {
        let mut state = test_wired();
        assert_eq!(current_artist_query(&state), None);

        state.queue.push(Track::new("t1", "Title", "  Artist Name  "));
        assert_eq!(current_artist_query(&state), Some("Artist Name".to_owned()));

        state.queue.push(Track::new("t2", "Title", "   "));
        state.queue.advance();
        assert_eq!(current_artist_query(&state), None);
    }
}
