//! HTTP client implementation for YouTube Music catalog endpoints.

use super::catalog::CatalogError;
use super::catalog_client::parse_search_all;
use super::catalog_live::{LiveCatalog, raw_to_track};
use super::catalog_model::{RawTrack, SearchHits};
use super::catalog_parse_album::{
    album_audio_playlist_id, has_music_video_rows, replace_album_tracks_with_audio,
};
use super::catalog_parse_continuation::parse_paginated_detail;
use super::catalog_parse_detail::parse_detail_all;
use super::catalog_parse_detail_track::now_epoch_secs;
use super::catalog_parse_explore::parse_explore_all;
use super::catalog_parse_home::parse_home_all;
use super::catalog_parse_library::{
    parse_library_albums, parse_library_artists, parse_library_playlists, parse_library_podcasts,
    parse_library_profiles, parse_library_songs, payload_is_logged_out,
};
use super::catalog_parse_playlist_create::parse_created_playlist_id;
use super::catalog_parse_radio::parse_radio_all;
use super::catalog_parse_search::parse_search_tracks;
use super::catalog_requests::{
    CATALOG_API_KEY, EXPLORE_BROWSE_ID, HISTORY_BROWSE_ID, HOME_BROWSE_ID,
    LIBRARY_ALBUMS_BROWSE_ID, LIBRARY_ARTISTS_BROWSE_ID, LIBRARY_PLAYLISTS_BROWSE_ID,
    LIBRARY_PODCASTS_BROWSE_ID, LIBRARY_PROFILES_BROWSE_ID, LIBRARY_SONGS_BROWSE_ID, browse_body,
    browse_body_with_params, browse_url, continuation_body, next_body, next_url,
    playlist_create_body, playlist_create_url, search_body, search_body_with_params,
    search_filter_params,
};
use crate::domain::{
    Album, Artist, Detail, DetailKind, Playlist, SearchQuery, SearchResult, Shelf, Track,
};
use crate::i18n::Locale;
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Blocking HTTP client for the catalog API.
pub struct CatalogClient {
    client: reqwest::blocking::Client,
    api_key: String,
    cookies: Option<String>,
    locale: Locale,
}

impl std::fmt::Debug for CatalogClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CatalogClient")
            .field("api_key", &self.api_key)
            .field("has_cookies", &self.cookies.is_some())
            .field("locale", &self.locale)
            .finish()
    }
}

/// Origin the catalog session signature is bound to.
const CATALOG_ORIGIN: &str = "https://music.youtube.com";

fn cookie_value<'a>(cookies: &'a str, name: &str) -> Option<&'a str> {
    cookies
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
}

/// Builds the origin-bound `SAPISIDHASH` authorization the catalog requires for signed-in requests.
fn sapisid_authorization(cookies: &str, timestamp: u64) -> Option<String> {
    let sapisid = cookie_value(cookies, "SAPISID")?;
    let digest = Sha1::digest(format!("{timestamp} {sapisid} {CATALOG_ORIGIN}").as_bytes());
    Some(format!("SAPISIDHASH {timestamp}_{digest:x}"))
}

fn has_grid_items(section: &Value) -> bool {
    const CANDIDATES: &[&str] = &[
        "/gridRenderer/items",
        "/musicGridRenderer/items",
        "/musicShelfRenderer/contents",
        "/itemSectionRenderer/contents",
        "/gridRenderer",
        "/musicGridRenderer",
        "/musicShelfRenderer",
        "/itemSectionRenderer",
    ];
    CANDIDATES.iter().any(|path| section.pointer(path).is_some())
}

fn has_library_section(value: &Value) -> bool {
    super::catalog_parse_detail::detail_sections(value)
        .map_or_else(|| has_grid_items(value), |sections| sections.iter().any(has_grid_items))
}

fn validate_library_result<T>(items: Vec<T>, payload: &Value) -> Result<Vec<T>, CatalogError> {
    if !items.is_empty() || has_library_section(payload) {
        Ok(items)
    } else {
        Err(CatalogError::Malformed)
    }
}

