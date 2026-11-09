//! HTTP client implementation for YouTube Music catalog endpoints.

use super::catalog::CatalogError;
use super::catalog_client::parse_search_all;
use super::catalog_live::{LiveCatalog, raw_to_track};
use super::catalog_model::{RawTrack, SearchHits};
use super::catalog_parse_detail::parse_detail_all;
use super::catalog_parse_explore::parse_explore_all;
use super::catalog_parse_home::parse_home_all;
use super::catalog_parse_library::{parse_library_albums, parse_library_playlists};
use super::catalog_parse_radio::parse_radio_all;
use super::catalog_parse_search::parse_search_tracks;
use super::catalog_requests::{
    CATALOG_API_KEY, EXPLORE_BROWSE_ID, HISTORY_BROWSE_ID, HOME_BROWSE_ID,
    LIBRARY_ALBUMS_BROWSE_ID, LIBRARY_PLAYLISTS_BROWSE_ID, browse_body, browse_body_with_params,
    browse_url, next_body, next_url, search_body, search_body_with_params, search_filter_params,
};
use crate::domain::{Album, Detail, DetailKind, Playlist, SearchQuery, SearchResult, Shelf, Track};
use crate::i18n::Locale;
use serde_json::Value;
use std::time::Duration;

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

    fn execute_search_payload(&self, body: &Value) -> anyhow::Result<Value> {
        let url = format!(
            "https://music.youtube.com/youtubei/v1/search?key={}&prettyPrint=false",
            self.api_key
        );
        let response = self.client.post(&url).json(body).send()?;
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
        let response = self.client.post(&url).json(&body).send()?;
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
        let response = self.client.post(&url).json(&body).send()?;
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
        parse_detail_all(&payload, kind).map(Some).ok_or(CatalogError::Malformed)
    }

    /// Executes authenticated browse request for `browse_id`.
    pub fn library_browse_payload(&self, browse_id: &str) -> anyhow::Result<Value> {
        let Some(cookie) = &self.cookies else {
            anyhow::bail!("Unauthenticated CatalogClient cannot request library endpoint");
        };
        let url = browse_url();
        let body = browse_body(browse_id, self.locale);
        let response = self.client.post(&url).header("Cookie", cookie).json(&body).send()?;
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
        validate_library_result(playlists, &payload)
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
        validate_library_result(albums, &payload)
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
}
