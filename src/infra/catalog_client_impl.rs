//! HTTP client implementation for YouTube Music catalog endpoints.

use super::catalog_model::{RawTrack, SearchHits};
use super::catalog_parse_detail::parse_detail_all;
use super::catalog_parse_explore::parse_explore_all;
use super::catalog_parse_home::parse_home_all;
use super::catalog_parse_library::{
    parse_library_albums, parse_library_liked_tracks, parse_library_playlists,
};
use super::catalog_parse_radio::parse_radio_all;
use super::catalog_parse_search::{parse_search_all, parse_search_tracks};
use super::catalog_requests::{
    CATALOG_API_KEY, EXPLORE_BROWSE_ID, HOME_BROWSE_ID, LIBRARY_ALBUMS_BROWSE_ID,
    LIBRARY_LIKED_BROWSE_ID, LIBRARY_PLAYLISTS_BROWSE_ID, browse_body, browse_url, like_body,
    like_url, next_body, next_url, parse_mutation_response, search_body,
};
use crate::domain::{Album, Detail, DetailKind, Playlist, Shelf, Track};
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

    /// Executes search request and returns the unparsed JSON response payload.
    pub fn search_payload(&self, query: &str) -> anyhow::Result<Value> {
        let url = format!(
            "https://music.youtube.com/youtubei/v1/search?key={}&prettyPrint=false",
            self.api_key
        );
        let body = search_body(query, self.locale);
        let response = self.client.post(&url).json(&body).send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("CatalogClient search request failed with status: {status}");
        }
        let payload: Value = response.json()?;
        Ok(payload)
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

    /// Executes browse endpoint request for `browse_id` and returns the unparsed JSON response payload.
    pub fn browse_payload(&self, browse_id: &str) -> anyhow::Result<Value> {
        let url = browse_url();
        let body = browse_body(browse_id, self.locale);
        let response = self.client.post(&url).json(&body).send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("CatalogClient browse request failed with status: {status}");
        }
        let payload: Value = response.json()?;
        Ok(payload)
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

    /// Queries the browse endpoint for `browse_id` and returns parsed detail view of `kind`.
    /// Returns `None` on blank id, network error, or parse failure.
    pub fn detail(&self, browse_id: &str, kind: DetailKind) -> Option<Detail> {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return None;
        }
        let payload = self.browse_payload(trimmed).ok()?;
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

    /// Queries the browse endpoint for user's liked tracks.
    /// Returns an empty vector when unauthenticated or on network/parse failure.
    pub fn library_liked_tracks(&self) -> Vec<Track> {
        if !self.is_authenticated() {
            return Vec::new();
        }
        self.library_browse_payload(LIBRARY_LIKED_BROWSE_ID)
            .map_or_else(|_| Vec::new(), |p| parse_library_liked_tracks(&p))
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

    /// Executes authenticated like/removelike mutation for `video_id`.
    pub fn set_liked_payload(&self, video_id: &str, liked: bool) -> anyhow::Result<Value> {
        let Some(cookie) = &self.cookies else {
            anyhow::bail!("Unauthenticated CatalogClient cannot execute library mutations");
        };
        let url = like_url(liked);
        let body = like_body(video_id, self.locale);
        let response = self.client.post(&url).header("Cookie", cookie).json(&body).send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("CatalogClient like mutation failed with status: {status}");
        }
        let payload: Value = response.json()?;
        if !parse_mutation_response(&payload) {
            anyhow::bail!("CatalogClient like mutation response reported an error");
        }
        Ok(payload)
    }

    /// Sets liked status on `video_id`. No-op when unauthenticated, blank id, or on network failure.
    pub fn set_liked(&self, video_id: &str, liked: bool) {
        let trimmed = video_id.trim();
        if trimmed.is_empty() || !self.is_authenticated() {
            return;
        }
        let _ = self.set_liked_payload(trimmed, liked);
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
        assert!(anon.library_liked_tracks().is_empty());
        assert!(anon.library_saved_albums().is_empty());
        anon.set_liked("lib-t1", true);
    }
}