fn validate_library_payload<T>(items: Vec<T>, payload: &Value) -> Result<Vec<T>, CatalogError> {
    if payload_is_logged_out(payload) {
        return Err(CatalogError::Unauthenticated);
    }
    validate_library_result(items, payload)
}

fn has_search_section(value: &Value) -> bool {
    const CANDIDATES: &[&str] = &[
        "/contents/tabbedSearchResultsRenderer",
        "/tabbedSearchResultsRenderer",
        "/contents/sectionListRenderer",
        "/sectionListRenderer",
        "/musicShelfRenderer",
        "/itemSectionRenderer",
    ];
    CANDIDATES.iter().any(|path| value.pointer(path).is_some())
}

fn validate_search_result<T>(items: Vec<T>, payload: &Value) -> Result<Vec<T>, CatalogError> {
    if !items.is_empty() || has_search_section(payload) {
        Ok(items)
    } else {
        Err(CatalogError::Malformed)
    }
}

fn has_radio_section(value: &Value) -> bool {
    const CANDIDATES: &[&str] = &[
        "/contents/singleColumnMusicWatchNextResultsRenderer",
        "/singleColumnMusicWatchNextResultsRenderer",
        "/tabbedRenderer/watchNextTabbedResultsRenderer",
        "/watchNextTabbedResultsRenderer",
        "/musicQueueRenderer",
        "/playlistPanelRenderer",
    ];
    CANDIDATES.iter().any(|path| value.pointer(path).is_some())
}

fn validate_radio_result<T>(items: Vec<T>, payload: &Value) -> Result<Vec<T>, CatalogError> {
    if !items.is_empty() || has_radio_section(payload) {
        Ok(items)
    } else {
        Err(CatalogError::Malformed)
    }
}

fn has_explore_section(value: &Value) -> bool {
    const CANDIDATES: &[&str] = &[
        "/contents/singleColumnBrowseResultsRenderer",
        "/singleColumnBrowseResultsRenderer",
        "/sectionListRenderer",
        "/musicCarouselShelfRenderer",
        "/musicShelfRenderer",
        "/gridRenderer",
        "/itemSectionRenderer",
    ];
    CANDIDATES.iter().any(|path| value.pointer(path).is_some())
}

fn validate_explore_result<T>(items: Vec<T>, payload: &Value) -> Result<Vec<T>, CatalogError> {
    if !items.is_empty() || has_explore_section(payload) {
        Ok(items)
    } else {
        Err(CatalogError::Malformed)
    }
}

impl CatalogClient {
    /// Creates a new catalog API client with a 10-second request timeout (anonymous).
    pub fn new() -> Self {
        Self::with_optional_cookies(None)
    }

