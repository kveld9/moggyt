//! Spotify Web API ingestion client using Client Credentials flow.
//!
//! Provides automated ingestion of public and unlisted Spotify playlists and
//! albums using the official Spotify Web API.
//!
//! # Client Credentials & ISRC Ingestion
//! Unlike embed HTML scraping (which is capped at 100 tracks and omits catalog
//! identifiers), this client accesses full playlist and album tracklists by
//! requesting an OAuth 2.0 Client Credentials bearer token and fetching all pages.
//! For each track, external identifiers (specifically ISRC) are extracted to enable
//! high-confidence matching against YouTube Music catalog records.
//!
//! # Pagination
//! Track endpoints are paginated in batches of [`API_PAGE_LIMIT`] items (offset 0, 100, 200, ...).
//! Ingestion continues until a page returns fewer raw items than [`API_PAGE_LIMIT`].

use std::time::Duration;

use anyhow::Context;
use reqwest::blocking::Client;
use serde_json::Value;

use crate::domain::{SpotifyPlaylist, SpotifyRef, SpotifyTrack};
use crate::infra::spotify_credentials::SpotifyCredentials;

/// Spotify OAuth 2.0 token endpoint.
pub const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

/// Spotify Web API v1 base URL.
pub const API_BASE_URL: &str = "https://api.spotify.com/v1";

/// Maximum number of tracks fetched per pagination request.
pub const API_PAGE_LIMIT: usize = 100;

/// Default timeout in seconds for Spotify Web API HTTP requests.
pub const API_TIMEOUT_SECS: u64 = 20;

/// Blocking HTTP client for Spotify Web API ingestion using Client Credentials.
pub struct SpotifyApiClient {
    client: Client,
    credentials: SpotifyCredentials,
}

impl std::fmt::Debug for SpotifyApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpotifyApiClient")
            .field("client_id", &self.credentials.client_id)
            .finish_non_exhaustive()
    }
}

impl SpotifyApiClient {
    /// Creates a new client configured with the given credentials.
    pub fn new(credentials: SpotifyCredentials) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(API_TIMEOUT_SECS))
            .user_agent(format!("moggyt/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { client, credentials }
    }

    /// Fetches a complete playlist or album from the Spotify Web API.
    pub fn fetch(&self, source: &SpotifyRef) -> anyhow::Result<SpotifyPlaylist> {
        let (id, is_playlist) = match source {
            SpotifyRef::Playlist(id) => (id.as_str(), true),
            SpotifyRef::Album(id) => (id.as_str(), false),
        };
        let token = self.request_token()?;
        let (title, author, artwork) = self.fetch_metadata(id, is_playlist, &token)?;
        let tracks = self.fetch_tracks(id, is_playlist, &token)?;
        Ok(SpotifyPlaylist { id: id.to_owned(), title, author, artwork, tracks })
    }

    fn request_token(&self) -> anyhow::Result<String> {
        let response = self
            .client
            .post(TOKEN_URL)
            .basic_auth(&self.credentials.client_id, Some(&self.credentials.client_secret))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .with_context(|| format!("failed to POST Spotify token URL '{TOKEN_URL}'"))?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Spotify token request to '{TOKEN_URL}' failed with HTTP {status}");
        }
        let body = response
            .text()
            .with_context(|| format!("failed to read Spotify token response from '{TOKEN_URL}'"))?;
        let root: Value = serde_json::from_str(&body).with_context(|| {
            format!("malformed JSON in Spotify token response from '{TOKEN_URL}'")
        })?;
        parse_token_response(&root)
    }

    fn get_json(&self, url: &str, token: &str) -> anyhow::Result<Value> {
        let response = self
            .client
            .get(url)
            .bearer_auth(token)
            .send()
            .with_context(|| format!("failed to GET Spotify API URL '{url}'"))?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Spotify API request to '{url}' failed with HTTP {status}");
        }
        let body = response
            .text()
            .with_context(|| format!("failed to read response text from '{url}'"))?;
        serde_json::from_str(&body).with_context(|| format!("malformed JSON response from '{url}'"))
    }

    fn fetch_metadata(
        &self,
        id: &str,
        is_playlist: bool,
        token: &str,
    ) -> anyhow::Result<(String, String, Option<String>)> {
        if is_playlist {
            let url = playlist_metadata_url(id);
            let root = self.get_json(&url, token)?;
            parse_playlist_metadata(&root)
                .with_context(|| format!("missing or blank playlist title for resource '{id}'"))
        } else {
            let url = album_metadata_url(id);
            let root = self.get_json(&url, token)?;
            parse_album_metadata(&root)
                .with_context(|| format!("missing or blank album title for resource '{id}'"))
        }
    }

    fn fetch_tracks(
        &self,
        id: &str,
        is_playlist: bool,
        token: &str,
    ) -> anyhow::Result<Vec<SpotifyTrack>> {
        let mut offset = 0;
        let mut tracks = Vec::new();
        loop {
            let page = self.fetch_single_page(id, is_playlist, offset, token)?;
            tracks.extend(page.tracks);
            if page.item_count < API_PAGE_LIMIT {
                break;
            }
            offset += API_PAGE_LIMIT;
        }
        Ok(tracks)
    }

    fn fetch_single_page(
        &self,
        id: &str,
        is_playlist: bool,
        offset: usize,
        token: &str,
    ) -> anyhow::Result<ParsedPage> {
        if is_playlist {
            let url = playlist_tracks_url(id, offset);
            let val = self.get_json(&url, token)?;
            Ok(parse_playlist_page(&val))
        } else {
            let url = album_tracks_url(id, offset);
            let val = self.get_json(&url, token)?;
            Ok(parse_album_page(&val))
        }
    }
}

