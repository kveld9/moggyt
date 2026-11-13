//! Blocking catalog search executors run on the playback worker thread.

use std::sync::{Arc, Mutex};

use crate::domain::{LyricsSnippet, SearchQuery, SearchResult, Shelf};
use crate::i18n::{Locale, tr};
use crate::infra::{Catalog, CatalogError};

/// Routes a lyrics-fragment query to the lyrics resolver, any other query to the catalog.
pub fn resolve_search<C: Catalog>(
    catalog: &C,
    query: &SearchQuery,
) -> Result<Vec<SearchResult>, CatalogError> {
    if !query.is_lyrics() {
        return catalog.search_filtered(query);
    }
    let snippet = LyricsSnippet::parse(query.text()).map_err(|_| CatalogError::Transport)?;
    catalog
        .search_by_lyrics(&snippet)
        .map(|tracks| tracks.into_iter().map(SearchResult::Track).collect())
}

/// Executes blocking search request off the UI thread and wraps results in a shelf.
pub fn execute_search<C: Catalog>(
    generation: u64,
    query: &SearchQuery,
    catalog: &Arc<Mutex<C>>,
    locale: Locale,
    songs_only: bool,
) -> (u64, String, Result<Vec<Shelf>, CatalogError>) {
    let res = catalog
        .lock()
        .map_err(|_| CatalogError::Transport)
        .and_then(|c| resolve_search(&*c, query));
    let shelves = res.map(|mut items| {
        items = crate::domain::apply_result_policy(items, songs_only);
        let mut shelf = Shelf::new(tr(locale, "Search Results"));
        shelf.items = items;
        vec![shelf]
    });
    (generation, query.text().to_string(), shelves)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::MockLibraryCatalog;

    #[test]
    fn test_resolve_search_plain_query_delegates_to_catalog() {
        let catalog = MockLibraryCatalog::default();
        let query = SearchQuery::parse("test").expect("valid query");
        let results = resolve_search(&catalog, &query).expect("results ok");
        assert!(results.is_empty());
    }

    #[test]
    fn test_execute_search_ranks_songs_ahead_of_music_videos() {
        use crate::domain::{MusicVideoType, Track};

        let video = SearchResult::Track(
            Track::new("v1", "Video", "Artist").with_video_type(MusicVideoType::Omv),
        );
        let song = SearchResult::Track(
            Track::new("s1", "Song", "Artist").with_video_type(MusicVideoType::Atv),
        );
        let catalog = MockLibraryCatalog::default()
            .with_search_filtered(Ok(vec![video.clone(), song.clone()]));
        let cat = Arc::new(Mutex::new(catalog));
        let query = SearchQuery::parse("zzz").expect("valid query");

        let (_, _, shelves) = execute_search(1, &query, &cat, Locale::English, false);

        assert_eq!(shelves.expect("shelves ok")[0].items, vec![song, video]);
    }
}
