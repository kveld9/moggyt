//! Production catalog adapter implementing the [`Catalog`] port via [`CatalogClient`].

use super::catalog::{Catalog, CatalogError};
use super::catalog_client::{
    CatalogClient, RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits,
};
use super::lyrics_lrcred::LrcRedClient;
use super::lyrics_search::{LYRICS_HIT_LIMIT, rank_lyrics_candidates};
use crate::domain::{
    Album, Artist, Detail, DetailKind, LyricsSnippet, Playlist, SearchFilter, SearchQuery,
    SearchResult, Shelf, Track,
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

pub(crate) fn raw_to_track(raw: RawTrack) -> Track {
    let mut track = Track::new(&raw.video_id, &raw.title, &raw.artist);
    if let Some(dur) = raw.duration_secs {
        track = track.with_duration(dur);
    }
    if let Some(video_type) = raw.video_type {
        track = track.with_video_type(video_type);
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
    albums.into_iter().map(|a| {
        SearchResult::Album(
            Album::new(&a.browse_id, &a.title, &a.artist, a.thumb_url)
                .with_artist_id(a.artist_id.clone()),
        )
    })
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
    /// Resolves a lyric fragment through lrc.red and the catalog, ranked by metadata fit.
    fn search_by_lyrics(&self, snippet: &LyricsSnippet) -> Result<Vec<Track>, CatalogError> {
        let hits = LrcRedClient::new().lookup_hits(snippet.as_str()).unwrap_or_default();
        let mut matches: Vec<Track> = Vec::new();
        for hit in hits.iter().take(LYRICS_HIT_LIMIT) {
            let query = format!("{} {}", hit.title, hit.artist);
            let Ok(candidates) = self.client.search(&query) else {
                continue;
            };
            for track in rank_lyrics_candidates(hit, &candidates) {
                if !matches.iter().any(|known| known.id == track.id) {
                    matches.push(track);
                }
            }
        }
        Ok(matches)
    }

    /// Returns catalog hits for `query`.
    fn search(&self, query: &str) -> Result<Vec<Track>, CatalogError> {
        self.client.search(query)
    }

    /// Returns catalog hits matching the query text and filter.
    fn search_filtered(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, CatalogError> {
        self.client.search_filtered(query)
    }

    /// Returns radio continuation tracks seeded by `seed_track_id`.
    fn radio(&self, seed_track_id: &str) -> Result<Vec<Track>, CatalogError> {
        let trimmed = seed_track_id.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        self.client.radio(trimmed)
    }

    /// Returns home shelves. Network failures map to an empty vector.
    fn home_shelves(&self) -> Vec<Shelf> {
        self.client.home_shelves()
    }

    /// Returns explore shelves.
    fn explore_shelves(&self) -> Result<Vec<Shelf>, CatalogError> {
        self.client.explore_shelves()
    }

    /// Returns category shelves for `browse_id` and optional `params`.
    fn category_shelves(
        &self,
        browse_id: &str,
        params: Option<&str>,
    ) -> Result<Vec<Shelf>, CatalogError> {
        self.client.category_shelves(browse_id, params)
    }

    /// Returns saved or owned playlists.
    fn playlists(&self) -> Result<Vec<Playlist>, CatalogError> {
        self.client.library_playlists()
    }

    /// Returns saved albums.
    fn saved_albums(&self) -> Result<Vec<Album>, CatalogError> {
        self.client.library_saved_albums()
    }

    /// Returns detail view for `browse_id` and `kind`.
    fn detail(&self, browse_id: &str, kind: DetailKind) -> Result<Option<Detail>, CatalogError> {
        self.client.detail(browse_id, kind)
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
                    video_type: None,
                },
                RawTrack {
                    video_id: "v2".into(),
                    title: "T2".into(),
                    artist: "A2".into(),
                    duration_secs: None,
                    thumb_url: None,
                    video_type: None,
                },
            ],
            albums: vec![RawAlbum {
                browse_id: "b1".into(),
                title: "Alb".into(),
                artist: "Art".into(),
                thumb_url: Some("https://artwork.test/alb1".into()),
                artist_id: None,
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
    fn live_catalog_unauthenticated_returns_unauthenticated_error() {
        let catalog = LiveCatalog::new();

        assert_eq!(catalog.playlists(), Err(CatalogError::Unauthenticated));
        assert_eq!(catalog.saved_albums(), Err(CatalogError::Unauthenticated));
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
            video_type: Some(crate::domain::MusicVideoType::Atv),
        };
        let track = raw_to_track(raw);
        assert_eq!(track.id, "radio-vid-1");
        assert_eq!(track.title, "Radio Title");
        assert_eq!(track.artist, "Radio Artist");
        assert_eq!(track.duration_secs, Some(225));
        assert_eq!(track.artwork.as_deref(), Some("https://img.local/radio.jpg"));
        assert_eq!(track.video_type, Some(crate::domain::MusicVideoType::Atv));

        let catalog = LiveCatalog::new();
        assert_eq!(catalog.radio(""), Ok(Vec::new()));
        assert_eq!(catalog.radio("   "), Ok(Vec::new()));
    }

    #[test]
    fn live_catalog_detail_blank_returns_none() {
        let catalog = LiveCatalog::new();
        assert_eq!(catalog.detail("", DetailKind::Album), Ok(None));
        assert_eq!(catalog.detail("   ", DetailKind::Artist), Ok(None));
        assert_eq!(catalog.detail("   ", DetailKind::Playlist), Ok(None));
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
