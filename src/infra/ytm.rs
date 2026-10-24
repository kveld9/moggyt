//! Catalog port and deterministic stub (no network).

use crate::domain::{Album, Artist, Playlist, SearchFilter, SearchQuery, SearchResult, Track};

/// Read-only catalog port: search by free text.
pub trait Catalog {
    /// Returns catalog hits for `query` (empty when nothing matches).
    fn search(&self, query: &str) -> Vec<Track>;

    /// Returns catalog hits matching the query text and filter.
    fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult>;
}

/// Fixed-data stub: keeps UI and tests deterministic until the real API lands.
#[derive(Debug, Default)]
pub struct CatalogStub;

impl CatalogStub {
    /// Creates the stub catalog.
    pub fn new() -> Self {
        Self
    }

    fn fixed_hits(query: &str) -> Vec<Track> {
        vec![
            Track::new("stub-1", query, "Stub Artist"),
            Track::new("stub-2", "B-side", "Stub Artist"),
        ]
    }

    fn stub_album() -> Album {
        Album::new("stub-album-1", "Stub Album", "Stub Artist")
    }

    fn stub_artist() -> Artist {
        Artist::new("stub-artist-1", "Stub Artist")
    }

    fn stub_playlist() -> Playlist {
        Playlist::new("stub-playlist-1", "Stub Playlist", 12)
    }
}

impl Catalog for CatalogStub {
    fn search(&self, query: &str) -> Vec<Track> {
        Self::fixed_hits(query)
    }

    fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult> {
        match query.filter() {
            SearchFilter::All => vec![
                SearchResult::Track(Track::new("stub-1", query.text(), "Stub Artist")),
                SearchResult::Album(Self::stub_album()),
                SearchResult::Artist(Self::stub_artist()),
                SearchResult::Playlist(Self::stub_playlist()),
            ],
            SearchFilter::Tracks => {
                Self::fixed_hits(query.text()).into_iter().map(SearchResult::Track).collect()
            }
            SearchFilter::Albums => vec![SearchResult::Album(Self::stub_album())],
            SearchFilter::Artists => vec![SearchResult::Artist(Self::stub_artist())],
            SearchFilter::Playlists => vec![SearchResult::Playlist(Self::stub_playlist())],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SearchFilter, SearchQuery, SearchResult};

    #[test]
    fn stub_returns_deterministic_hits() {
        let catalog = CatalogStub::new();
        let hits = catalog.search("midnight");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "midnight");
    }

    #[test]
    fn stub_search_filtered_respects_filter_and_is_deterministic() {
        let catalog = CatalogStub::new();
        let query_all = SearchQuery::parse("echoed").expect("valid query");
        let first_all = catalog.search_filtered(&query_all);
        let second_all = catalog.search_filtered(&query_all);
        assert_eq!(first_all, second_all);
        assert_eq!(first_all.len(), 4);
        assert_eq!(
            first_all[0],
            SearchResult::Track(Track::new("stub-1", "echoed", "Stub Artist"))
        );

        let cases = [
            (SearchFilter::Tracks, 2),
            (SearchFilter::Albums, 1),
            (SearchFilter::Artists, 1),
            (SearchFilter::Playlists, 1),
        ];

        for (filter, expected_count) in cases {
            let query = SearchQuery::parse("echoed").expect("valid query").with_filter(filter);
            let hits = catalog.search_filtered(&query);
            assert_eq!(hits.len(), expected_count);
            assert!(hits.iter().all(|item| item.kind() == filter));
        }

        let legacy_hits = catalog.search("echoed");
        assert_eq!(legacy_hits.len(), 2);
        assert_eq!(legacy_hits[0].title, "echoed");
    }
}