/// Extracts the access token string from a Spotify token response.
pub(crate) fn parse_token_response(root: &Value) -> anyhow::Result<String> {
    let token = root
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .context("missing or empty 'access_token' in Spotify token response")?;
    Ok(token.to_owned())
}

/// Builds the metadata request URL for a Spotify playlist.
pub(crate) fn playlist_metadata_url(id: &str) -> String {
    format!("{API_BASE_URL}/playlists/{id}?fields=name,owner(display_name),images")
}

/// Builds the paginated tracks request URL for a Spotify playlist.
pub(crate) fn playlist_tracks_url(id: &str, offset: usize) -> String {
    format!("{API_BASE_URL}/playlists/{id}/tracks?limit={API_PAGE_LIMIT}&offset={offset}")
}

/// Builds the metadata request URL for a Spotify album.
pub(crate) fn album_metadata_url(id: &str) -> String {
    format!("{API_BASE_URL}/albums/{id}?fields=name,artists,images")
}

/// Builds the paginated tracks request URL for a Spotify album.
pub(crate) fn album_tracks_url(id: &str, offset: usize) -> String {
    format!("{API_BASE_URL}/albums/{id}/tracks?limit={API_PAGE_LIMIT}&offset={offset}")
}

/// Parses playlist metadata (title, author, artwork) from a Spotify API response.
pub(crate) fn parse_playlist_metadata(
    root: &Value,
) -> anyhow::Result<(String, String, Option<String>)> {
    let resource_id = root
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or("unknown");
    let title = root
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .with_context(|| {
        format!("missing or blank playlist title for resource '{resource_id}'")
    })?;
    let author = extract_playlist_author(root);
    let artwork = extract_first_image_url(root);
    Ok((title.to_owned(), author, artwork))
}

/// Parses album metadata (title, author, artwork) from a Spotify API response.
pub(crate) fn parse_album_metadata(
    root: &Value,
) -> anyhow::Result<(String, String, Option<String>)> {
    let resource_id = root
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or("unknown");
    let title = root
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .with_context(|| {
        format!("missing or blank album title for resource '{resource_id}'")
    })?;
    let author = extract_joined_artists(root);
    let artwork = extract_first_image_url(root);
    Ok((title.to_owned(), author, artwork))
}

