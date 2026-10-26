//! InnerTube API client and response parser for YouTube Music search.

use crate::domain::{Album, Artist, Playlist, SearchFilter, SearchResult, Shelf, Track};
use serde_json::Value;
use std::time::Duration;

/// Public InnerTube web API key for the WEB_REMIX client context.
///
/// Embedded by every unofficial client; grants no private-data or billable-resource
/// access and cannot be rotated per installation (see `.gitleaks.toml` allowlist).
pub const YTM_API_KEY: &str = "AIzaSyC1xlcUPO2SCEmJEWeeIg4X6KyWnhDk-4M";

/// InnerTube client version for the WEB_REMIX client context.
///
/// Verified against live API on 2026-10-24. WEB_REMIX client versions
/// rotate over time; bump this value when YouTube starts rejecting search
/// requests with HTTP 400 FAILED_PRECONDITION. No auto-discovery in v1.
pub const YTM_CLIENT_VERSION: &str = "1.20251022.01.00";

/// InnerTube browse ID for the home recommendations page.
pub const HOME_BROWSE_ID: &str = "FEmusic_home";

/// Returns the InnerTube search endpoint URL.
pub fn search_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/search?key={YTM_API_KEY}&prettyPrint=false")
}

/// Returns the InnerTube browse endpoint URL.
pub fn browse_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/browse?key={YTM_API_KEY}&prettyPrint=false")
}

fn remix_context() -> Value {
    serde_json::json!({
        "client": {
            "clientName": "WEB_REMIX",
            "clientVersion": YTM_CLIENT_VERSION,
            "hl": "en",
            "gl": "US",
        }
    })
}

/// Builds the minimal WEB_REMIX search payload for `query`.
pub fn search_body(query: &str) -> Value {
    serde_json::json!({ "context": remix_context(), "query": query })
}

/// Builds the minimal WEB_REMIX browse payload for `browse_id`.
pub fn browse_body(browse_id: &str) -> Value {
    serde_json::json!({ "context": remix_context(), "browseId": browse_id })
}

