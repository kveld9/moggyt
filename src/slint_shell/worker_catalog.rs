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
        if songs_only {
            items = crate::domain::retain_song_results(items);
        }
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
}
