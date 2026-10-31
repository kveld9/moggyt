//! Ingestion adapter for public Spotify embed pages.
//!
//! Parses `<script id="__NEXT_DATA__" type="application/json">` payloads
//! extracted from Spotify embed HTML documents.
//!
//! Provenance: Test fixtures `spotify_embed_playlist.html` and `spotify_embed_album.html`
//! are real captures of `https://open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M`
//! and `https://open.spotify.com/embed/album/1ATL5GLyefJaxhQzSPVrLX` taken on 2026-10-31,
//! with each track list trimmed to the first three entries.

use anyhow::Context;
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

use crate::domain::{SpotifyPlaylist, SpotifyRef, SpotifyTrack};

pub const EMBED_BASE_URL: &str = "https://open.spotify.com/embed";
pub const EMBED_MAX_TRACKS: usize = 100;
pub const EMBED_TIMEOUT_SECS: u64 = 20;

const SCRIPT_OPEN: &str = "<script id=\"__NEXT_DATA__\" type=\"application/json\">";
const SCRIPT_CLOSE: &str = "</script>";

/// Builds the embed URL for a Spotify playlist or album.
pub fn embed_url(source: &SpotifyRef) -> String {
    match source {
        SpotifyRef::Playlist(id) => format!("{EMBED_BASE_URL}/playlist/{id}"),
        SpotifyRef::Album(id) => format!("{EMBED_BASE_URL}/album/{id}"),
    }
}

/// Extracts the JSON payload from the `__NEXT_DATA__` script tag.
pub fn extract_next_data(html: &str) -> anyhow::Result<&str> {
    let open_idx = html.find(SCRIPT_OPEN).context("missing __NEXT_DATA__ script tag in html")?;
    let content_start = open_idx + SCRIPT_OPEN.len();
    let close_idx = html[content_start..]
        .find(SCRIPT_CLOSE)
        .context("unterminated __NEXT_DATA__ script tag in html")?;
    Ok(&html[content_start..content_start + close_idx])
}

