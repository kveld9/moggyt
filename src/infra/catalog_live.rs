//! Production catalog adapter implementing the [`Catalog`] port via [`CatalogClient`].

use super::catalog::Catalog;
use super::catalog_client::{
    CatalogClient, RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits, parse_search_all,
};
use crate::domain::{
    Album, Artist, Detail, DetailKind, Playlist, SearchFilter, SearchQuery, SearchResult, Shelf,
    Track,
};

/// Catalog API-backed YouTube Music catalog implementing the [`Catalog`] port.
#[derive(Debug, Default)]
pub struct LiveCatalog {
    client: CatalogClient,
}

impl LiveCatalog {
    /// Creates a new catalog backed by default catalog API client.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an authenticated catalog injecting session cookies.
    pub fn with_cookies(cookies: &str) -> Self {
        Self::with_client(CatalogClient::with_cookies(cookies))
    }

    /// Creates a catalog injecting a catalog API client for testing or custom configuration.
    pub fn with_client(client: CatalogClient) -> Self {
        Self { client }
    }

    /// Updates session cookies by swapping the inner catalog API client.
    pub fn set_session_cookies(&mut self, cookies: Option<&str>) {
        let clean = cookies.filter(|c| !c.trim().is_empty());
        let current_locale = self.client.locale();
        self.client = match clean {
            Some(c) => CatalogClient::with_cookies(c),
            None => CatalogClient::new(),
        }
        .with_locale(current_locale);
    }

    /// Updates the interface locale on the inner catalog API client.
    pub fn set_locale(&mut self, locale: crate::i18n::Locale) {
        self.client.set_locale(locale);
    }

    /// Returns true if the catalog client has authenticated session credentials configured.
    pub fn is_authenticated(&self) -> bool {
        self.client.is_authenticated()
    }

