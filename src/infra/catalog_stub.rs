//! Deterministic catalog stub (no network).

use super::catalog::Catalog;
use crate::domain::{
    Album, Artist, Detail, DetailKind, LyricsSnippet, Playlist, SearchFilter, SearchQuery,
    SearchResult, Shelf, Track,
};

/// Fixed-data stub: keeps UI and tests deterministic until the real API lands.
#[derive(Debug, Clone)]
pub struct CatalogStub {
    liked_ids: Vec<String>,
    last_session_cookies: Option<String>,
}

impl Default for CatalogStub {
    fn default() -> Self {
        Self { liked_ids: vec!["stub-1".to_owned()], last_session_cookies: None }
    }
}

const STUB_ARTWORK_URL: &str = "https://artwork.local/stub-1";

impl CatalogStub {
    /// Creates the stub catalog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the last session cookies received by the stub, if any.
    pub fn last_session_cookies(&self) -> Option<&str> {
        self.last_session_cookies.as_deref()
    }

    fn fixed_hits(query: &str) -> Vec<Track> {
        vec![
            Track::new("stub-1", query, "Stub Artist").with_artwork(STUB_ARTWORK_URL),
            Track::new("stub-2", "B-side", "Stub Artist"),
        ]
    }

    fn stub_liked_hits() -> Vec<Track> {
        vec![
            Track::new("stub-1", "Liked Song", "Stub Artist").with_artwork(STUB_ARTWORK_URL),
            Track::new("stub-2", "B-side", "Stub Artist"),
        ]
    }

    fn stub_lyrics_index() -> Vec<(Track, &'static str)> {
        vec![
            (
                Track::new("stub-1", "Midnight City", "Stub Artist").with_artwork(STUB_ARTWORK_URL),
                "city lights are glowing in the midnight breeze",
            ),
            (
                Track::new("stub-2", "Echoes of Silence", "Stub Artist"),
                "hear the echoes whisper softly in the dark",
            ),
        ]
    }

    fn stub_lyrics_hits(snippet: &str) -> Vec<Track> {
        let needle = snippet.to_ascii_lowercase();
        Self::stub_lyrics_index()
            .into_iter()
            .filter_map(|(track, lyrics)| {
                lyrics.to_ascii_lowercase().contains(&needle).then_some(track)
            })
            .collect()
    }

    fn stub_album() -> Album {
        Album::new("stub-album-1", "Stub Album", "Stub Artist", Some(STUB_ARTWORK_URL.to_owned()))
    }

    fn stub_artist() -> Artist {
        Artist::new("stub-artist-1", "Stub Artist", Some(STUB_ARTWORK_URL.to_owned()))
    }

    fn stub_playlist() -> Playlist {
        Playlist::new("stub-playlist-1", "Stub Playlist", 12, Some(STUB_ARTWORK_URL.to_owned()))
    }

    fn stub_playlists() -> Vec<Playlist> {
        vec![Self::stub_playlist(), Playlist::new("stub-playlist-2", "Favorites", 24, None)]
    }

    fn stub_home_shelves() -> Vec<Shelf> {
        vec![
            Shelf {
                title: "Trending".to_owned(),
                items: vec![
                    SearchResult::Album(Self::stub_album()),
                    SearchResult::Artist(Self::stub_artist()),
                ],
            },
            Shelf {
                title: "Featured Playlists".to_owned(),
                items: vec![SearchResult::Playlist(Self::stub_playlist())],
            },
        ]
    }

    fn stub_explore_shelves() -> Vec<Shelf> {
        vec![
            Shelf {
                title: "Charts".to_owned(),
                items: vec![SearchResult::Track(Track::new(
                    "stub-chart-1",
                    "Top Chart Track",
                    "Stub Artist",
                ))],
            },
            Shelf {
                title: "Moods & genres".to_owned(),
                items: vec![SearchResult::Playlist(Playlist::new(
                    "FEmusic_moods_and_genres_category",
                    "Chill",
                    0,
                    None,
                ))],
            },
        ]
    }
}

impl Catalog for CatalogStub {
    fn search(&self, query: &str) -> Vec<Track> {
        Self::fixed_hits(query)
    }

    fn search_by_lyrics(&self, snippet: &LyricsSnippet) -> Vec<Track> {
        Self::stub_lyrics_hits(snippet.as_str())
    }

    fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult> {
        match query.filter() {
            SearchFilter::All => vec![
                SearchResult::Track(
                    Track::new("stub-1", query.text(), "Stub Artist")
                        .with_artwork(STUB_ARTWORK_URL),
                ),
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

    fn radio(&self, seed_track_id: &str) -> Vec<Track> {
        let trimmed = seed_track_id.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        vec![
            Track::new(&format!("{trimmed}-radio-1"), &format!("{trimmed} Radio 1"), "Stub Artist")
                .with_duration(180),
            Track::new(&format!("{trimmed}-radio-2"), &format!("{trimmed} Radio 2"), "Stub Artist")
                .with_duration(210),
        ]
    }

    fn home_shelves(&self) -> Vec<Shelf> {
        Self::stub_home_shelves()
    }

    fn explore_shelves(&self) -> Vec<Shelf> {
        Self::stub_explore_shelves()
    }

    fn playlists(&self) -> Vec<Playlist> {
        Self::stub_playlists()
    }

    fn liked_tracks(&self) -> Vec<Track> {
        Self::stub_liked_hits()
            .into_iter()
            .filter(|track| self.liked_ids.iter().any(|id| id == &track.id))
            .collect()
    }

    fn saved_albums(&self) -> Vec<Album> {
        vec![Self::stub_album()]
    }

    fn set_liked(&mut self, track_id: &str, liked: bool) {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            return;
        }
        if liked {
            if !self.liked_ids.iter().any(|id| id == trimmed) {
                self.liked_ids.push(trimmed.to_owned());
            }
        } else {
            self.liked_ids.retain(|id| id != trimmed);
        }
    }

    fn detail(&self, browse_id: &str, kind: DetailKind) -> Option<Detail> {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return None;
        }
        let tracks = vec![
            Track::new(&format!("{trimmed}-1"), &format!("{trimmed} Track 1"), "Stub Artist")
                .with_duration(180)
                .with_artwork(STUB_ARTWORK_URL),
            Track::new(&format!("{trimmed}-2"), &format!("{trimmed} Track 2"), "Stub Artist")
                .with_duration(210),
        ];
        Some(Detail::new(kind, &format!("{trimmed} Title"), "Stub Subtitle", tracks))
    }

    fn set_session_cookies(&mut self, cookies: Option<&str>) {
        self.last_session_cookies = cookies.map(str::to_owned);
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
        assert_eq!(hits[0].artwork.as_deref(), Some(STUB_ARTWORK_URL));
        assert_eq!(hits[1].artwork, None);
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
            SearchResult::Track(
                Track::new("stub-1", "echoed", "Stub Artist").with_artwork(STUB_ARTWORK_URL)
            )
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

    #[test]
    fn stub_library_lifecycle_defaults_and_readers() {
        let catalog = CatalogStub::new();

        let playlists = catalog.playlists();
        assert_eq!(playlists.len(), 2);
        assert!(!playlists[0].title.is_empty());
        assert!(!playlists[1].title.is_empty());

        let albums = catalog.saved_albums();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].id, "stub-album-1");

        let liked = catalog.liked_tracks();
        assert_eq!(liked.len(), 1);
        assert_eq!(liked[0].id, "stub-1");
        assert_eq!(liked[0].title, "Liked Song");
    }

    #[test]
    fn stub_library_mutation_and_filtering() {
        let mut catalog = CatalogStub::new();

        // Initial default state: stub-1 liked.
        assert_eq!(catalog.liked_tracks().len(), 1);

        // Add stub-2 -> 2 liked tracks ("stub-1", "stub-2").
        catalog.set_liked("stub-2", true);
        let liked_both = catalog.liked_tracks();
        assert_eq!(liked_both.len(), 2);
        let ids: Vec<_> = liked_both.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["stub-1", "stub-2"]);

        // Duplicate insert keeps exactly 1 entry for that id.
        catalog.set_liked("stub-2", true);
        assert_eq!(catalog.liked_tracks().len(), 2);
        assert_eq!(catalog.liked_tracks().iter().filter(|t| t.id == "stub-2").count(), 1);

        // Remove stub-1 -> only stub-2 remains.
        catalog.set_liked("stub-1", false);
        let liked_one = catalog.liked_tracks();
        assert_eq!(liked_one.len(), 1);
        assert_eq!(liked_one[0].id, "stub-2");

        // Remove stub-2 -> empty.
        catalog.set_liked("stub-2", false);
        assert!(catalog.liked_tracks().is_empty());

        // Blank and whitespace-only ids are ignored.
        catalog.set_liked("", true);
        catalog.set_liked("   ", true);
        catalog.set_liked("", false);
        catalog.set_liked("   ", false);
        assert!(catalog.liked_tracks().is_empty());