/// Tracks and raw item count parsed from a single page of playlist or album tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedPage {
    pub tracks: Vec<SpotifyTrack>,
    pub item_count: usize,
}

/// Parses a page of tracks from a Spotify playlist tracks response.
pub(crate) fn parse_playlist_page(root: &Value) -> ParsedPage {
    let Some(items) = root.get("items").and_then(Value::as_array) else {
        return ParsedPage { tracks: Vec::new(), item_count: 0 };
    };
    let item_count = items.len();
    let tracks = items
        .iter()
        .filter_map(extract_playlist_track_object)
        .filter_map(parse_track_object)
        .collect();
    ParsedPage { tracks, item_count }
}

/// Parses a page of tracks from a Spotify album tracks response.
pub(crate) fn parse_album_page(root: &Value) -> ParsedPage {
    let Some(items) = root.get("items").and_then(Value::as_array) else {
        return ParsedPage { tracks: Vec::new(), item_count: 0 };
    };
    let item_count = items.len();
    let tracks = items.iter().filter_map(parse_track_object).collect();
    ParsedPage { tracks, item_count }
}

fn extract_playlist_author(root: &Value) -> String {
    root.pointer("/owner/display_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_owned()
}

fn extract_joined_artists(root: &Value) -> String {
    let Some(artists) = root.get("artists").and_then(Value::as_array) else {
        return String::new();
    };
    let names: Vec<&str> = artists
        .iter()
        .filter_map(|a| a.get("name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    names.join(", ")
}

fn extract_first_image_url(root: &Value) -> Option<String> {
    let images = root.get("images").and_then(Value::as_array)?;
    images
        .iter()
        .filter_map(|img| img.get("url").and_then(Value::as_str))
        .map(str::trim)
        .find(|url| !url.is_empty())
        .map(ToOwned::to_owned)
}

fn extract_playlist_track_object(item: &Value) -> Option<&Value> {
    item.get("track").filter(|t| !t.is_null()).or_else(|| item.get("item").filter(|i| !i.is_null()))
}

fn parse_track_object(track: &Value) -> Option<SpotifyTrack> {
    if track.get("is_local").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let title = track.get("name").and_then(Value::as_str)?.trim();
    if title.is_empty() {
        return None;
    }
    let artist = extract_joined_artists(track);
    let album = track
        .pointer("/album/name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let duration_ms = track.get("duration_ms").and_then(Value::as_u64).unwrap_or(0) as u32;
    let isrc = track
        .pointer("/external_ids/isrc")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    Some(SpotifyTrack { title: title.to_owned(), artist, album, duration_ms, isrc })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn token_parse_success_and_missing_error() {
        let valid = json!({
            "access_token": "BQ_valid_token_12345",
            "token_type": "Bearer",
            "expires_in": 3600
        });
        assert_eq!(
            parse_token_response(&valid).expect("valid token parsed"),
            "BQ_valid_token_12345"
        );

        let missing = json!({
            "error": "invalid_client",
            "error_description": "Invalid client credentials"
        });
        assert!(parse_token_response(&missing).is_err());

        let empty = json!({
            "access_token": "   ",
            "token_type": "Bearer"
        });
        assert!(parse_token_response(&empty).is_err());
    }

    #[test]
    fn exact_url_strings_metadata_and_offsets() {
        let pl_id = "37i9dQZF1DXcBWIGoYBM5M";
        assert_eq!(
            playlist_metadata_url(pl_id),
            "https://api.spotify.com/v1/playlists/37i9dQZF1DXcBWIGoYBM5M?fields=name,owner(display_name),images"
        );
        assert_eq!(
            playlist_tracks_url(pl_id, 0),
            "https://api.spotify.com/v1/playlists/37i9dQZF1DXcBWIGoYBM5M/tracks?limit=100&offset=0"
        );
        assert_eq!(
            playlist_tracks_url(pl_id, 100),
            "https://api.spotify.com/v1/playlists/37i9dQZF1DXcBWIGoYBM5M/tracks?limit=100&offset=100"
        );

        let alb_id = "1ATL5GLyefJaxhQzSPVrLX";
        assert_eq!(
            album_metadata_url(alb_id),
            "https://api.spotify.com/v1/albums/1ATL5GLyefJaxhQzSPVrLX?fields=name,artists,images"
        );
        assert_eq!(
            album_tracks_url(alb_id, 0),
            "https://api.spotify.com/v1/albums/1ATL5GLyefJaxhQzSPVrLX/tracks?limit=100&offset=0"
        );
        assert_eq!(
            album_tracks_url(alb_id, 100),
            "https://api.spotify.com/v1/albums/1ATL5GLyefJaxhQzSPVrLX/tracks?limit=100&offset=100"
        );
    }

    #[test]
    fn playlist_metadata_parse_with_and_without_owner_images() {
        let with_all = json!({
            "id": "pl_full",
            "name": "Today's Top Hits",
            "owner": {
                "display_name": "Spotify"
            },
            "images": [
                {
                    "url": "https://i.scdn.co/image/art1.jpg",
                    "height": 640,
                    "width": 640
                },
                {
                    "url": "https://i.scdn.co/image/art2.jpg"
                }
            ]
        });
        let (title, author, artwork) =
            parse_playlist_metadata(&with_all).expect("metadata parses with full payload");
        assert_eq!(title, "Today's Top Hits");
        assert_eq!(author, "Spotify");
        assert_eq!(artwork.as_deref(), Some("https://i.scdn.co/image/art1.jpg"));

        let minimal = json!({
            "id": "pl_min",
            "name": "Minimal Playlist",
            "owner": null,
            "images": []
        });
        let (title, author, artwork) =
            parse_playlist_metadata(&minimal).expect("metadata parses with minimal payload");
        assert_eq!(title, "Minimal Playlist");
        assert_eq!(author, "");
        assert_eq!(artwork, None);
    }

    #[test]
    fn playlist_page_track_filtering_and_mapping() {
        let payload = json!({
            "items": [
                {
                    "track": {
                        "name": "Track One",
                        "artists": [
                            {"name": "Primary Artist"},
                            {"name": "Featured Artist"}
                        ],
                        "album": {
                            "name": "Greatest Hits"
                        },
                        "duration_ms": 215000,
                        "external_ids": {
                            "isrc": "USAT29900001"
                        },
                        "is_local": false
                    }
                },
                {
                    "track": null
                },
                {
                    "track": {
                        "name": "Local Audio File",
                        "artists": [{"name": "Self"}],
                        "is_local": true,
                        "duration_ms": 180000
                    }
                },
                {
                    "track": {
                        "name": "No ISRC Track",
                        "artists": [{"name": "Solo Singer"}],
                        "duration_ms": 195000
                    }
                },
                {
                    "item": {
                        "name": "Item Key Track",
                        "artists": [{"name": "Alternative Format"}],
                        "album": {"name": "Single Release"},
                        "duration_ms": 160000,
                        "external_ids": {"isrc": "GBAYE0000002"}
                    }
                },
                {
                    "track": {
                        "name": "   ",
                        "artists": [{"name": "Blank Title Artist"}]
                    }
                }
            ]
        });

        let page = parse_playlist_page(&payload);
        assert_eq!(page.item_count, 6);
        assert_eq!(page.tracks.len(), 3);

        assert_eq!(page.tracks[0].title, "Track One");
        assert_eq!(page.tracks[0].artist, "Primary Artist, Featured Artist");
        assert_eq!(page.tracks[0].album.as_deref(), Some("Greatest Hits"));
        assert_eq!(page.tracks[0].duration_ms, 215000);
        assert_eq!(page.tracks[0].isrc.as_deref(), Some("USAT29900001"));

        assert_eq!(page.tracks[1].title, "No ISRC Track");
        assert_eq!(page.tracks[1].artist, "Solo Singer");
        assert_eq!(page.tracks[1].album, None);
        assert_eq!(page.tracks[1].duration_ms, 195000);
        assert_eq!(page.tracks[1].isrc, None);

        assert_eq!(page.tracks[2].title, "Item Key Track");
        assert_eq!(page.tracks[2].artist, "Alternative Format");
        assert_eq!(page.tracks[2].album.as_deref(), Some("Single Release"));
        assert_eq!(page.tracks[2].duration_ms, 160000);
        assert_eq!(page.tracks[2].isrc.as_deref(), Some("GBAYE0000002"));
    }

    #[test]
    fn album_metadata_parse_with_artists_and_images() {
        let payload = json!({
            "id": "alb_123",
            "name": "Masterpiece",
            "artists": [
                {"name": "Artist Alpha"},
                {"name": "Artist Beta"}
            ],
            "images": [
                {"url": "https://i.scdn.co/image/album_art.png"}
            ]
        });
        let (title, author, artwork) =
            parse_album_metadata(&payload).expect("album metadata parses");
        assert_eq!(title, "Masterpiece");
        assert_eq!(author, "Artist Alpha, Artist Beta");
        assert_eq!(artwork.as_deref(), Some("https://i.scdn.co/image/album_art.png"));

        let minimal = json!({
            "id": "alb_solo",
            "name": "Solo Record"
        });
        let (title, author, artwork) =
            parse_album_metadata(&minimal).expect("minimal album metadata parses");
        assert_eq!(title, "Solo Record");
        assert_eq!(author, "");
        assert_eq!(artwork, None);
    }

    #[test]
    fn album_page_parse_direct_track_items() {
        let payload = json!({
            "items": [
                {
                    "name": "Track A",
                    "artists": [{"name": "Band"}],
                    "duration_ms": 150000,
                    "external_ids": {"isrc": "SEABC1234567"}
                },
                {
                    "name": "Track B",
                    "artists": [{"name": "Band"}],
                    "duration_ms": 175000
                },
                {
                    "name": "   ",
                    "artists": [{"name": "Band"}]
                }
            ]
        });
        let page = parse_album_page(&payload);
        assert_eq!(page.item_count, 3);
        assert_eq!(page.tracks.len(), 2);

        assert_eq!(page.tracks[0].title, "Track A");
        assert_eq!(page.tracks[0].artist, "Band");
        assert_eq!(page.tracks[0].duration_ms, 150000);
        assert_eq!(page.tracks[0].isrc.as_deref(), Some("SEABC1234567"));

        assert_eq!(page.tracks[1].title, "Track B");
        assert_eq!(page.tracks[1].artist, "Band");
        assert_eq!(page.tracks[1].duration_ms, 175000);
        assert_eq!(page.tracks[1].isrc, None);
    }

    #[test]
    fn blank_playlist_and_album_title_errors_name_resource() {
        let blank_pl = json!({
            "id": "test_playlist_999",
            "name": "   "
        });
        let err = parse_playlist_metadata(&blank_pl).expect_err("blank title must fail");
        let msg = format!("{err:#}");
        assert!(msg.contains("test_playlist_999"), "error must name playlist resource id: {msg}");

        let blank_alb = json!({
            "id": "test_album_888",
            "name": ""
        });
        let err = parse_album_metadata(&blank_alb).expect_err("blank album title must fail");
        let msg = format!("{err:#}");
        assert!(msg.contains("test_album_888"), "error must name album resource id: {msg}");
    }

    #[test]
    fn client_constructor_and_debug_format() {
        let creds = SpotifyCredentials::new("client_id_val", "super_secret_val");
        let client = SpotifyApiClient::new(creds);
        let debug_str = format!("{client:?}");
        assert!(debug_str.contains("SpotifyApiClient"));
        assert!(debug_str.contains("client_id_val"));
        assert!(!debug_str.contains("super_secret_val"));
    }
}
