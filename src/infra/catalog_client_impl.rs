//! HTTP client implementation for YouTube Music catalog endpoints.

use super::catalog_model::{RawTrack, SearchHits};
use super::catalog_parse_detail::parse_detail_all;
use super::catalog_parse_explore::parse_explore_all;
use super::catalog_parse_home::parse_home_all;
use super::catalog_parse_library::{parse_library_albums, parse_library_playlists};
use super::catalog_parse_radio::parse_radio_all;
use super::catalog_parse_search::{parse_search_all, parse_search_tracks};
use super::catalog_requests::{
    CATALOG_API_KEY, EXPLORE_BROWSE_ID, HISTORY_BROWSE_ID, HOME_BROWSE_ID,
    LIBRARY_ALBUMS_BROWSE_ID, LIBRARY_PLAYLISTS_BROWSE_ID, browse_body, browse_body_with_params,
    browse_url, next_body, next_url, search_body, search_body_with_params, search_filter_params,
};
use crate::domain::{Album, Detail, DetailKind, Playlist, SearchQuery, Shelf};
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
    /// Returns an empty vector on any network or parse failure.
    pub fn radio_tracks(&self, video_id: &str) -> Vec<RawTrack> {
        let trimmed = video_id.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        self.next_payload(trimmed).map_or_else(|_| Vec::new(), |p| parse_radio_all(&p))
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
    /// Returns an empty vector on blank id, network error, or parse failure.
    pub fn category_shelves(&self, browse_id: &str, params: Option<&str>) -> Vec<Shelf> {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        self.browse_payload_with_params(trimmed, params)
            .map_or_else(|_| Vec::new(), |p| parse_explore_all(&p))
    }

    /// Queries the browse endpoint for home shelves and returns parsed shelves.
    /// Returns an empty vector on any network or parse failure.
    pub fn home_shelves(&self) -> Vec<Shelf> {
        self.browse_payload(HOME_BROWSE_ID).map_or_else(|_| Vec::new(), |p| parse_home_all(&p))
    }

    /// Queries the browse endpoint for explore shelves and returns parsed shelves.
    /// Returns an empty vector on any network or parse failure.
    pub fn explore_shelves(&self) -> Vec<Shelf> {
        self.browse_payload(EXPLORE_BROWSE_ID)
            .map_or_else(|_| Vec::new(), |p| parse_explore_all(&p))
    }

    fn detail_payload(&self, id: &str) -> anyhow::Result<Value> {
        if id == HISTORY_BROWSE_ID {
            self.library_browse_payload(id)
        } else {
            self.browse_payload(id)
        }
    }

    /// Queries the browse endpoint for `browse_id` and returns parsed detail view of `kind`.
    /// Returns `None` on blank id, network error, or parse failure.
    pub fn detail(&self, browse_id: &str, kind: DetailKind) -> Option<Detail> {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return None;
        }
        let payload = self.detail_payload(trimmed).ok()?;
        parse_detail_all(&payload, kind)
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
    /// Returns an empty vector when unauthenticated or on network/parse failure.
    pub fn library_playlists(&self) -> Vec<Playlist> {
        if !self.is_authenticated() {
            return Vec::new();
        }
        self.library_browse_payload(LIBRARY_PLAYLISTS_BROWSE_ID)
            .map_or_else(|_| Vec::new(), |p| parse_library_playlists(&p))
    }

    /// Queries the browse endpoint for user's saved albums.
    /// Returns an empty vector when unauthenticated or on network/parse failure.
    pub fn library_saved_albums(&self) -> Vec<Album> {
        if !self.is_authenticated() {
            return Vec::new();
        }
        self.library_browse_payload(LIBRARY_ALBUMS_BROWSE_ID)
            .map_or_else(|_| Vec::new(), |p| parse_library_albums(&p))
    }

    /// Converts this catalog API client into a [`LiveCatalog`](super::catalog_live::LiveCatalog) adapter.
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
        assert_eq!(client.detail("", DetailKind::Album), None);
        assert_eq!(client.detail("   ", DetailKind::Artist), None);
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
        assert!(anon.library_playlists().is_empty());
        assert!(anon.library_saved_albums().is_empty());
    }

    #[test]
    fn catalog_client_history_detail_unauthenticated_returns_none() {
        let client = CatalogClient::new();
        assert_eq!(client.detail(HISTORY_BROWSE_ID, DetailKind::Playlist), None);
    }

    #[test]
    fn catalog_client_category_shelves_parses_minimal_payload_and_guards() {
        let client = CatalogClient::new();
        assert!(client.category_shelves("", None).is_empty());
        assert!(client.category_shelves("   ", Some("p")).is_empty());

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