    /// Creates an authenticated catalog API client with stored session cookies.
    ///
    /// Empty or whitespace-only cookie strings are treated as unauthenticated (`None`).
    pub fn with_cookies(cookies: &str) -> Self {
        let trimmed = cookies.trim();
        let opt = if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) };
        Self::with_optional_cookies(opt)
    }

    /// Configures the active locale for requests.
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    /// Sets the active locale for subsequent requests.
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// Returns the currently configured request locale.
    pub fn locale(&self) -> Locale {
        self.locale
    }

    fn with_optional_cookies(cookies: Option<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client, api_key: CATALOG_API_KEY.to_owned(), cookies, locale: Locale::default() }
    }

    /// Returns true if this client has credentials configured for authenticated requests.
    pub fn is_authenticated(&self) -> bool {
        self.cookies.is_some()
    }

    /// Adds the stored session and its origin-bound signature to a catalog request.
    ///
    /// Anonymous requests pass through unchanged. A stored session without `SAPISID`
    /// cannot be signed, so the request stays anonymous and the caller sees an empty
    /// authenticated payload; warn instead of silently degrading.
    fn signed_request(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        let Some(cookie) = self.cookies.as_deref() else {
            return request;
        };
        let request = request.header("Cookie", cookie).header("X-Origin", CATALOG_ORIGIN);
        let timestamp = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(elapsed) => elapsed.as_secs(),
            Err(_) => {
                tracing::warn!("system clock precedes the unix epoch; catalog request is unsigned");
                return request;
            }
        };
        match sapisid_authorization(cookie, timestamp) {
            Some(authorization) => request.header("Authorization", authorization),
            None => {
                tracing::warn!(
                    "stored session has no SAPISID cookie; catalog request is anonymous"
                );
                request
            }
        }
    }

    fn execute_search_payload(&self, body: &Value) -> anyhow::Result<Value> {
        let url = format!(
            "https://music.youtube.com/youtubei/v1/search?key={}&prettyPrint=false",
            self.api_key
        );
        let response = self.signed_request(self.client.post(&url)).json(body).send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("CatalogClient search request failed with status: {status}");
        }
        let payload: Value = response.json()?;
        Ok(payload)
    }

    /// Executes search request and returns the unparsed JSON response payload.
    pub fn search_payload(&self, query: &str) -> anyhow::Result<Value> {
        let body = search_body(query, self.locale);
        self.execute_search_payload(&body)
    }

    /// Executes filtered search request and returns the unparsed JSON response payload.
    pub fn search_payload_filtered(&self, query: &SearchQuery) -> anyhow::Result<Value> {
        let params = search_filter_params(query.filter());
        let body = search_body_with_params(query.text(), self.locale, params);
        self.execute_search_payload(&body)
    }

    /// Performs search for `query` and returns parsed tracks.
    pub fn search(&self, query: &str) -> Result<Vec<Track>, CatalogError> {
        let payload = self.search_payload(query).map_err(|_| CatalogError::Transport)?;
        let tracks: Vec<Track> =
            parse_search_tracks(&payload).into_iter().map(raw_to_track).collect();
        validate_search_result(tracks, &payload)
    }

    /// Performs filtered search and returns domain search results.
    pub fn search_filtered(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, CatalogError> {
        let payload = self.search_payload_filtered(query).map_err(|_| CatalogError::Transport)?;
        let hits = parse_search_all(&payload);
        let results = LiveCatalog::map_hits(hits, query.filter());
        validate_search_result(results, &payload)
    }

    /// Performs search for `query` and returns parsed raw tracks.
    pub fn search_tracks(&self, query: &str) -> anyhow::Result<Vec<RawTrack>> {
        let payload = self.search_payload(query)?;
        Ok(parse_search_tracks(&payload))
    }

    /// Performs search for `query` and returns grouped search hits across all shelves.
    pub fn search_all(&self, query: &str) -> anyhow::Result<SearchHits> {
        self.search_payload(query).map(|p| parse_search_all(&p))
    }

    /// Executes next endpoint request for radio and returns the unparsed JSON response payload.
    pub fn next_payload(&self, video_id: &str) -> anyhow::Result<Value> {
        let url = next_url();
        let body = next_body(video_id, self.locale);
        let response = self.signed_request(self.client.post(&url)).json(&body).send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("CatalogClient next request failed with status: {status}");
        }
        let payload: Value = response.json()?;
        Ok(payload)
    }

    /// Queries the next endpoint for `video_id` and returns parsed radio tracks.
    pub fn radio(&self, video_id: &str) -> Result<Vec<Track>, CatalogError> {
        let trimmed = video_id.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let payload = self.next_payload(trimmed).map_err(|_| CatalogError::Transport)?;
        let tracks: Vec<Track> = parse_radio_all(&payload).into_iter().map(raw_to_track).collect();
        validate_radio_result(tracks, &payload)
    }

    /// Executes browse endpoint request for `browse_id` with optional `params`, returning unparsed JSON.
    pub fn browse_payload_with_params(
        &self,
        browse_id: &str,
        params: Option<&str>,
    ) -> anyhow::Result<Value> {
        let url = browse_url();
        let body = browse_body_with_params(browse_id, params, self.locale);
        let response = self.signed_request(self.client.post(&url)).json(&body).send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("CatalogClient browse request failed with status: {status}");
        }
        let payload: Value = response.json()?;
        Ok(payload)
    }

    /// Executes browse endpoint request for `browse_id` and returns the unparsed JSON response payload.
    pub fn browse_payload(&self, browse_id: &str) -> anyhow::Result<Value> {
        self.browse_payload_with_params(browse_id, None)
    }

    /// Queries the browse endpoint for category shelves with `browse_id` and optional `params`.
    pub fn category_shelves(
        &self,
        browse_id: &str,
        params: Option<&str>,
    ) -> Result<Vec<Shelf>, CatalogError> {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let payload = self
            .browse_payload_with_params(trimmed, params)
            .map_err(|_| CatalogError::Transport)?;
        let shelves = parse_explore_all(&payload);
        validate_explore_result(shelves, &payload)
    }

    /// Queries the browse endpoint for home shelves and returns parsed shelves.
    /// Returns an empty vector on any network or parse failure.
    pub fn home_shelves(&self) -> Vec<Shelf> {
        self.browse_payload(HOME_BROWSE_ID).map_or_else(|_| Vec::new(), |p| parse_home_all(&p))
    }

    /// Queries the browse endpoint for explore shelves and returns parsed shelves.
    pub fn explore_shelves(&self) -> Result<Vec<Shelf>, CatalogError> {
        let payload =
            self.browse_payload(EXPLORE_BROWSE_ID).map_err(|_| CatalogError::Transport)?;
        let shelves = parse_explore_all(&payload);
        validate_explore_result(shelves, &payload)
    }

    fn detail_payload(&self, id: &str) -> anyhow::Result<Value> {
        if id == HISTORY_BROWSE_ID {
            self.library_browse_payload(id)
        } else {
            self.browse_payload(id)
        }
    }

    /// Queries the browse endpoint for `browse_id` and returns parsed detail view of `kind`.
    pub fn detail(
        &self,
        browse_id: &str,
        kind: DetailKind,
    ) -> Result<Option<Detail>, CatalogError> {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if trimmed == HISTORY_BROWSE_ID && !self.is_authenticated() {
            return Err(CatalogError::Unauthenticated);
        }
        let payload = self.detail_payload(trimmed).map_err(|_| CatalogError::Transport)?;
        let detail = parse_detail_all(&payload, kind).ok_or(CatalogError::Malformed)?;
        Ok(Some(self.with_album_audio_tracks(detail, kind, &payload)))
    }

    /// Replaces a music-video album tracklist with its canonical audio playlist tracks.
    ///
    /// Album rows whose provider supplies official videos point at the video ids, so streaming them
    /// would play the video edit. The album payload also carries the release audio playlist
    /// (`OLAK5uy_...`); browsing it yields the art tracks with the album durations. A failed or empty
    /// audio playlist keeps the catalog rows and is reported as a degraded response.
    fn with_album_audio_tracks(&self, detail: Detail, kind: DetailKind, payload: &Value) -> Detail {
        if kind != DetailKind::Album || !has_music_video_rows(&detail) {
            return detail;
        }
        let Some(playlist_id) = album_audio_playlist_id(payload) else {
            return detail;
        };
        let Ok(audio) = self.browse_payload(&format!("VL{playlist_id}")) else {
            tracing::warn!(
                playlist_id = %playlist_id,
                "album audio playlist request failed; keeping catalog rows"
            );
            return detail;
        };
        replace_album_tracks_with_audio(detail, &audio, now_epoch_secs())
    }

    /// Queries the browse endpoint for `browse_id` and merges its track-list continuation
    /// pages up to `max_pages`.
    pub fn detail_paginated(
        &self,
        browse_id: &str,
        kind: DetailKind,
        max_pages: usize,
    ) -> Result<Option<Detail>, CatalogError> {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if trimmed == HISTORY_BROWSE_ID && !self.is_authenticated() {
            return Err(CatalogError::Unauthenticated);
        }
        let payload = self.detail_payload(trimmed).map_err(|_| CatalogError::Transport)?;
        Ok(parse_paginated_detail(&payload, kind, max_pages, |token| {
            self.continuation_payload(token).ok()
        }))
    }

    /// Executes a browse request that follows a track-list continuation token.
    fn continuation_payload(&self, token: &str) -> anyhow::Result<Value> {
        let url = browse_url();
        let body = continuation_body(token, self.locale);
        let response = self.signed_request(self.client.post(&url)).json(&body).send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("CatalogClient continuation request failed with status: {status}");
        }
        let payload: Value = response.json()?;
        Ok(payload)
    }

    /// Executes authenticated browse request for `browse_id`.
    pub fn library_browse_payload(&self, browse_id: &str) -> anyhow::Result<Value> {
        if self.cookies.is_none() {
            anyhow::bail!("Unauthenticated CatalogClient cannot request library endpoint");
        }
        let url = browse_url();
        let body = browse_body(browse_id, self.locale);
        let response = self.signed_request(self.client.post(&url)).json(&body).send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("CatalogClient library browse request failed with status: {status}");
        }
        let payload: Value = response.json()?;
        Ok(payload)
    }

    /// Queries the browse endpoint for user's library playlists.
    pub fn library_playlists(&self) -> Result<Vec<Playlist>, CatalogError> {
        if !self.is_authenticated() {
            return Err(CatalogError::Unauthenticated);
        }
        let payload = self
            .library_browse_payload(LIBRARY_PLAYLISTS_BROWSE_ID)
            .map_err(|_| CatalogError::Transport)?;
        let playlists = parse_library_playlists(&payload);
        validate_library_payload(playlists, &payload)
    }

    /// Queries the browse endpoint for user's saved albums.
    pub fn library_saved_albums(&self) -> Result<Vec<Album>, CatalogError> {
        if !self.is_authenticated() {
            return Err(CatalogError::Unauthenticated);
        }
        let payload = self
            .library_browse_payload(LIBRARY_ALBUMS_BROWSE_ID)
            .map_err(|_| CatalogError::Transport)?;
        let albums = parse_library_albums(&payload);
        validate_library_payload(albums, &payload)
    }

    /// Queries the browse endpoint for the user's followed artists.
    pub fn library_artists(&self) -> Result<Vec<Artist>, CatalogError> {
        if !self.is_authenticated() {
            return Err(CatalogError::Unauthenticated);
        }
        let payload = self
            .library_browse_payload(LIBRARY_ARTISTS_BROWSE_ID)
            .map_err(|_| CatalogError::Transport)?;
        let artists = parse_library_artists(&payload);
        validate_library_payload(artists, &payload)
    }

    /// Queries the browse endpoint for the user's liked songs.
    pub fn library_songs(&self) -> Result<Vec<Track>, CatalogError> {
        if !self.is_authenticated() {
            return Err(CatalogError::Unauthenticated);
        }
        let payload = self
            .library_browse_payload(LIBRARY_SONGS_BROWSE_ID)
            .map_err(|_| CatalogError::Transport)?;
        let songs = parse_library_songs(&payload);
        validate_library_payload(songs, &payload)
    }

    /// Queries the browse endpoint for the user's saved podcasts.
    pub fn library_podcasts(&self) -> Result<Vec<Playlist>, CatalogError> {
        if !self.is_authenticated() {
            return Err(CatalogError::Unauthenticated);
        }
        let payload = self
            .library_browse_payload(LIBRARY_PODCASTS_BROWSE_ID)
            .map_err(|_| CatalogError::Transport)?;
        let podcasts = parse_library_podcasts(&payload);
        validate_library_payload(podcasts, &payload)
    }

    /// Queries the browse endpoint for the user's followed profiles.
    pub fn library_profiles(&self) -> Result<Vec<Artist>, CatalogError> {
        if !self.is_authenticated() {
            return Err(CatalogError::Unauthenticated);
        }
        let payload = self
            .library_browse_payload(LIBRARY_PROFILES_BROWSE_ID)
            .map_err(|_| CatalogError::Transport)?;
        let profiles = parse_library_profiles(&payload);
        validate_library_payload(profiles, &payload)
    }

    /// Creates a new private playlist in the user's account and returns its playlist id.
    pub fn create_playlist(&self, title: &str) -> Result<String, CatalogError> {
        if !self.is_authenticated() {
            return Err(CatalogError::Unauthenticated);
        }
        let url = playlist_create_url();
        let body = playlist_create_body(title, self.locale);
        let response = self
            .signed_request(self.client.post(&url))
            .json(&body)
            .send()
            .map_err(|_| CatalogError::Transport)?;
        if !response.status().is_success() {
            return Err(CatalogError::Transport);
        }
        let payload: Value = response.json().map_err(|_| CatalogError::Malformed)?;
        parse_created_playlist_id(&payload).ok_or(CatalogError::Malformed)
    }

    /// Converts this catalog API client into a [`LiveCatalog`] adapter.
    pub fn into_catalog(self) -> super::catalog_live::LiveCatalog {
        super::catalog_live::LiveCatalog::with_client(self)
    }
}

