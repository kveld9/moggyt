//! InnerTube API client and response parser for YouTube Music search.

use crate::domain::SearchFilter;
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

/// Returns the InnerTube search endpoint URL.
pub fn search_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/search?key={YTM_API_KEY}&prettyPrint=false")
}

/// Returns the InnerTube browse endpoint URL.
pub fn browse_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/browse?key={YTM_API_KEY}&prettyPrint=false")
}

/// Returns the InnerTube like mutation endpoint URL.
pub fn like_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/like/like?key={YTM_API_KEY}&prettyPrint=false")
}

/// Returns the InnerTube remove-like mutation endpoint URL.
pub fn removelike_url() -> String {
    format!(
        "https://music.youtube.com/youtubei/v1/like/removelike?key={YTM_API_KEY}&prettyPrint=false"
    )
}

/// InnerTube browse identifier for saved and owned playlists.
pub const BROWSE_PLAYLISTS: &str = "FEmusic_liked_playlists";

/// InnerTube browse identifier for liked tracks playlist.
pub const BROWSE_LIKED_TRACKS: &str = "LM";

/// InnerTube browse identifier for saved library albums.
pub const BROWSE_ALBUMS: &str = "FEmusic_liked_albums";

/// Builds the minimal WEB_REMIX search payload for `query`.
pub fn search_body(query: &str) -> Value {
    serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": YTM_CLIENT_VERSION,
                "hl": "en",
                "gl": "US",
            }
        },
        "query": query,
    })
}

/// Builds the minimal WEB_REMIX browse payload for `browse_id`.
pub fn browse_body(browse_id: &str) -> Value {
    serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": YTM_CLIENT_VERSION,
                "hl": "en",
                "gl": "US",
            }
        },
        "browseId": browse_id,
    })
}

