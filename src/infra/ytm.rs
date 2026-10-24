//! Catalog port and deterministic stub (no network).

use super::innertube::{
    InnerTube, RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits, parse_search_all,
};
use crate::domain::{Album, Artist, Playlist, SearchFilter, SearchQuery, SearchResult, Track};

/// Catalog port: search by free text and user library read/mutation access.
pub trait Catalog {
    /// Returns catalog hits for `query` (empty when nothing matches).
    fn search(&self, query: &str) -> Vec<Track>;

    /// Returns catalog hits matching the query text and filter.
    fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult>;

    /// Returns saved or owned playlists.
    fn playlists(&self) -> Vec<Playlist>;

    /// Returns tracks saved to the user's liked collection.
    fn liked_tracks(&self) -> Vec<Track>;

    /// Returns albums saved to the user's library.
    fn saved_albums(&self) -> Vec<Album>;

    /// Adds (`liked = true`) or removes (`liked = false`) a track from the liked collection.
    /// Blank identifiers are ignored.
    fn set_liked(&mut self, track_id: &str, liked: bool);
}

/// Fixed-data stub: keeps UI and tests deterministic until the real API lands.
#[derive(Debug, Clone)]
pub struct CatalogStub {
    liked_ids: Vec<String>,
}

impl Default for CatalogStub {
    fn default() -> Self {
        Self { liked_ids: vec!["stub-1".to_owned()] }
    }
}

impl CatalogStub {
    /// Creates the stub catalog.
    pub fn new() -> Self {
        Self::default()
    }

    fn fixed_hits(query: &str) -> Vec<Track> {
        vec![
            Track::new("stub-1", query, "Stub Artist"),
            Track::new("stub-2", "B-side", "Stub Artist"),
        ]
    }