    fn fetch_hits(&self, query_text: &str) -> SearchHits {
        self.client
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

fn raw_to_track(raw: RawTrack) -> Track {
    let mut track = Track::new(&raw.video_id, &raw.title, &raw.artist);
    if let Some(dur) = raw.duration_secs {
        track = track.with_duration(dur);
    }
    match raw.thumb_url {
        Some(url) => track.with_artwork(url),
        None => track,
    }
}

fn map_tracks(tracks: Vec<RawTrack>) -> impl Iterator<Item = SearchResult> {
    tracks.into_iter().map(|t| SearchResult::Track(raw_to_track(t)))
}

fn map_albums(albums: Vec<RawAlbum>) -> impl Iterator<Item = SearchResult> {
    albums
        .into_iter()
        .map(|a| SearchResult::Album(Album::new(&a.browse_id, &a.title, &a.artist, a.thumb_url)))
}

fn map_artists(artists: Vec<RawArtist>) -> impl Iterator<Item = SearchResult> {
    artists
        .into_iter()
        .map(|a| SearchResult::Artist(Artist::new(&a.browse_id, &a.name, a.thumb_url)))
}

fn map_playlists(playlists: Vec<RawPlaylist>) -> impl Iterator<Item = SearchResult> {
    playlists
        .into_iter()
        .map(|p| SearchResult::Playlist(Playlist::new(&p.browse_id, &p.title, 0, p.thumb_url)))
}

impl Catalog for LiveCatalog {
    /// Returns catalog hits for `query`. Network failures map to an empty vector.
    fn search(&self, query: &str) -> Vec<Track> {
        self.client.search_tracks(query).unwrap_or_default().into_iter().map(raw_to_track).collect()
    }

    /// Returns catalog hits matching the query text and filter. Network failures map to an empty vector.
    fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult> {
        Self::map_hits(self.fetch_hits(query.text()), query.filter())
    }

    /// Returns radio continuation tracks seeded by `seed_track_id`. Network failures map to an empty vector.
    fn radio(&self, seed_track_id: &str) -> Vec<Track> {
        let trimmed = seed_track_id.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        self.client.radio_tracks(trimmed).into_iter().map(raw_to_track).collect()
    }

    /// Returns home shelves. Network failures map to an empty vector.
    fn home_shelves(&self) -> Vec<Shelf> {
        self.client.home_shelves()
    }

    /// Returns explore shelves. Network failures map to an empty vector.
    fn explore_shelves(&self) -> Vec<Shelf> {
        self.client.explore_shelves()
    }

    /// Returns saved or owned playlists. Returns empty when unauthenticated or on failure.
    fn playlists(&self) -> Vec<Playlist> {
        self.client.library_playlists()
    }

    /// Returns saved albums. Returns empty when unauthenticated or on failure.
    fn saved_albums(&self) -> Vec<Album> {
        self.client.library_saved_albums()
    }

    /// Returns detail view for `browse_id` and `kind`. Network failures map to None.
    fn detail(&self, browse_id: &str, kind: DetailKind) -> Option<Detail> {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return None;
        }
        self.client.detail(trimmed, kind)
    }

    fn set_session_cookies(&mut self, cookies: Option<&str>) {
        self.set_session_cookies(cookies);
    }

    fn set_locale(&mut self, locale: crate::i18n::Locale) {
        self.set_locale(locale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SearchFilter, SearchResult};

    #[test]
    fn live_catalog_maps_hits_and_honors_filters() {
        use super::super::catalog_client::{
            RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits,
        };

        let hits = SearchHits {
            tracks: vec![
                RawTrack {
                    video_id: "v1".into(),
                    title: "T1".into(),
                    artist: "A1".into(),
                    duration_secs: None,
                    thumb_url: Some("https://artwork.test/v1".into()),
                },
                RawTrack {
                    video_id: "v2".into(),
                    title: "T2".into(),
                    artist: "A2".into(),
                    duration_secs: None,
                    thumb_url: None,
                },
            ],
            albums: vec![RawAlbum {
                browse_id: "b1".into(),
                title: "Alb".into(),
                artist: "Art".into(),
                thumb_url: Some("https://artwork.test/alb1".into()),
            }],
            artists: vec![RawArtist {
                browse_id: "ar1".into(),
                name: "ArtName".into(),
                thumb_url: Some("https://artwork.test/ar1".into()),
            }],
            playlists: vec![RawPlaylist {
                browse_id: "p1".into(),
                title: "PlayTitle".into(),
                thumb_url: None,
            }],
        };

        let all = LiveCatalog::map_hits(hits.clone(), SearchFilter::All);
        assert_eq!(all.len(), 5);
        assert_eq!(
            all[0],
            SearchResult::Track(
                Track::new("v1", "T1", "A1").with_artwork("https://artwork.test/v1")
            )
        );
        assert_eq!(all[1], SearchResult::Track(Track::new("v2", "T2", "A2")));
        assert_eq!(
            all[2],
            SearchResult::Album(Album::new(
                "b1",
                "Alb",
                "Art",
                Some("https://artwork.test/alb1".into())
            ))
        );
        assert_eq!(
            all[3],
            SearchResult::Artist(Artist::new(
                "ar1",
                "ArtName",
                Some("https://artwork.test/ar1".into())
            ))
        );
        assert_eq!(all[4], SearchResult::Playlist(Playlist::new("p1", "PlayTitle", 0, None)));

        for (filter, expected) in [
            (SearchFilter::Tracks, 2),
            (SearchFilter::Albums, 1),
            (SearchFilter::Artists, 1),
            (SearchFilter::Playlists, 1),
        ] {
            let mapped = LiveCatalog::map_hits(hits.clone(), filter);
            assert_eq!(mapped.len(), expected);
            assert!(mapped.iter().all(|item| item.kind() == filter));
        }
    }

    #[test]
    fn live_catalog_constructors_and_client_injection() {
        assert!(format!("{:?}", LiveCatalog::new()).contains("LiveCatalog"));
        assert!(format!("{:?}", LiveCatalog::default()).contains("LiveCatalog"));
        assert!(format!("{:?}", CatalogClient::new().into_catalog()).contains("LiveCatalog"));
        assert!(
            format!("{:?}", LiveCatalog::with_client(CatalogClient::new())).contains("LiveCatalog")
        );
    }

    #[test]
    fn live_catalog_unauthenticated_returns_empty_without_network() {
        let catalog = LiveCatalog::new();

        assert!(catalog.playlists().is_empty());
        assert!(catalog.saved_albums().is_empty());
    }

    #[test]
    fn live_catalog_with_cookies_constructor() {
        let catalog = LiveCatalog::with_cookies("fake_cookie_secret");
        assert!(format!("{catalog:?}").contains("LiveCatalog"));
    }

    #[test]
    fn live_catalog_maps_radio_tracks() {
        let raw = RawTrack {
            video_id: "radio-vid-1".into(),
            title: "Radio Title".into(),
            artist: "Radio Artist".into(),
            duration_secs: Some(225),
            thumb_url: Some("https://img.local/radio.jpg".into()),
        };
        let track = raw_to_track(raw);
        assert_eq!(track.id, "radio-vid-1");
        assert_eq!(track.title, "Radio Title");
        assert_eq!(track.artist, "Radio Artist");
        assert_eq!(track.duration_secs, Some(225));
        assert_eq!(track.artwork.as_deref(), Some("https://img.local/radio.jpg"));

        let catalog = LiveCatalog::new();
        assert!(catalog.radio("").is_empty());
        assert!(catalog.radio("   ").is_empty());
    }

    #[test]
    fn live_catalog_detail_blank_returns_none() {
        let catalog = LiveCatalog::new();
        assert_eq!(catalog.detail("", DetailKind::Album), None);
        assert_eq!(catalog.detail("   ", DetailKind::Artist), None);
        assert_eq!(catalog.detail("   ", DetailKind::Playlist), None);
    }

    #[test]
    fn live_catalog_set_session_cookies_swaps_authentication_state() {
        let mut catalog = LiveCatalog::new();
        assert!(!catalog.is_authenticated());

        catalog.set_session_cookies(Some("SID=auth_cookie"));
        assert!(catalog.is_authenticated());

        catalog.set_session_cookies(Some("   "));
        assert!(!catalog.is_authenticated());

        catalog.set_session_cookies(Some("HSID=auth2"));
        assert!(catalog.is_authenticated());

        catalog.set_session_cookies(None);
        assert!(!catalog.is_authenticated());
    }

    #[test]
    fn live_catalog_set_locale_propagates_and_persists_across_cookies() {
        let mut catalog = LiveCatalog::new();
        assert_eq!(catalog.client.locale(), crate::i18n::Locale::English);

        catalog.set_locale(crate::i18n::Locale::Spanish);
        assert_eq!(catalog.client.locale(), crate::i18n::Locale::Spanish);

        catalog.set_session_cookies(Some("SID=auth_cookie"));
        assert_eq!(catalog.client.locale(), crate::i18n::Locale::Spanish);

        catalog.set_session_cookies(None);
        assert_eq!(catalog.client.locale(), crate::i18n::Locale::Spanish);
    }
}