fn find_resource_hint(raw: &str) -> String {
    const MARKERS: &[&str] = &["spotify:playlist:", "spotify:album:", "/playlist/", "/album/"];
    for marker in MARKERS {
        if let Some(pos) = raw.find(marker) {
            let candidate: String = raw[pos + marker.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if !candidate.is_empty() {
                return candidate;
            }
        }
    }
    "unknown".to_owned()
}

fn extract_resource_hint(root: &Value, raw: &str) -> String {
    if let Some(id) = root.pointer("/query/id").and_then(Value::as_str) {
        return id.to_owned();
    }
    if let Some(uri) =
        root.pointer("/props/pageProps/state/data/embeded_entity_uri").and_then(Value::as_str)
    {
        return uri.to_owned();
    }
    find_resource_hint(raw)
}

fn extract_entity_id(entity: &Value) -> Option<String> {
    if let Some(id) = entity.get("id").and_then(Value::as_str) {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    let uri = entity.get("uri").and_then(Value::as_str)?.trim();
    uri.rsplit(':').next().map(str::trim).filter(|s| !s.is_empty()).map(ToOwned::to_owned)
}

fn parse_single_track(item: &Value) -> Option<SpotifyTrack> {
    if let Some(et) = item.get("entityType").and_then(Value::as_str) {
        if et != "track" {
            return None;
        }
    }
    let title = item.get("title").and_then(Value::as_str)?.trim();
    if title.is_empty() {
        return None;
    }
    let artist =
        item.get("subtitle").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_owned();
    let duration_ms = item.get("duration").and_then(Value::as_u64).unwrap_or(0) as u32;

    Some(SpotifyTrack { title: title.to_owned(), artist, album: None, duration_ms, isrc: None })
}

pub(crate) fn parse_track_list(items: &[Value], limit: usize) -> Vec<SpotifyTrack> {
    items.iter().filter_map(parse_single_track).take(limit).collect()
}

/// Parses pure domain playlist models from Spotify embed page HTML with a track count limit.
pub(crate) fn parse_embed_html_with_limit(
    html: &str,
    limit: usize,
) -> anyhow::Result<SpotifyPlaylist> {
    let json_text = extract_next_data(html)?;
    let raw_hint = find_resource_hint(html);
    let root: Value = serde_json::from_str(json_text).with_context(|| {
        format!("malformed __NEXT_DATA__ JSON for Spotify resource '{raw_hint}'")
    })?;
    let hint = extract_resource_hint(&root, html);

    let entity = root.pointer("/props/pageProps/state/data/entity").with_context(|| {
        format!("missing entity payload in Spotify embed next data for resource '{hint}'")
    })?;

    let id = extract_entity_id(entity).unwrap_or(hint);
    let title = entity
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .with_context(|| {
            format!("missing or blank title in Spotify embed entity for resource '{id}'")
        })?;

    let author = entity
        .get("subtitle")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();

    let artwork = entity
        .pointer("/coverArt/sources/0/url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToOwned::to_owned);

    let track_list =
        entity.get("trackList").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let tracks = parse_track_list(track_list, limit);

    Ok(SpotifyPlaylist { id, title: title.to_owned(), author, artwork, tracks })
}

/// Parses pure domain playlist models from Spotify embed page HTML.
pub fn parse_embed_html(html: &str) -> anyhow::Result<SpotifyPlaylist> {
    parse_embed_html_with_limit(html, EMBED_MAX_TRACKS)
}

/// Blocking HTTP client for fetching and parsing Spotify embed pages.
#[derive(Debug)]
pub struct SpotifyEmbedClient {
    client: Client,
}

impl SpotifyEmbedClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(EMBED_TIMEOUT_SECS))
            .user_agent(format!("moggyt/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { client }
    }

    pub fn fetch(&self, source: &SpotifyRef) -> anyhow::Result<SpotifyPlaylist> {
        let url = embed_url(source);
        let response = self
            .client
            .get(&url)
            .send()
            .with_context(|| format!("failed to GET Spotify embed URL '{url}'"))?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Spotify embed request to '{url}' failed with HTTP {status}");
        }
        let body = response
            .text()
            .with_context(|| format!("failed to read response text from '{url}'"))?;
        parse_embed_html(&body)
    }
}

impl Default for SpotifyEmbedClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYLIST_HTML: &str = include_str!("../../tests/fixtures/spotify_embed_playlist.html");
    const ALBUM_HTML: &str = include_str!("../../tests/fixtures/spotify_embed_album.html");

    #[test]
    fn embed_url_formats_both_variants() {
        let playlist_ref = SpotifyRef::Playlist("37i9dQZF1DXcBWIGoYBM5M".to_string());
        assert_eq!(
            embed_url(&playlist_ref),
            "https://open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M"
        );

        let album_ref = SpotifyRef::Album("1ATL5GLyefJaxhQzSPVrLX".to_string());
        assert_eq!(
            embed_url(&album_ref),
            "https://open.spotify.com/embed/album/1ATL5GLyefJaxhQzSPVrLX"
        );
    }

    #[test]
    fn extract_next_data_success_and_errors() {
        assert!(extract_next_data("<html><body>no script</body></html>").is_err());
        assert!(
            extract_next_data(
                "<script id=\"__NEXT_DATA__\" type=\"application/json\">{\"test\": 1}"
            )
            .is_err()
        );

        let extracted = extract_next_data(PLAYLIST_HTML).expect("extracts payload");
        assert!(extracted.starts_with("{\"props\":"));
    }

    #[test]
    fn parse_playlist_fixture() {
        let playlist = parse_embed_html(PLAYLIST_HTML).expect("parses playlist fixture");
        assert_eq!(playlist.id, "37i9dQZF1DXcBWIGoYBM5M");
        assert_eq!(playlist.title, "Today\u{2019}s Top Hits");
        assert_eq!(playlist.author, "Spotify");
        assert_eq!(
            playlist.artwork.as_deref(),
            Some("https://i.scdn.co/image/ab67706f00000002960019ea66cba66a44fdd4c2")
        );
        assert_eq!(playlist.tracks.len(), 3);

        assert_eq!(playlist.tracks[0].title, "Loser");
        assert_eq!(playlist.tracks[0].artist, "Tame Impala");
        assert_eq!(playlist.tracks[0].duration_ms, 223069);
        assert_eq!(playlist.tracks[0].album, None);
        assert_eq!(playlist.tracks[0].isrc, None);

        assert_eq!(playlist.tracks[1].title, "Housewife");
        assert_eq!(playlist.tracks[1].artist, "Tate McRae");
        assert_eq!(playlist.tracks[1].duration_ms, 208810);

        assert_eq!(playlist.tracks[2].title, "Nicole Kidman");
        assert_eq!(playlist.tracks[2].artist, "AD\u{00c9}LA");
        assert_eq!(playlist.tracks[2].duration_ms, 181270);
    }

    #[test]
    fn parse_album_fixture() {
        let album = parse_embed_html(ALBUM_HTML).expect("parses album fixture");
        assert_eq!(album.id, "1ATL5GLyefJaxhQzSPVrLX");
        assert_eq!(album.title, "Scorpion");
        assert_eq!(album.author, "Drake");
        assert_eq!(album.artwork, None);
        assert_eq!(album.tracks.len(), 3);

        assert_eq!(album.tracks[0].title, "Survival");
        assert_eq!(album.tracks[0].artist, "Drake");
        assert_eq!(album.tracks[0].duration_ms, 136186);

        assert_eq!(album.tracks[1].title, "Nonstop");
        assert_eq!(album.tracks[1].artist, "Drake");
        assert_eq!(album.tracks[1].duration_ms, 238614);

        assert_eq!(album.tracks[2].title, "Elevate");
        assert_eq!(album.tracks[2].artist, "Drake");
        assert_eq!(album.tracks[2].duration_ms, 184960);
    }

    #[test]
    fn malformed_json_fails_with_context() {
        let html = "<script id=\"__NEXT_DATA__\" type=\"application/json\">{invalid json</script>";
        let err = parse_embed_html(html).expect_err("malformed json fails");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("JSON") || msg.contains("json"),
            "error should mention json parsing issue: {msg}"
        );
    }

    #[test]
    fn missing_cover_art_yields_none() {
        let html = r#"<!DOCTYPE html><html><body>
<script id="__NEXT_DATA__" type="application/json">
{"props":{"pageProps":{"state":{"data":{"entity":{"id":"inline_1","title":"Minimal Playlist","subtitle":"Author","trackList":[]}}}}}}
</script></body></html>"#;
        let playlist = parse_embed_html(html).expect("parses minimal html without coverArt");
        assert_eq!(playlist.id, "inline_1");
        assert_eq!(playlist.title, "Minimal Playlist");
        assert_eq!(playlist.author, "Author");
        assert_eq!(playlist.artwork, None);
        assert!(playlist.tracks.is_empty());
    }

    #[test]
    fn entity_validation_errors_name_resource() {
        let missing_entity_html = r#"<script id="__NEXT_DATA__" type="application/json">
{"props":{"pageProps":{"state":{"data":{}}}},"query":{"id":"sp_res_123"}}
</script>"#;
        let err = parse_embed_html(missing_entity_html).expect_err("missing entity must error");
        let msg = format!("{err:#}");
        assert!(msg.contains("sp_res_123"), "error should name resource: {msg}");

        let blank_title_html = r#"<script id="__NEXT_DATA__" type="application/json">
{"props":{"pageProps":{"state":{"data":{"entity":{"id":"sp_res_456","title":"   "}}}}}}
</script>"#;
        let err = parse_embed_html(blank_title_html).expect_err("blank title must error");
        let msg = format!("{err:#}");
        assert!(msg.contains("sp_res_456"), "error should name resource: {msg}");
    }

    #[test]
    fn track_list_truncation_and_skipping() {
        let items = vec![
            serde_json::json!({
                "title": "Track 1",
                "subtitle": "Artist 1",
                "duration": 1000,
                "entityType": "track"
            }),
            serde_json::json!({
                "title": "Episode 1",
                "subtitle": "Podcaster",
                "duration": 2000,
                "entityType": "episode"
            }),
            serde_json::json!({
                "title": "   ",
                "subtitle": "Blank Title",
                "duration": 3000,
                "entityType": "track"
            }),
            serde_json::json!({
                "title": "Track 2",
                "subtitle": "Artist 2",
                "duration": 4000,
                "entityType": "track"
            }),
            serde_json::json!({
                "title": "Track 3",
                "subtitle": "Artist 3",
                "duration": 5000,
                "entityType": "track"
            }),
        ];

        let parsed = parse_track_list(&items, 2);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title, "Track 1");
        assert_eq!(parsed[1].title, "Track 2");
    }

    #[test]
    fn embed_html_truncates_tracks_with_limit_helper() {
        let html = r#"<!DOCTYPE html><html><body>
<script id="__NEXT_DATA__" type="application/json">
{"props":{"pageProps":{"state":{"data":{"entity":{"id":"inline_trunc","title":"3-Track List","trackList":[
{"title":"T1","subtitle":"A1","duration":100,"entityType":"track"},
{"title":"T2","subtitle":"A2","duration":200,"entityType":"track"},
{"title":"T3","subtitle":"A3","duration":300,"entityType":"track"}
]}}}}}}
</script></body></html>"#;
        let playlist = parse_embed_html_with_limit(html, 2).expect("parses with limit 2");
        assert_eq!(playlist.tracks.len(), 2);
        assert_eq!(playlist.tracks[0].title, "T1");
        assert_eq!(playlist.tracks[1].title, "T2");
    }

    #[test]
    fn client_constructor_and_default() {
        let client = SpotifyEmbedClient::new();
        assert!(format!("{client:?}").contains("SpotifyEmbedClient"));
        let default_client = SpotifyEmbedClient::default();
        assert!(format!("{default_client:?}").contains("SpotifyEmbedClient"));
    }
}