/// Converts a raw track hit into a domain `Track`.
pub fn raw_to_track(raw: RawTrack) -> Track {
    let track = Track::new(&raw.video_id, &raw.title, &raw.artist);
    match raw.thumb_url {
        Some(url) => track.with_artwork(url),
        None => track,
    }
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
    /// Highest-resolution thumbnail URL when known.
    pub thumb_url: Option<String>,
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
    let thumb_url = best_thumbnail(renderer);
    Some(RawTrack { video_id, title, artist, thumb_url })
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

fn collect_item_or_shelf_track(item: &Value, out: &mut Vec<RawTrack>) {
    if let Some(shelf) = item.get("musicShelfRenderer") {
        collect_shelf_items(shelf, out);
    } else if let Some(track) = item.get("musicResponsiveListItemRenderer").and_then(parse_item) {
        out.push(track);
    }
}

fn collect_item_section_tracks(item_section: &Value, out: &mut Vec<RawTrack>) {
    let Some(items) = item_section.get("contents").and_then(Value::as_array) else {
        return;
    };
    for item in items {
        collect_item_or_shelf_track(item, out);
    }
}

fn collect_section_tracks(section: &Value, out: &mut Vec<RawTrack>) {
    if let Some(shelf) = section.get("musicShelfRenderer") {
        collect_shelf_items(shelf, out);
    } else if let Some(item_section) = section.get("itemSectionRenderer") {
        collect_item_section_tracks(item_section, out);
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

/// Extracts the highest-resolution thumbnail URL from a renderer node.
///
/// Reads `.thumbnail.musicThumbnailRenderer.thumbnail.thumbs[]`.
/// When multiple thumbnails exist, picks the one with the greatest width.
/// In case of ties, the first candidate wins (strictly-greater replacement).
/// Blank URLs and non-array payloads return `None`.
pub fn best_thumbnail(value: &Value) -> Option<String> {
    let thumbs = value.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbs")?.as_array()?;

    let mut best: Option<(&str, u64)> = None;
    for item in thumbs {
        let url = item.get("url").and_then(Value::as_str).map(str::trim).unwrap_or("");
        if url.is_empty() {
            continue;
        }
        let width = item.get("width").and_then(Value::as_u64).unwrap_or(0);
        if best.is_none_or(|(_, max_w)| width > max_w) {
            best = Some((url, width));
        }
    }
    best.map(|(url, _)| url.to_owned())
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

fn collect_item_or_shelf_all(item: &Value, hits: &mut SearchHits) {
    if let Some(shelf) = item.get("musicShelfRenderer") {
        collect_shelf(shelf, hits);
    } else if let Some(track) = item.get("musicResponsiveListItemRenderer").and_then(parse_item) {
        // Bare responsive items outside named shelves are always classified as Tracks.
        hits.tracks.push(track);
    }
}

fn collect_item_section_all(item_section: &Value, hits: &mut SearchHits) {
    let Some(items) = item_section.get("contents").and_then(Value::as_array) else {
        return;
    };
    for item in items {
        collect_item_or_shelf_all(item, hits);
    }
}

fn collect_section_all(section: &Value, hits: &mut SearchHits) {
    if let Some(shelf) = section.get("musicShelfRenderer") {
        collect_shelf(shelf, hits);
    } else if let Some(item_section) = section.get("itemSectionRenderer") {
        collect_item_section_all(item_section, hits);
    }
}

fn collect_tab_all(tab: &Value, hits: &mut SearchHits) {
    let path = "/tabRenderer/content/sectionListRenderer/contents";
    let Some(sections) = tab.pointer(path).and_then(Value::as_array) else {
        return;
    };
    for section in sections {
        collect_section_all(section, hits);
    }
}

/// Parses grouped search hits across all supported shelves.
///
/// Bare responsive list items inside `itemSectionRenderer` nodes are always
/// classified as tracks; shelf title category routing applies inside named shelves.
pub fn parse_search_all(value: &Value) -> SearchHits {
    let mut hits = SearchHits::empty();
    let Some(tabs) =
        value.pointer("/contents/tabbedSearchResultsRenderer/tabs").and_then(Value::as_array)
    else {
        return hits;
    };
    for tab in tabs {
        collect_tab_all(tab, &mut hits);
    }
    hits
}

fn classify_two_row_filter(id: &str, category: Option<SearchFilter>) -> Option<SearchFilter> {
    if let Some(cat) = category {
        return Some(cat);
    }
    if id.starts_with("MPREb") {
        return Some(SearchFilter::Albums);
    }
    if id.starts_with("UC") {
        return Some(SearchFilter::Artists);
    }
    if id.starts_with("VL") || id.starts_with("PL") {
        return Some(SearchFilter::Playlists);
    }
    None
}

fn parse_two_row_result(renderer: &Value, category: Option<SearchFilter>) -> Option<SearchResult> {
    let (title, subtitle, id_opt) = parse_two_row(renderer)?;
    let id = id_opt.unwrap_or_default();
    let filter = classify_two_row_filter(&id, category).unwrap_or_else(|| {
        let sub = subtitle.to_ascii_lowercase();
        if sub.contains("artist") {
            SearchFilter::Artists
        } else if sub.contains("playlist") {
            SearchFilter::Playlists
        } else {
            SearchFilter::Albums
        }
    });
    match filter {
        SearchFilter::Artists => Some(SearchResult::Artist(Artist::new(&id, &title))),
        SearchFilter::Playlists => Some(SearchResult::Playlist(Playlist::new(&id, &title, 0))),
        _ => Some(SearchResult::Album(Album::new(&id, &title, &subtitle))),
    }
}

fn parse_shelf_item(item: &Value, category: Option<SearchFilter>) -> Option<SearchResult> {
    if let Some(r) = item.get("musicResponsiveListItemRenderer") {
        return parse_item(r).map(|t| SearchResult::Track(raw_to_track(t)));
    }
    if let Some(r) = item.get("musicTwoRowItemRenderer") {
        return parse_two_row_result(r, category);
    }
    None
}

fn shelf_node_title(shelf: &Value) -> Option<&str> {
    const PATHS: &[&str] =
        &["/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text", "/title/runs/0/text"];
    for path in PATHS {
        if let Some(t) = shelf.pointer(path).and_then(Value::as_str) {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn extract_shelf_node(section: &Value) -> Option<&Value> {
    section.get("musicCarouselShelfRenderer").or_else(|| section.get("musicShelfRenderer"))
}

fn parse_shelf(shelf: &Value) -> Option<Shelf> {
    let title = shelf_node_title(shelf)?;
    let items_val = shelf.get("contents").and_then(Value::as_array)?;
    let category = shelf_category(title);
    let mut items = Vec::new();
    for item in items_val {
        if let Some(parsed) = parse_shelf_item(item, category) {
            items.push(parsed);
        }
    }
    if items.is_empty() {
        return None;
    }
    Some(Shelf { title: title.to_owned(), items })
}

fn browse_sections(value: &Value) -> Option<&Vec<Value>> {
    const PATHS: &[&str] = &[
        "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents",
        "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents",
        "/contents/sectionListRenderer/contents",
    ];
    for path in PATHS {
        if let Some(arr) = value.pointer(path).and_then(Value::as_array) {
            return Some(arr);
        }
    }
    None
}

/// Parses home shelves into domain `Shelf` items from an InnerTube browse response.
pub fn parse_home_shelves(value: &Value) -> Vec<Shelf> {
    let mut shelves = Vec::new();
    let Some(sections) = browse_sections(value) else {
        return shelves;
    };
    for section in sections {
        if let Some(shelf_node) = extract_shelf_node(section) {
            if let Some(shelf) = parse_shelf(shelf_node) {
                shelves.push(shelf);
            }
        }
    }
    shelves
}

fn collect_shelf_tracks(items: &[Value], tracks: &mut Vec<Track>) {
    for item in items {
        if let Some(r) = item.get("musicResponsiveListItemRenderer") {
            if let Some(raw) = parse_item(r) {
                tracks.push(raw_to_track(raw));
            }
        }
    }
}

fn collect_tracks_from_section(section: &Value, tracks: &mut Vec<Track>) {
    let shelf =
        section.get("musicShelfRenderer").or_else(|| section.get("musicPlaylistShelfRenderer"));
    if let Some(items) = shelf.and_then(|s| s.get("contents")).and_then(Value::as_array) {
        collect_shelf_tracks(items, tracks);
    }
}

/// Parses album track list from an InnerTube album browse response.
pub fn parse_album_tracks(value: &Value) -> Vec<Track> {
    let mut tracks = Vec::new();
    if let Some(sections) = browse_sections(value) {
        for section in sections {
            collect_tracks_from_section(section, &mut tracks);
        }
    }
    tracks
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

    /// Executes browse request for `browse_id` and returns unparsed JSON response payload.
    pub fn browse_payload(&self, browse_id: &str) -> anyhow::Result<Value> {
        let url = browse_url();
        let body = browse_body(browse_id);
        let response = self.client.post(&url).json(&body).send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("InnerTube browse request failed with status: {status}");
        }
        let payload: Value = response.json()?;
        Ok(payload)
    }

    /// Fetches and parses home shelves from InnerTube.
    pub fn home_shelves(&self) -> anyhow::Result<Vec<Shelf>> {
        self.browse_payload(HOME_BROWSE_ID).map(|p| parse_home_shelves(&p))
    }

    /// Fetches and parses album tracks for `album_id`.
    pub fn album_tracks(&self, album_id: &str) -> anyhow::Result<Vec<Track>> {
        self.browse_payload(album_id).map(|p| parse_album_tracks(&p))
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
    fn best_thumbnail_resolution_and_fallback_table() {
        let renderer_with = |thumbs: Value| {
            serde_json::json!({
                "thumbnail": {
                    "musicThumbnailRenderer": {
                        "thumbnail": {
                            "thumbs": thumbs
                        }
                    }
                }
            })
        };

        // Missing / null / non-array thumbs
        assert_eq!(best_thumbnail(&serde_json::json!({})), None);
        assert_eq!(best_thumbnail(&renderer_with(Value::Null)), None);
        assert_eq!(best_thumbnail(&renderer_with(serde_json::json!("not-an-array"))), None);
        assert_eq!(best_thumbnail(&renderer_with(serde_json::json!([]))), None);

        // Blank and whitespace URLs are skipped
        assert_eq!(
            best_thumbnail(&renderer_with(serde_json::json!([
                { "url": "", "width": 100, "height": 100 },
                { "url": "   ", "width": 200, "height": 200 }
            ]))),
            None
        );

        // Multiple sizes: widest wins
        let multi = renderer_with(serde_json::json!([
            { "url": "https://img.test/small.jpg", "width": 60, "height": 60 },
            { "url": "https://img.test/large.jpg", "width": 540, "height": 540 },
            { "url": "https://img.test/med.jpg", "width": 120, "height": 120 }
        ]));
        assert_eq!(best_thumbnail(&multi), Some("https://img.test/large.jpg".to_owned()));

        // Ties: strictly-greater replacement means first-wins ties
        let ties = renderer_with(serde_json::json!([
            { "url": "https://img.test/first.jpg", "width": 300, "height": 300 },
            { "url": "https://img.test/second.jpg", "width": 300, "height": 300 }
        ]));
        assert_eq!(best_thumbnail(&ties), Some("https://img.test/first.jpg".to_owned()));
    }

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
        assert_eq!(body["context"]["client"]["clientVersion"], YTM_CLIENT_VERSION);
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
                thumb_url: Some("https://lh3.googleusercontent.com/test-thumb-large".to_owned()),
            }
        );
        assert_eq!(
            tracks[1],
            RawTrack {
                video_id: "vid-2".to_owned(),
                title: "Starboy".to_owned(),
                artist: "The Weeknd".to_owned(),
                thumb_url: None,
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
        assert_eq!(first.tracks[0].thumb_url, None);
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

    #[test]
    fn parse_search_tracks_live_shape_and_determinism() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path = std::path::Path::new(manifest_dir)
            .join("tests")
            .join("fixtures")
            .join("innertube_live_shape.json");
        let raw = std::fs::read_to_string(&fixture_path).expect("fixture file readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture json");

        let first = parse_search_tracks(&payload);
        let second = parse_search_tracks(&payload);
        assert_eq!(first, second, "parsing must be deterministic");

        let ids: Vec<&str> = first.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(ids, vec!["live-bare-1", "live-shelf-1"]);
        assert_eq!(first[0].title, "Live Track Bare");
        assert_eq!(first[0].artist, "Live Artist Bare");
        assert_eq!(first[0].thumb_url, None);
        assert_eq!(first[1].title, "Live Track Shelved");
        assert_eq!(first[1].artist, "Live Artist Shelved");
    }

    #[test]
    fn parse_search_all_live_shape_and_determinism() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path = std::path::Path::new(manifest_dir)
            .join("tests")
            .join("fixtures")
            .join("innertube_live_shape.json");
        let raw = std::fs::read_to_string(&fixture_path).expect("fixture file readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture json");

        let first = parse_search_all(&payload);
        let second = parse_search_all(&payload);
        assert_eq!(first, second, "parsing must be deterministic");

        let track_ids: Vec<&str> = first.tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(track_ids, vec!["live-bare-1", "live-shelf-1"]);
        assert!(first.albums.is_empty());
        assert!(first.artists.is_empty());
        assert!(first.playlists.is_empty());
    }

    #[test]
    fn browse_builders_structure() {
        assert!(browse_url().contains("/youtubei/v1/browse"));
        assert_eq!(browse_body("FEmusic_home")["browseId"], "FEmusic_home");
    }

    #[test]
    fn parse_home_shelves_golden_fixture_and_determinism() {
        assert!(parse_home_shelves(&serde_json::json!({})).is_empty());
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = std::path::Path::new(manifest_dir).join("tests/fixtures/innertube_home.json");
        let raw = std::fs::read_to_string(path).expect("fixture readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture");

        let first = parse_home_shelves(&payload);
        assert_eq!(first, parse_home_shelves(&payload), "home shelves must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].title, "Quick picks");
        assert_eq!(first[0].items.len(), 2);
        assert_eq!(
            first[0].items[0],
            SearchResult::Track(Track::new("home-t1", "Home Track", "Home Artist"))
        );
        assert_eq!(
            first[0].items[1],
            SearchResult::Album(Album::new("MPREb_home_1", "Home Album", "Home Artist"))
        );
        assert_eq!(first[1].title, "Artists");
        assert_eq!(first[1].items.len(), 1);
        assert_eq!(
            first[1].items[0],
            SearchResult::Artist(Artist::new("UC_feat_1", "Featured Artist"))
        );
    }

    #[test]
    fn parse_album_tracks_golden_fixture_and_determinism() {
        assert!(parse_album_tracks(&serde_json::json!({})).is_empty());
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path =
            std::path::Path::new(manifest_dir).join("tests/fixtures/innertube_album_detail.json");
        let raw = std::fs::read_to_string(path).expect("fixture readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture");

        let first = parse_album_tracks(&payload);
        assert_eq!(first, parse_album_tracks(&payload), "album tracks must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], Track::new("alb-t1", "Album Track 1", "Band"));
        assert_eq!(first[1], Track::new("alb-t2", "Album Track 2", "Band"));
    }
}
