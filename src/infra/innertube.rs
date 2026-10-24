//! InnerTube API client and response parser for YouTube Music search.

use crate::domain::SearchFilter;
use serde_json::Value;
use std::time::Duration;

/// InnerTube web API key for the WEB_REMIX client context.
pub const YTM_API_KEY: &str = "AIzaSyC1xlcUPO2SCEmJEWeeIg4X6KyWnhDk-4M";

/// Returns the InnerTube search endpoint URL.
pub fn search_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/search?key={YTM_API_KEY}&prettyPrint=false")
}

/// Builds the minimal WEB_REMIX search payload for `query`.
pub fn search_body(query: &str) -> Value {
    serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "hl": "en",
                "gl": "US",
            }
        },
        "query": query,
    })
}

/// Parsed track hit from InnerTube search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTrack {
    /// YouTube video identifier.
    pub video_id: String,
    /// Track title.
    pub title: String,
    /// Artist name.
    pub artist: String,
}

/// Parsed album hit from InnerTube search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawAlbum {
    pub browse_id: String,
    pub title: String,
    pub artist: String,
}

/// Parsed artist hit from InnerTube search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawArtist {
    pub browse_id: String,
    pub name: String,
}

/// Parsed playlist hit from InnerTube search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPlaylist {
    pub browse_id: String,
    pub title: String,
}

/// Grouped search hits across catalog item categories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchHits {
    pub tracks: Vec<RawTrack>,
    pub albums: Vec<RawAlbum>,
    pub artists: Vec<RawArtist>,
    pub playlists: Vec<RawPlaylist>,
}

impl SearchHits {
    /// Creates an empty search hits container.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Returns `true` if all categories have zero hits.
    pub fn is_empty(&self) -> bool {
        [self.tracks.len(), self.albums.len(), self.artists.len(), self.playlists.len()] == [0; 4]
    }
}