impl Default for CatalogClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_value_reads_named_pairs() {
        let cookies = "SID=abc; SAPISID=xyz;  SSID=q";
        assert_eq!(cookie_value(cookies, "SAPISID"), Some("xyz"));
        assert_eq!(cookie_value(cookies, "SID"), Some("abc"));
        assert_eq!(cookie_value(cookies, "MISSING"), None);
        assert_eq!(cookie_value("SAPISID=  ", "SAPISID"), None);
    }

    #[test]
    fn sapisid_authorization_signs_the_catalog_origin() {
        let cookies = "SID=abc; SAPISID=Y8DaQK1-VRZxrpWu/AOBQYOHADPvQdIhvU; SSID=q";
        assert_eq!(
            sapisid_authorization(cookies, 1_700_000_000).as_deref(),
            Some("SAPISIDHASH 1700000000_50bc7ad615487c9316ec93da978bcb26411ca453")
        );
        assert_eq!(sapisid_authorization("SID=abc", 1_700_000_000), None);
    }

    #[test]
    fn catalog_client_detail_blank_id_returns_none() {
        let client = CatalogClient::new();
        assert_eq!(client.detail("", DetailKind::Album), Ok(None));
        assert_eq!(client.detail("   ", DetailKind::Artist), Ok(None));
    }

    #[test]
    fn catalog_client_locale_configuration() {
        let client = CatalogClient::new();
        assert_eq!(client.locale(), Locale::English);

        let es_client = CatalogClient::new().with_locale(Locale::Spanish);
        assert_eq!(es_client.locale(), Locale::Spanish);

        let mut mut_client = CatalogClient::new();
        mut_client.set_locale(Locale::Spanish);
        assert_eq!(mut_client.locale(), Locale::Spanish);
    }

    #[test]
    fn catalog_client_cookies_masking_and_unauthenticated_guard() {
        assert!(!CatalogClient::new().is_authenticated());
        assert!(!CatalogClient::with_cookies("   ").is_authenticated());
        let client = CatalogClient::with_cookies("secret-cookie-val");
        assert!(client.is_authenticated());
        let debug_str = format!("{client:?}");
        assert!(!debug_str.contains("secret-cookie-val"));
        assert!(debug_str.contains("has_cookies: true"));

        let anon = CatalogClient::new();
        assert_eq!(anon.library_playlists(), Err(CatalogError::Unauthenticated));
        assert_eq!(anon.library_saved_albums(), Err(CatalogError::Unauthenticated));
    }

    #[test]
    fn unauthenticated_library_is_distinct_from_empty() {
        let anon = CatalogClient::new();
        assert_eq!(anon.library_playlists(), Err(CatalogError::Unauthenticated));
        assert_eq!(anon.library_saved_albums(), Err(CatalogError::Unauthenticated));
    }

    #[test]
    fn empty_library_section_is_ok_empty() {
        let empty_payload = serde_json::json!({
            "sectionListRenderer": {
                "contents": [{
                    "gridRenderer": {
                        "items": []
                    }
                }]
            }
        });
        let playlists = parse_library_playlists(&empty_payload);
        assert_eq!(validate_library_result(playlists, &empty_payload), Ok(Vec::new()));

        let albums = parse_library_albums(&empty_payload);
        assert_eq!(validate_library_result(albums, &empty_payload), Ok(Vec::new()));

        let malformed_payload = serde_json::json!({});
        let malformed_playlists = parse_library_playlists(&malformed_payload);
        assert_eq!(
            validate_library_result(malformed_playlists, &malformed_payload),
            Err(CatalogError::Malformed)
        );
    }

    #[test]
    fn logged_out_library_payload_is_unauthenticated() {
        let raw = include_str!("../../tests/fixtures/catalog_library_logged_out.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");

        let playlists = parse_library_playlists(&payload);
        assert_eq!(
            validate_library_payload(playlists, &payload),
            Err(CatalogError::Unauthenticated)
        );

        let albums = parse_library_albums(&payload);
        assert_eq!(validate_library_payload(albums, &payload), Err(CatalogError::Unauthenticated));
    }

    #[test]
    fn empty_search_section_is_ok_empty() {
        let empty_payload = serde_json::json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": []
                }
            }
        });
        let tracks: Vec<Track> = Vec::new();
        assert_eq!(validate_search_result(tracks, &empty_payload), Ok(Vec::new()));

        let malformed_payload = serde_json::json!({});
        let malformed_tracks: Vec<Track> = Vec::new();
        assert_eq!(
            validate_search_result(malformed_tracks, &malformed_payload),
            Err(CatalogError::Malformed)
        );
    }

    #[test]
    fn catalog_client_history_detail_unauthenticated_returns_unauthenticated() {
        let client = CatalogClient::new();
        assert_eq!(
            client.detail(HISTORY_BROWSE_ID, DetailKind::Playlist),
            Err(CatalogError::Unauthenticated)
        );
    }

    #[test]
    fn empty_explore_section_is_ok_empty() {
        let empty_payload = serde_json::json!({
            "sectionListRenderer": { "contents": [] }
        });
        let shelves: Vec<Shelf> = Vec::new();
        assert_eq!(validate_explore_result(shelves, &empty_payload), Ok(Vec::new()));

        let malformed_payload = serde_json::json!({});
        let malformed_shelves: Vec<Shelf> = Vec::new();
        assert_eq!(
            validate_explore_result(malformed_shelves, &malformed_payload),
            Err(CatalogError::Malformed)
        );
    }

    #[test]
    fn empty_radio_section_is_ok_empty() {
        let empty_payload = serde_json::json!({
            "playlistPanelRenderer": { "contents": [] }
        });
        let tracks: Vec<Track> = Vec::new();
        assert_eq!(validate_radio_result(tracks, &empty_payload), Ok(Vec::new()));

        let malformed_payload = serde_json::json!({});
        let malformed_tracks: Vec<Track> = Vec::new();
        assert_eq!(
            validate_radio_result(malformed_tracks, &malformed_payload),
            Err(CatalogError::Malformed)
        );
    }

    #[test]
    fn catalog_client_category_shelves_parses_minimal_payload_and_guards() {
        let client = CatalogClient::new();
        assert_eq!(client.category_shelves("", None), Ok(Vec::new()));
        assert_eq!(client.category_shelves("   ", Some("p")), Ok(Vec::new()));

        let payload = serde_json::json!({
            "sectionListRenderer": { "contents": [{
                "musicCarouselShelfRenderer": {
                    "header": { "musicCarouselShelfBasicHeaderRenderer": { "title": { "runs": [{ "text": "Calm" }] } } },
                    "contents": [{ "musicTwoRowItemRenderer": {
                        "title": { "runs": [{ "text": "Piano" }] },
                        "navigationEndpoint": { "browseEndpoint": { "browseId": "VL_piano" } }
                    }}]
                }
            }] }
        });
        let shelves = parse_explore_all(&payload);
        assert_eq!(shelves.len(), 1);
        assert_eq!(shelves[0].title, "Calm");
        assert_eq!(
            shelves[0].items[0],
            crate::domain::SearchResult::Playlist(Playlist::new("VL_piano", "Piano", 0, None))
        );
        assert!(parse_explore_all(&serde_json::json!({})).is_empty());
        assert!(parse_explore_all(&serde_json::json!({"error": {"code": 404}})).is_empty());
    }

    #[test]
    fn catalog_client_create_playlist_unauthenticated_returns_unauthenticated() {
        assert_eq!(CatalogClient::new().create_playlist("Mix"), Err(CatalogError::Unauthenticated));
    }
}