        // Unknown id is ignored by stub_liked_hits membership filter.
        catalog.set_liked("unknown-id", true);
        assert!(catalog.liked_tracks().is_empty());
    }

    #[test]
    fn stub_radio_determinism_and_blank_seed() {
        let catalog = CatalogStub::new();
        let first = catalog.radio("vid-seed");
        let second = catalog.radio("vid-seed");
        assert_eq!(first, second, "stub radio must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].id, "vid-seed-radio-1");
        assert_eq!(first[0].artist, "Stub Artist");
        assert_eq!(first[0].duration_secs, Some(180));
        assert_eq!(first[1].id, "vid-seed-radio-2");
        assert_eq!(first[1].artist, "Stub Artist");
        assert_eq!(first[1].duration_secs, Some(210));

        assert!(catalog.radio("").is_empty());
        assert!(catalog.radio("   ").is_empty());
    }

    #[test]
    fn stub_home_shelves_determinism_and_content() {
        let catalog = CatalogStub::new();
        let first = catalog.home_shelves();
        let second = catalog.home_shelves();
        assert_eq!(first, second, "stub home shelves must be deterministic");
        assert_eq!(first.len(), 2);

        assert_eq!(first[0].title, "Trending");
        assert_eq!(first[0].items.len(), 2);
        assert_eq!(first[0].items[0], SearchResult::Album(CatalogStub::stub_album()));
        assert_eq!(first[0].items[1], SearchResult::Artist(CatalogStub::stub_artist()));

        assert_eq!(first[1].title, "Featured Playlists");
        assert_eq!(first[1].items.len(), 1);
        assert_eq!(first[1].items[0], SearchResult::Playlist(CatalogStub::stub_playlist()));
    }

    #[test]
    fn stub_explore_shelves_determinism_and_content() {
        let catalog = CatalogStub::new();
        let first = catalog.explore_shelves();
        let second = catalog.explore_shelves();
        assert_eq!(first, second, "stub explore shelves must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].title, "Charts");
        assert_eq!(first[0].items.len(), 1);
        assert_eq!(first[1].title, "Moods & genres");
        assert_eq!(first[1].items.len(), 1);
    }

    #[test]
    fn stub_detail_determinism_per_kind_and_blank_none() {
        let catalog = CatalogStub::new();
        for kind in [DetailKind::Album, DetailKind::Artist, DetailKind::Playlist] {
            let first = catalog.detail("browse-123", kind);
            let second = catalog.detail("browse-123", kind);
            assert_eq!(first, second, "stub detail must be deterministic");
            let detail = first.expect("expected detail");
            assert_eq!(detail.kind, kind);
            assert_eq!(detail.title, "browse-123 Title");
            assert_eq!(detail.subtitle, "Stub Subtitle");
            assert_eq!(detail.tracks.len(), 2);
            assert_eq!(detail.tracks[0].id, "browse-123-1");
            assert_eq!(detail.tracks[0].duration_secs, Some(180));
            assert_eq!(detail.tracks[0].artwork.as_deref(), Some(STUB_ARTWORK_URL));
            assert_eq!(detail.tracks[1].id, "browse-123-2");
            assert_eq!(detail.tracks[1].duration_secs, Some(210));
        }

        assert_eq!(catalog.detail("", DetailKind::Album), None);
        assert_eq!(catalog.detail("   ", DetailKind::Album), None);
    }

    #[test]
    fn stub_set_session_cookies_seam_records_and_clears() {
        let mut catalog = CatalogStub::new();
        assert_eq!(catalog.last_session_cookies(), None);

        catalog.set_session_cookies(Some("SID=session123"));
        assert_eq!(catalog.last_session_cookies(), Some("SID=session123"));

        catalog.set_session_cookies(None);
        assert_eq!(catalog.last_session_cookies(), None);
    }

    #[test]
    fn stub_matches_lyric_substring_and_misses_on_unrelated_text() {
        let catalog = CatalogStub::new();

        let hit1 = catalog.search_by_lyrics(&LyricsSnippet::parse("midnight breeze").unwrap());
        assert_eq!(hit1.len(), 1);
        assert_eq!(hit1[0].id, "stub-1");
        assert_eq!(hit1[0].title, "Midnight City");
        assert_eq!(hit1[0].artwork.as_deref(), Some(STUB_ARTWORK_URL));

        // Case-insensitive substring match
        let hit2 = catalog.search_by_lyrics(&LyricsSnippet::parse("WHISPER SOFTLY").unwrap());
        assert_eq!(hit2.len(), 1);
        assert_eq!(hit2[0].id, "stub-2");
        assert_eq!(hit2[0].title, "Echoes of Silence");

        // Unrelated query returns empty vec
        let misses = catalog.search_by_lyrics(
            &LyricsSnippet::parse("completely unrelated lyrics not in index").unwrap(),
        );
        assert!(misses.is_empty());
    }
}