fn watch_video_id(item: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId",
        "/navigationEndpoint/watchEndpoint/videoId",
        "/overlay/navigationEndpoint/watchEndpoint/videoId",
        "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/watchEndpoint/videoId",
    ];

    for path in PATHS {
        if let Some(id) = item.pointer(path).and_then(Value::as_str) {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn flex_text(item: &Value, col_idx: usize) -> Option<String> {
    let col = item.get("flexColumns")?.get(col_idx)?;
    let text = col
        .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")?
        .as_str()?
        .trim();
    if text.is_empty() { None } else { Some(text.to_owned()) }
}

fn parse_item(renderer: &Value) -> Option<RawTrack> {
    let video_id = watch_video_id(renderer)?;
    let title = flex_text(renderer, 0)?;
    let artist = flex_text(renderer, 1).unwrap_or_default();
    Some(RawTrack { video_id, title, artist })
}

fn collect_shelf_items(shelf: &Value, out: &mut Vec<RawTrack>) {
    let Some(items) = shelf.get("contents").and_then(Value::as_array) else {
        return;
    };
    for item in items {
        if let Some(track) = item.get("musicResponsiveListItemRenderer").and_then(parse_item) {
            out.push(track);
        }
    }
}

fn collect_section_tracks(section: &Value, out: &mut Vec<RawTrack>) {
    if let Some(shelf) = section.get("musicShelfRenderer") {
        collect_shelf_items(shelf, out);
    }
}

fn collect_tab_tracks(tab: &Value, out: &mut Vec<RawTrack>) {
    let path = "/tabRenderer/content/sectionListRenderer/contents";
    let Some(sections) = tab.pointer(path).and_then(Value::as_array) else {
        return;
    };
    for section in sections {
        collect_section_tracks(section, out);
    }
}

/// Parses search track hits from InnerTube search response payload.
pub fn parse_search_tracks(value: &Value) -> Vec<RawTrack> {
    let mut tracks = Vec::new();
    let path = "/contents/tabbedSearchResultsRenderer/tabs";
    let Some(tabs) = value.pointer(path).and_then(Value::as_array) else {
        return tracks;
    };
    for tab in tabs {
        collect_tab_tracks(tab, &mut tracks);
    }
    tracks
}

const CATEGORY_KEYWORDS: &[(&[&str], SearchFilter)] = &[
    (&["song", "top result"], SearchFilter::Tracks),
    (&["album"], SearchFilter::Albums),
    (&["artist"], SearchFilter::Artists),
    (&["playlist", "mix", "show"], SearchFilter::Playlists),
];

/// Maps a shelf header title to a catalog category filter.
pub fn shelf_category(title: &str) -> Option<SearchFilter> {
    let lower = title.to_ascii_lowercase();
    CATEGORY_KEYWORDS.iter().find(|(kw, _)| kw.iter().any(|k| lower.contains(k))).map(|(_, f)| *f)
}

fn two_row_title(renderer: &Value) -> Option<String> {
    let title = renderer.pointer("/title/runs/0/text")?.as_str()?.trim();
    if title.is_empty() { None } else { Some(title.to_owned()) }
}

fn subtitle_text(renderer: &Value) -> String {
    renderer
        .pointer("/subtitle/runs/0/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_owned()
}

fn browse_id(renderer: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/navigationEndpoint/browseEndpoint/browseId",
        "/title/runs/0/navigationEndpoint/browseEndpoint/browseId",
    ];
    for path in PATHS {
        let trimmed = renderer.pointer(path).and_then(Value::as_str).unwrap_or("").trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    None
}

/// Parses title, subtitle, and optional browse ID from a two-row item renderer.
pub fn parse_two_row(renderer: &Value) -> Option<(String, String, Option<String>)> {
    let title = two_row_title(renderer)?;
    let subtitle = subtitle_text(renderer);
    let id = browse_id(renderer);
    Some((title, subtitle, id))
}

fn dispatch_two_row(category: SearchFilter, renderer: &Value, hits: &mut SearchHits) {
    let Some((title, subtitle, browse_id)) = parse_two_row(renderer) else {
        return;
    };
    let id = browse_id.unwrap_or_default();
    match category {
        SearchFilter::Albums => {
            hits.albums.push(RawAlbum { browse_id: id, title, artist: subtitle })
        }
        SearchFilter::Artists => hits.artists.push(RawArtist { browse_id: id, name: title }),
        SearchFilter::Playlists => hits.playlists.push(RawPlaylist { browse_id: id, title }),
        _ => {}
    }
}

fn collect_shelf_item(category: SearchFilter, item: &Value, hits: &mut SearchHits) {
    if category == SearchFilter::Tracks {
        if let Some(track) = item.get("musicResponsiveListItemRenderer").and_then(parse_item) {
            hits.tracks.push(track);
        }
    } else if let Some(renderer) = item.get("musicTwoRowItemRenderer") {
        dispatch_two_row(category, renderer, hits);
    }
}

fn collect_shelf(shelf: &Value, hits: &mut SearchHits) {
    let title = shelf.pointer("/title/runs/0/text").and_then(Value::as_str).unwrap_or("");
    let (Some(category), Some(items)) =
        (shelf_category(title), shelf.get("contents").and_then(Value::as_array))
    else {
        return;
    };
    for item in items {
        collect_shelf_item(category, item, hits);
    }
}

/// Parses grouped search hits across all supported shelves.
pub fn parse_search_all(value: &Value) -> SearchHits {
    let mut hits = SearchHits::empty();
    let Some(tabs) =
        value.pointer("/contents/tabbedSearchResultsRenderer/tabs").and_then(Value::as_array)
    else {
        return hits;
    };
    for tab in tabs {
        let Some(sections) = tab
            .pointer("/tabRenderer/content/sectionListRenderer/contents")
            .and_then(Value::as_array)
        else {
            continue;
        };
        for section in sections {
            if let Some(shelf) = section.get("musicShelfRenderer") {
                collect_shelf(shelf, &mut hits);
            }
        }
    }
    hits
}

/// Blocking HTTP client for the InnerTube API.
#[derive(Debug)]
pub struct InnerTube {
    client: reqwest::blocking::Client,
    api_key: String,
}

impl InnerTube {
    /// Creates a new InnerTube client with a 10-second request timeout.
    pub fn new() -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client, api_key: YTM_API_KEY.to_owned() }
    }

    /// Executes search request and returns the unparsed JSON response payload.
    pub fn search_payload(&self, query: &str) -> anyhow::Result<Value> {
        let url = format!(
            "https://music.youtube.com/youtubei/v1/search?key={}&prettyPrint=false",
            self.api_key
        );
        let body = search_body(query);
        let response = self.client.post(&url).json(&body).send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("InnerTube search request failed with status: {status}");
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

    /// Converts this InnerTube client into a [`YtmCatalog`](super::ytm::YtmCatalog) adapter.
    pub fn into_catalog(self) -> super::ytm::YtmCatalog {
        super::ytm::YtmCatalog::with_tube(self)
    }
}

impl Default for InnerTube {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_url_format_and_key() {
        let url = search_url();
        assert_eq!(
            url,
            format!(
                "https://music.youtube.com/youtubei/v1/search?key={YTM_API_KEY}&prettyPrint=false"
            )
        );
    }

    #[test]
    fn search_body_structure() {
        let body = search_body("midnight");
        assert_eq!(body["query"], "midnight");
        assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
        assert_eq!(body["context"]["client"]["hl"], "en");
        assert_eq!(body["context"]["client"]["gl"], "US");
    }

    #[test]
    fn parse_search_tracks_tolerates_empty_and_malformed() {
        assert!(parse_search_tracks(&serde_json::json!({})).is_empty());
        assert!(parse_search_tracks(&serde_json::json!({"contents": null})).is_empty());
        assert!(
            parse_search_tracks(&serde_json::json!({
                "contents": { "tabbedSearchResultsRenderer": { "tabs": [] } }
            }))
            .is_empty()
        );
    }

    #[test]
    fn parse_search_tracks_golden_fixture() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path = std::path::Path::new(manifest_dir)
            .join("tests")
            .join("fixtures")
            .join("innertube_search.json");
        let raw = std::fs::read_to_string(&fixture_path).expect("fixture file readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture json");

        let tracks = parse_search_tracks(&payload);
        assert_eq!(tracks.len(), 2);
        assert_eq!(
            tracks[0],
            RawTrack {
                video_id: "vid-1".to_owned(),
                title: "Midnight City".to_owned(),
                artist: "M83".to_owned(),
            }
        );
        assert_eq!(
            tracks[1],
            RawTrack {
                video_id: "vid-2".to_owned(),
                title: "Starboy".to_owned(),
                artist: "The Weeknd".to_owned(),
            }
        );
    }

    #[test]
    fn parse_search_tracks_skips_item_without_title() {
        let payload = serde_json::json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicShelfRenderer": {
                                            "contents": [{
                                                "musicResponsiveListItemRenderer": {
                                                    "navigationEndpoint": {
                                                        "watchEndpoint": { "videoId": "v1" }
                                                    }
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        });
        assert!(parse_search_tracks(&payload).is_empty());
    }

    #[test]
    fn shelf_category_routing_table() {
        for (title, expected) in [
            ("Songs", Some(SearchFilter::Tracks)),
            ("Top result", Some(SearchFilter::Tracks)),
            ("Top songs", Some(SearchFilter::Tracks)),
            ("Albums", Some(SearchFilter::Albums)),
            ("Artists", Some(SearchFilter::Artists)),
            ("Playlists", Some(SearchFilter::Playlists)),
            ("Community mixes", Some(SearchFilter::Playlists)),
            ("Featured shows", Some(SearchFilter::Playlists)),
            ("Podcasts", None),
            ("", None),
        ] {
            assert_eq!(shelf_category(title), expected, "failed for shelf title: {title}");
        }
    }

    #[test]
    fn parse_two_row_variants() {
        let ep = serde_json::json!({ "browseEndpoint": { "browseId": "b1" } });
        let row = |t, s, e| serde_json::json!({ "title": { "runs": [{ "text": t }] }, "subtitle": { "runs": [{ "text": s }] }, "navigationEndpoint": e });
        let row_te = |t, s, e| serde_json::json!({ "title": { "runs": [{ "text": t, "navigationEndpoint": e }] }, "subtitle": { "runs": [{ "text": s }] } });
        let ok =
            |t: &str, s: &str, id: Option<&str>| Some((t.into(), s.into(), id.map(Into::into)));
        let cases = [
            (row("A1", "B1", &ep), ok("A1", "B1", Some("b1"))),
            (row_te("A2", "B2", &ep), ok("A2", "B2", Some("b1"))),
            (row("A3", "B3", &Value::Null), ok("A3", "B3", None)),
            (serde_json::json!({ "title": { "runs": [{ "text": "   " }] } }), None),
        ];

        for (renderer, expected) in cases {
            assert_eq!(parse_two_row(&renderer), expected);
        }
    }

    #[test]
    fn parse_search_all_golden_fixture_and_determinism() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path =
            std::path::Path::new(manifest_dir).join("tests/fixtures/innertube_shelves.json");
        let raw = std::fs::read_to_string(fixture_path).expect("fixture file readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture json");

        let first = parse_search_all(&payload);
        assert_eq!(first, parse_search_all(&payload), "parsing must be deterministic");
        assert_eq!(
            (first.tracks.len(), first.albums.len(), first.artists.len(), first.playlists.len()),
            (1, 1, 1, 0)
        );
        assert_eq!(first.tracks[0].video_id, "solar-1");
        assert_eq!(first.tracks[0].title, "Solaris");
        assert_eq!(first.tracks[0].artist, "Photay");
        assert_eq!(first.albums[0].browse_id, "MPREb_album_1");
        assert_eq!(first.albums[0].title, "Waking Season");
        assert_eq!(first.albums[0].artist, "Photay");
        assert_eq!(first.artists[0].browse_id, "UC_photay_1");
        assert_eq!(first.artists[0].name, "Photay");
    }

    #[test]
    fn parse_search_all_ignores_unknown_shelves_and_tolerates_empty() {
        assert!(parse_search_all(&serde_json::json!({})).is_empty());

        let payload = serde_json::json!({
            "contents": { "tabbedSearchResultsRenderer": { "tabs": [{ "tabRenderer": {
                "content": { "sectionListRenderer": { "contents": [{ "musicShelfRenderer": {
                    "title": { "runs": [{ "text": "Podcasts" }] },
                    "contents": [{ "musicTwoRowItemRenderer": { "title": { "runs": [{ "text": "Item" }] } } }]
                }}]}}
            }}]}}
        });
        assert!(parse_search_all(&payload).is_empty());
    }
}