    fn stub_liked_hits() -> Vec<Track> {
        vec![
            Track::new("stub-1", "Liked Song", "Stub Artist"),
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

    fn stub_playlists() -> Vec<Playlist> {
        vec![Self::stub_playlist(), Playlist::new("stub-playlist-2", "Favorites", 24)]
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
}

/// InnerTube-backed YouTube Music catalog implementing the [`Catalog`] port.
#[derive(Debug, Default)]
pub struct YtmCatalog {
    tube: InnerTube,
}

impl YtmCatalog {
    /// Creates a new catalog backed by default InnerTube client.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a catalog injecting an InnerTube client for testing or custom configuration.
    pub fn with_tube(tube: InnerTube) -> Self {
        Self { tube }
    }

    fn fetch_hits(&self, query_text: &str) -> SearchHits {
        self.tube
            .search_payload(query_text)
            .map_or_else(|_| SearchHits::empty(), |p| parse_search_all(&p))
    }

    /// Maps search hits into domain search results according to the requested filter.
    pub fn map_hits(hits: SearchHits, filter: SearchFilter) -> Vec<SearchResult> {
        match filter {
            SearchFilter::All => map_tracks(hits.tracks)
                .chain(map_albums(hits.albums))
                .chain(map_artists(hits.artists))
                .chain(map_playlists(hits.playlists))
                .collect(),
            SearchFilter::Tracks => map_tracks(hits.tracks).collect(),
            SearchFilter::Albums => map_albums(hits.albums).collect(),
            SearchFilter::Artists => map_artists(hits.artists).collect(),
            SearchFilter::Playlists => map_playlists(hits.playlists).collect(),
        }
    }
}

fn map_tracks(tracks: Vec<RawTrack>) -> impl Iterator<Item = SearchResult> {
    tracks.into_iter().map(|t| SearchResult::Track(Track::new(&t.video_id, &t.title, &t.artist)))
}

fn map_albums(albums: Vec<RawAlbum>) -> impl Iterator<Item = SearchResult> {
    albums.into_iter().map(|a| SearchResult::Album(Album::new(&a.browse_id, &a.title, &a.artist)))
}

fn map_artists(artists: Vec<RawArtist>) -> impl Iterator<Item = SearchResult> {
    artists.into_iter().map(|a| SearchResult::Artist(Artist::new(&a.browse_id, &a.name)))
}

fn map_playlists(playlists: Vec<RawPlaylist>) -> impl Iterator<Item = SearchResult> {
    playlists.into_iter().map(|p| SearchResult::Playlist(Playlist::new(&p.browse_id, &p.title, 0)))
}

impl Catalog for YtmCatalog {
    /// Returns catalog hits for `query`. Network failures map to an empty vector.
    fn search(&self, query: &str) -> Vec<Track> {
        self.tube
            .search_tracks(query)
            .unwrap_or_default()
            .into_iter()
            .map(|t| Track::new(&t.video_id, &t.title, &t.artist))
            .collect()
    }

    /// Returns catalog hits matching the query text and filter. Network failures map to an empty vector.
    fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult> {
        Self::map_hits(self.fetch_hits(query.text()), query.filter())
    }

    /// Returns saved or owned playlists. Anonymous interim returns an empty list
    /// because authenticated library endpoints require a signed-in InnerTube client.
    fn playlists(&self) -> Vec<Playlist> {
        Vec::new()
    }

    /// Returns liked tracks. Anonymous interim returns an empty list
    /// because authenticated library endpoints require a signed-in InnerTube client.
    fn liked_tracks(&self) -> Vec<Track> {
        Vec::new()
    }

    /// Returns saved albums. Anonymous interim returns an empty list
    /// because authenticated library endpoints require a signed-in InnerTube client.
    fn saved_albums(&self) -> Vec<Album> {
        Vec::new()
    }

    /// Sets liked status. Anonymous interim is a no-op because an unauthenticated
    /// client cannot mutate remote user library state.
    fn set_liked(&mut self, _track_id: &str, _liked: bool) {}
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

    #[test]
    fn ytm_catalog_maps_hits_and_honors_filters() {
        use super::super::innertube::{RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits};

        let hits = SearchHits {
            tracks: vec![RawTrack {
                video_id: "v1".into(),
                title: "T1".into(),
                artist: "A1".into(),
            }],
            albums: vec![RawAlbum {
                browse_id: "b1".into(),
                title: "Alb".into(),
                artist: "Art".into(),
            }],
            artists: vec![RawArtist { browse_id: "ar1".into(), name: "ArtName".into() }],
            playlists: vec![RawPlaylist { browse_id: "p1".into(), title: "PlayTitle".into() }],
        };

        let all = YtmCatalog::map_hits(hits.clone(), SearchFilter::All);
        assert_eq!(all.len(), 4);
        assert_eq!(all[0], SearchResult::Track(Track::new("v1", "T1", "A1")));
        assert_eq!(all[1], SearchResult::Album(Album::new("b1", "Alb", "Art")));
        assert_eq!(all[2], SearchResult::Artist(Artist::new("ar1", "ArtName")));
        assert_eq!(all[3], SearchResult::Playlist(Playlist::new("p1", "PlayTitle", 0)));

        for (filter, expected) in [
            (SearchFilter::Tracks, 1),
            (SearchFilter::Albums, 1),
            (SearchFilter::Artists, 1),
            (SearchFilter::Playlists, 1),
        ] {
            let mapped = YtmCatalog::map_hits(hits.clone(), filter);
            assert_eq!(mapped.len(), expected);
            assert!(mapped.iter().all(|item| item.kind() == filter));
        }
    }

    #[test]
    fn ytm_catalog_constructors_and_tube_injection() {
        assert!(format!("{:?}", YtmCatalog::new()).contains("YtmCatalog"));
        assert!(format!("{:?}", YtmCatalog::default()).contains("YtmCatalog"));
        assert!(format!("{:?}", InnerTube::new().into_catalog()).contains("YtmCatalog"));
        assert!(format!("{:?}", YtmCatalog::with_tube(InnerTube::new())).contains("YtmCatalog"));
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
    fn ytm_catalog_interim_library_semantics() {
        let mut catalog = YtmCatalog::new();

        assert!(catalog.playlists().is_empty());
        assert!(catalog.saved_albums().is_empty());
        assert!(catalog.liked_tracks().is_empty());

        // Anonymous interim mutation is a documented no-op and does not panic.
        catalog.set_liked("stub-1", true);
        catalog.set_liked("stub-1", false);
        catalog.set_liked("", true);

        assert!(catalog.playlists().is_empty());
        assert!(catalog.saved_albums().is_empty());
        assert!(catalog.liked_tracks().is_empty());
    }
}