/// Builds the WEB_REMIX like mutation payload for `video_id`.
pub fn like_body(video_id: &str) -> Value {
    serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": YTM_CLIENT_VERSION,
                "hl": "en",
                "gl": "US",
            }
        },
        "target": {
            "videoId": video_id,
        }
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
    } else if let Some(shelf) = item.get("musicPlaylistShelfRenderer") {
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
    } else if let Some(shelf) = section.get("musicPlaylistShelfRenderer") {
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

fn find_sections(value: &Value) -> Vec<&Value> {
    let mut sections = Vec::new();
    let tabs = value
        .pointer("/contents/singleColumnBrowseResultsRenderer/tabs")
        .or_else(|| value.pointer("/contents/tabbedSearchResultsRenderer/tabs"))
        .and_then(Value::as_array);

    if let Some(tabs) = tabs {
        for tab in tabs {
            let path = "/tabRenderer/content/sectionListRenderer/contents";
            if let Some(s) = tab.pointer(path).and_then(Value::as_array) {
                sections.extend(s);
            }
        }
    } else if let Some(s) =
        value.pointer("/contents/sectionListRenderer/contents").and_then(Value::as_array)
    {
        sections.extend(s);
    }
    sections
}

fn section_items(section: &Value) -> &[Value] {
    if let Some(items) = section.pointer("/gridRenderer/items").and_then(Value::as_array) {
        return items;
    }
    if let Some(items) = section.pointer("/musicShelfRenderer/contents").and_then(Value::as_array) {
        return items;
    }
    if let Some(items) = section.pointer("/itemSectionRenderer/contents").and_then(Value::as_array)
    {
        return items;
    }
    &[]
}

fn parse_item_metadata(renderer: &Value) -> Option<(String, String, String)> {
    if let Some(two_row) = renderer.get("musicTwoRowItemRenderer") {
        let (title, subtitle, id) = parse_two_row(two_row)?;
        return Some((id?, title, subtitle));
    }
    if let Some(list_item) = renderer.get("musicResponsiveListItemRenderer") {
        let title = flex_text(list_item, 0)?;
        let subtitle = flex_text(list_item, 1).unwrap_or_default();
        let id = browse_id(list_item)?;
        return Some((id, title, subtitle));
    }
    None
}

fn collect_from_items<T>(
    items: &[Value],
    transform: &mut impl FnMut(String, String, String) -> T,
    out: &mut Vec<T>,
) {
    for item in items {
        if let Some((id, title, subtitle)) = parse_item_metadata(item) {
            out.push(transform(id, title, subtitle));
        }
    }
}

fn collect_section_two_rows<T>(
    section: &Value,
    transform: &mut impl FnMut(String, String, String) -> T,
    out: &mut Vec<T>,
) {
    let direct = section_items(section);
    collect_from_items(direct, transform, out);
    let sub_items = section
        .get("itemSectionRenderer")
        .and_then(|is| is.get("contents"))
        .and_then(Value::as_array);
    if let Some(subs) = sub_items {
        for sub in subs {
            collect_from_items(section_items(sub), transform, out);
        }
    }
}

fn collect_two_rows<T>(
    value: &Value,
    mut transform: impl FnMut(String, String, String) -> T,
) -> Vec<T> {
    let mut results = Vec::new();
    for section in find_sections(value) {
        collect_section_two_rows(section, &mut transform, &mut results);
    }
    results
}

/// Parses saved or owned playlists from an InnerTube browse response.
pub fn parse_library_playlists(value: &Value) -> Vec<RawPlaylist> {
    collect_two_rows(value, |browse_id, title, _sub| RawPlaylist { browse_id, title })
}

/// Parses saved albums from an InnerTube browse response.
pub fn parse_library_saved_albums(value: &Value) -> Vec<RawAlbum> {
    collect_two_rows(value, |browse_id, title, subtitle| RawAlbum {
        browse_id,
        title,
        artist: subtitle,
    })
}

/// Parses liked tracks from an InnerTube browse response.
pub fn parse_library_liked_tracks(value: &Value) -> Vec<RawTrack> {
    let mut tracks = Vec::new();
    for section in find_sections(value) {
        collect_section_tracks(section, &mut tracks);
    }
    tracks
}

/// Parses the response of a like or remove-like mutation endpoint.
pub fn parse_like_response(value: &Value) -> bool {
    if !value.is_object() {
        return false;
    }
    value.get("error").is_none()
}

/// Blocking HTTP client for the InnerTube API.
#[derive(Debug)]
pub struct InnerTube {
    client: reqwest::blocking::Client,
    api_key: String,
    cookies: Option<String>,
}

impl InnerTube {
    /// Creates a new InnerTube client with a 10-second request timeout and anonymous credentials.
    pub fn new() -> Self {
        Self::with_cookies(None)
    }

    /// Creates an InnerTube client with optional session-cookie authentication.
    pub fn with_cookies(cookies: Option<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        let clean_cookies = cookies.filter(|c| !c.trim().is_empty());
        Self { client, api_key: YTM_API_KEY.to_owned(), cookies: clean_cookies }
    }

    /// Updates session cookies for authenticated requests.
    pub fn set_cookies(&mut self, cookies: Option<String>) {
        self.cookies = cookies.filter(|c| !c.trim().is_empty());
    }

    /// Returns `true` if this client has configured session cookies.
    pub fn is_authenticated(&self) -> bool {
        self.cookies.is_some()
    }

    fn post_json(&self, url: &str, body: &Value) -> anyhow::Result<Value> {
        let mut request = self.client.post(url).json(body);
        if let Some(cookie) = &self.cookies {
            request = request.header("Cookie", cookie);
        }
        let response = request.send()?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("InnerTube request failed with status: {status}");
        }
        let payload: Value = response.json()?;
        Ok(payload)
    }

    /// Executes search request and returns the unparsed JSON response payload.
    pub fn search_payload(&self, query: &str) -> anyhow::Result<Value> {
        let url = format!(
            "https://music.youtube.com/youtubei/v1/search?key={}&prettyPrint=false",
            self.api_key
        );
        let body = search_body(query);
        self.post_json(&url, &body)
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

    /// Executes browse request and returns the unparsed JSON response payload.
    pub fn browse_payload(&self, browse_id: &str) -> anyhow::Result<Value> {
        if !self.is_authenticated() {
            anyhow::bail!("InnerTube browse requires authenticated session cookies");
        }
        let body = browse_body(browse_id);
        self.post_json(&browse_url(), &body)
    }

    /// Fetches saved or owned playlists from the user library.
    pub fn library_playlists(&self) -> anyhow::Result<Vec<RawPlaylist>> {
        let payload = self.browse_payload(BROWSE_PLAYLISTS)?;
        Ok(parse_library_playlists(&payload))
    }

    /// Fetches liked tracks from the user library.
    pub fn library_liked_tracks(&self) -> anyhow::Result<Vec<RawTrack>> {
        let payload = self.browse_payload(BROWSE_LIKED_TRACKS)?;
        Ok(parse_library_liked_tracks(&payload))
    }

    /// Fetches saved albums from the user library.
    pub fn library_saved_albums(&self) -> anyhow::Result<Vec<RawAlbum>> {
        let payload = self.browse_payload(BROWSE_ALBUMS)?;
        Ok(parse_library_saved_albums(&payload))
    }

    /// Executes like or unlike mutation request and returns unparsed payload.
    pub fn set_liked_payload(&self, video_id: &str, liked: bool) -> anyhow::Result<Value> {
        if !self.is_authenticated() {
            anyhow::bail!("InnerTube like mutation requires authenticated session cookies");
        }
        let url = if liked { like_url() } else { removelike_url() };
        let body = like_body(video_id);
        self.post_json(&url, &body)
    }

    /// Mutates the liked status for `video_id`.
    pub fn set_liked(&self, video_id: &str, liked: bool) -> anyhow::Result<()> {
        let payload = self.set_liked_payload(video_id, liked)?;
        if !parse_like_response(&payload) {
            anyhow::bail!("InnerTube like mutation response rejected");
        }
        Ok(())
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
    fn library_urls_and_body_builders() {
        assert_eq!(
            browse_url(),
            format!(
                "https://music.youtube.com/youtubei/v1/browse?key={YTM_API_KEY}&prettyPrint=false"
            )
        );
        assert_eq!(
            like_url(),
            format!(
                "https://music.youtube.com/youtubei/v1/like/like?key={YTM_API_KEY}&prettyPrint=false"
            )
        );
        assert_eq!(
            removelike_url(),
            format!(
                "https://music.youtube.com/youtubei/v1/like/removelike?key={YTM_API_KEY}&prettyPrint=false"
            )
        );

        let browse = browse_body("LM");
        assert_eq!(browse["browseId"], "LM");
        assert_eq!(browse["context"]["client"]["clientName"], "WEB_REMIX");

        let like = like_body("vid-xyz");
        assert_eq!(like["target"]["videoId"], "vid-xyz");
        assert_eq!(like["context"]["client"]["clientName"], "WEB_REMIX");
    }

    #[test]
    fn innertube_auth_lifecycle() {
        let mut tube = InnerTube::new();
        assert!(!tube.is_authenticated());

        tube.set_cookies(Some("   ".into()));
        assert!(!tube.is_authenticated());

        tube.set_cookies(Some("SID=123; HSID=456".into()));
        assert!(tube.is_authenticated());

        tube.set_cookies(None);
        assert!(!tube.is_authenticated());

        let authed = InnerTube::with_cookies(Some("SID=abc".into()));
        assert!(authed.is_authenticated());
    }

    #[test]
    fn unauthenticated_library_methods_bail() {
        let tube = InnerTube::new();
        assert!(tube.browse_payload("LM").is_err());
        assert!(tube.library_playlists().is_err());
        assert!(tube.library_liked_tracks().is_err());
        assert!(tube.library_saved_albums().is_err());
        assert!(tube.set_liked("v1", true).is_err());
    }

    #[test]
    fn parse_library_playlists_fixture_and_determinism() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path = std::path::Path::new(manifest_dir)
            .join("tests/fixtures/innertube_library_playlists.json");
        let raw = std::fs::read_to_string(fixture_path).expect("fixture file readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture json");

        let first = parse_library_playlists(&payload);
        let second = parse_library_playlists(&payload);
        assert_eq!(first, second, "parsing must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(
            first[0],
            RawPlaylist {
                browse_id: "VLPL_synthwave_favs".into(),
                title: "Synthwave Favorites".into(),
            }
        );
        assert_eq!(
            first[1],
            RawPlaylist { browse_id: "VLPL_chill_beats".into(), title: "Chill Beats".into() }
        );
    }

    #[test]
    fn parse_library_liked_tracks_fixture_and_determinism() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path = std::path::Path::new(manifest_dir)
            .join("tests/fixtures/innertube_library_liked_tracks.json");
        let raw = std::fs::read_to_string(fixture_path).expect("fixture file readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture json");

        let first = parse_library_liked_tracks(&payload);
        let second = parse_library_liked_tracks(&payload);
        assert_eq!(first, second, "parsing must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(
            first[0],
            RawTrack {
                video_id: "home-res-1".into(),
                title: "Resonance".into(),
                artist: "HOME".into(),
                thumb_url: Some("https://lh3.googleusercontent.com/home-thumb".into()),
            }
        );
        assert_eq!(
            first[1],
            RawTrack {
                video_id: "mem-reboot-2".into(),
                title: "Memory Reboot".into(),
                artist: "VOJ, Narvent".into(),
                thumb_url: None,
            }
        );
    }

    #[test]
    fn parse_library_saved_albums_fixture_and_determinism() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path = std::path::Path::new(manifest_dir)
            .join("tests/fixtures/innertube_library_saved_albums.json");
        let raw = std::fs::read_to_string(fixture_path).expect("fixture file readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture json");

        let first = parse_library_saved_albums(&payload);
        let second = parse_library_saved_albums(&payload);
        assert_eq!(first, second, "parsing must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(
            first[0],
            RawAlbum {
                browse_id: "MPREb_odyssey".into(),
                title: "Odyssey".into(),
                artist: "HOME".into(),
            }
        );
        assert_eq!(
            first[1],
            RawAlbum {
                browse_id: "MPREb_discovery".into(),
                title: "Discovery".into(),
                artist: "Daft Punk".into(),
            }
        );
    }

    #[test]
    fn parse_like_response_fixture_and_error_cases() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path =
            std::path::Path::new(manifest_dir).join("tests/fixtures/innertube_like_response.json");
        let raw = std::fs::read_to_string(fixture_path).expect("fixture file readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture json");

        assert!(parse_like_response(&payload));
        assert!(parse_like_response(&serde_json::json!({})));
        assert!(!parse_like_response(&serde_json::json!({
            "error": { "code": 401, "message": "unauthorized" }
        })));
        assert!(!parse_like_response(&serde_json::json!("not-an-object")));
    }

    #[test]
    fn parse_library_tolerates_empty_and_malformed() {
        let empty = serde_json::json!({});
        assert!(parse_library_playlists(&empty).is_empty());
        assert!(parse_library_liked_tracks(&empty).is_empty());
        assert!(parse_library_saved_albums(&empty).is_empty());

        let malformed = serde_json::json!({
            "contents": { "singleColumnBrowseResultsRenderer": null }
        });
        assert!(parse_library_playlists(&malformed).is_empty());
        assert!(parse_library_liked_tracks(&malformed).is_empty());
        assert!(parse_library_saved_albums(&malformed).is_empty());
    }
}
