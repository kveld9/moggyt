//! Catalog port and deterministic stub (no network).

use super::innertube::{
    InnerTube, RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits, parse_search_all,
};
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
}
