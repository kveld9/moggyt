//! Catalog port and deterministic stub (no network).

use crate::domain::{
    Album, Detail, DetailKind, LyricsSnippet, Playlist, SearchQuery, SearchResult, Shelf, Track,
};

/// Catalog port: search by free text and user library read/mutation access.
pub trait Catalog {
    /// Returns catalog hits for `query` (empty when nothing matches).
    fn search(&self, query: &str) -> Vec<Track>;

    /// Returns catalog hits matching the query text and filter.
    fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult>;

    /// Returns catalog hits matching lyrics in `snippet` (empty when nothing matches).
    ///
    /// Default implementation forwards the snippet text to [`Catalog::search`].
    fn search_by_lyrics(&self, snippet: &LyricsSnippet) -> Vec<Track> {
        self.search(snippet.as_str())
    }

    /// Returns radio continuation tracks seeded by `seed_track_id` (empty when seed is blank).
    fn radio(&self, seed_track_id: &str) -> Vec<Track>;

    /// Returns home shelves (empty on failure).
    fn home_shelves(&self) -> Vec<Shelf>;

    /// Returns explore shelves (empty on failure).
    fn explore_shelves(&self) -> Vec<Shelf> {
        Vec::new()
    }

    /// Returns saved or owned playlists.
    fn playlists(&self) -> Vec<Playlist>;

    /// Returns albums saved to the user's library.
    fn saved_albums(&self) -> Vec<Album>;

    /// Returns detail view for `browse_id` and `kind` (None on blank id or failure).
    fn detail(&self, browse_id: &str, kind: DetailKind) -> Option<Detail>;

    /// Updates the session cookies used for authenticated catalog requests.
    ///
    /// Default implementation is a no-op for anonymous catalogs or stubs.
    fn set_session_cookies(&mut self, _cookies: Option<&str>) {}

    /// Updates the interface locale used for localized catalog requests.
    ///
    /// Default implementation is a no-op for stubs or static catalogs.
    fn set_locale(&mut self, _locale: crate::i18n::Locale) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct PassThroughCatalog {
        last_query: std::sync::Mutex<Option<String>>,
    }

    impl Catalog for PassThroughCatalog {
        fn search(&self, query: &str) -> Vec<Track> {
            *self.last_query.lock().unwrap() = Some(query.to_owned());
            vec![Track::new("pass-1", query, "Pass Artist")]
        }
        fn search_filtered(&self, _query: &SearchQuery) -> Vec<SearchResult> {
            Vec::new()
        }
        fn radio(&self, _seed_track_id: &str) -> Vec<Track> {
            Vec::new()
        }
        fn home_shelves(&self) -> Vec<Shelf> {
            Vec::new()
        }
        fn playlists(&self) -> Vec<Playlist> {
            Vec::new()
        }
        fn saved_albums(&self) -> Vec<Album> {
            Vec::new()
        }
        fn detail(&self, _browse_id: &str, _kind: DetailKind) -> Option<Detail> {
            None
        }
    }

    #[test]
    fn default_forwarding_works_through_pass_through_catalog() {
        let pass_through = PassThroughCatalog::default();
        let snippet = LyricsSnippet::parse("running through the night").unwrap();
        let hits = pass_through.search_by_lyrics(&snippet);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "pass-1");
        assert_eq!(hits[0].title, "running through the night");
        let recorded = pass_through.last_query.lock().unwrap().clone();
        assert_eq!(recorded.as_deref(), Some("running through the night"));
    }

    #[test]
    fn default_explore_shelves_returns_empty_vec() {
        let pass_through = PassThroughCatalog::default();
        assert!(pass_through.explore_shelves().is_empty());
    }
}
