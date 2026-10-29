//! Catalog API client and response parser for YouTube Music search.

use crate::domain::{
    Album, Artist, Detail, DetailKind, Playlist, SearchFilter, SearchResult, Shelf, Track,
};
use crate::i18n::Locale;
use serde_json::Value;
use std::time::Duration;

/// Home feed browse identifier for YouTube Music.
pub const HOME_BROWSE_ID: &str = "FEmusic_home";

/// Explore feed browse identifier for YouTube Music.
pub const EXPLORE_BROWSE_ID: &str = "FEmusic_explore";

/// Library playlists browse identifier for YouTube Music.
pub const LIBRARY_PLAYLISTS_BROWSE_ID: &str = "FEmusic_liked_playlists";

/// Library liked songs browse identifier for YouTube Music.
pub const LIBRARY_LIKED_BROWSE_ID: &str = "FEmusic_liked_videos";

/// Library saved albums browse identifier for YouTube Music.
pub const LIBRARY_ALBUMS_BROWSE_ID: &str = "FEmusic_liked_albums";

/// Public catalog API web key for the WEB_REMIX client context.
///
/// Embedded by every unofficial client; grants no private-data or billable-resource
/// access and cannot be rotated per installation (see `.gitleaks.toml` allowlist).
pub const CATALOG_API_KEY: &str = "AIzaSyC1xlcUPO2SCEmJEWeeIg4X6KyWnhDk-4M";

/// Catalog API client version for the WEB_REMIX client context.
///
/// Verified against live API on 2026-10-24. WEB_REMIX client versions
/// rotate over time; bump this value when YouTube starts rejecting search
/// requests with HTTP 400 FAILED_PRECONDITION. No auto-discovery in v1.
pub const CATALOG_CLIENT_VERSION: &str = "1.20251022.01.00";

/// Returns the catalog API search endpoint URL.
pub fn search_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/search?key={CATALOG_API_KEY}&prettyPrint=false")
}

/// Returns the catalog API next endpoint URL.
pub fn next_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/next?key={CATALOG_API_KEY}&prettyPrint=false")
}

/// Default region for catalog client requests.
pub const CATALOG_CLIENT_GL: &str = "US";

fn client_context(locale: Locale) -> Value {
    serde_json::json!({
        "clientName": "WEB_REMIX",
        "clientVersion": CATALOG_CLIENT_VERSION,
        "hl": locale.tag(),
        "gl": CATALOG_CLIENT_GL,
    })
}

/// Builds the minimal WEB_REMIX search payload for `query` with `locale`.
pub fn search_body(query: &str, locale: Locale) -> Value {
    serde_json::json!({
        "context": {
            "client": client_context(locale),
        },
        "query": query,
    })
}

/// Builds the minimal WEB_REMIX next payload for `video_id` with `locale`.
pub fn next_body(video_id: &str, locale: Locale) -> Value {
    serde_json::json!({
        "context": {
            "client": client_context(locale),
        },
        "videoId": video_id,
        "playlistId": format!("RD{video_id}"),
    })
}

/// Returns the catalog API browse endpoint URL.
pub fn browse_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/browse?key={CATALOG_API_KEY}&prettyPrint=false")
}

/// Builds the minimal WEB_REMIX browse payload for `browse_id` with `locale`.
pub fn browse_body(browse_id: &str, locale: Locale) -> Value {
    serde_json::json!({
        "context": {
            "client": client_context(locale),
        },
        "browseId": browse_id,
    })
}

/// Returns the catalog API like endpoint URL.
pub fn like_url(liked: bool) -> String {
    let action = if liked { "like" } else { "removelike" };
    format!(
        "https://music.youtube.com/youtubei/v1/like/{}?key={}&prettyPrint=false",
        action, CATALOG_API_KEY
    )
}

/// Builds the minimal WEB_REMIX like/removelike payload for `video_id` with `locale`.
pub fn like_body(video_id: &str, locale: Locale) -> Value {
    serde_json::json!({
        "context": {
            "client": client_context(locale),
        },
        "target": {
            "videoId": video_id,
        }
    })
}

/// Validates if a catalog API mutation response succeeded (no error object).
pub fn parse_mutation_response(value: &Value) -> bool {
    value.get("error").is_none()
}

/// Parsed track hit from catalog API search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTrack {
    /// YouTube video identifier.
    pub video_id: String,
    /// Track title.
    pub title: String,
    /// Artist name.
    pub artist: String,
    /// Optional track duration in seconds.
    pub duration_secs: Option<u32>,
    /// Highest-resolution thumbnail URL when known.
    pub thumb_url: Option<String>,
}

/// Parsed album hit from catalog API search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawAlbum {
    pub browse_id: String,
    pub title: String,
    pub artist: String,
    pub thumb_url: Option<String>,
}

/// Parsed artist hit from catalog API search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawArtist {
    pub browse_id: String,
    pub name: String,
    pub thumb_url: Option<String>,
}

/// Parsed playlist hit from catalog API search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPlaylist {
    pub browse_id: String,
    pub title: String,
    pub thumb_url: Option<String>,
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
        "/playlistItemData/videoId",
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
    Some(RawTrack { video_id, title, artist, duration_secs: None, thumb_url })
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

/// Parses search track hits from catalog API search response payload.
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

fn thumbnail_candidates(value: &Value) -> Option<&[Value]> {
    const CANDIDATES: &[&str] = &[
        "/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails",
        "/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbs",
        "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails",
        "/thumbnail/musicThumbnailRenderer/thumbnail/thumbs",
        "/thumbnail/croppedThumbnailRenderer/thumbnail/thumbnails",
        "/thumbnail/croppedThumbnailRenderer/thumbnail/thumbs",
        "/thumbnail/croppedThumbnail/thumbnails",
        "/thumbnail/croppedThumbnail/thumbs",
        "/thumbnailRenderer/croppedThumbnailRenderer/thumbnail/thumbnails",
        "/thumbnailRenderer/croppedThumbnailRenderer/thumbnail/thumbs",
        "/thumbnail/thumbnails",
        "/thumbnail/thumbs",
        "/thumbnails",
        "/thumbs",
    ];
    CANDIDATES
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_array).map(Vec::as_slice))
}

/// Extracts the highest-resolution thumbnail URL from a renderer node.
///
/// Reads `.thumbnail.musicThumbnailRenderer.thumbnail.thumbnails[]` first,
/// falling back to `.thumbs[]` or `.thumbnail.thumbnails[]`.
/// When multiple thumbnails exist, picks the one with the greatest width.
/// In case of ties, the first candidate wins (strictly-greater replacement).
/// Blank URLs and non-array payloads return `None`.
pub fn best_thumbnail(value: &Value) -> Option<String> {
    let thumbs = thumbnail_candidates(value)?;

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

fn browse_of(renderer: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/navigationEndpoint/browseEndpoint/browseId",
        "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/browseEndpoint/browseId",
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

fn browse_id(renderer: &Value) -> Option<String> {
    browse_of(renderer)
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
    let thumb_url = best_thumbnail(renderer);
    match category {
        SearchFilter::Albums => {
            hits.albums.push(RawAlbum { browse_id: id, title, artist: subtitle, thumb_url })
        }
        SearchFilter::Artists => {
            hits.artists.push(RawArtist { browse_id: id, name: title, thumb_url })
        }
        SearchFilter::Playlists => {
            hits.playlists.push(RawPlaylist { browse_id: id, title, thumb_url })
        }
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

fn video_id_of(renderer: &Value) -> Option<String> {
    if let Some(id) = renderer.pointer("/playlistItemData/videoId").and_then(Value::as_str) {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    watch_video_id(renderer)
}

fn col_runs(renderer: &Value, col_idx: usize) -> Option<&[Value]> {
    renderer
        .get("flexColumns")?
        .get(col_idx)?
        .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs")?
        .as_array()
        .map(Vec::as_slice)
}

fn badge_of(renderer: &Value) -> String {
    let Some(runs) = col_runs(renderer, 1) else {
        return "Song".to_owned();
    };
    let Some(first) = runs.first().and_then(|r| r.get("text")).and_then(Value::as_str) else {
        return "Song".to_owned();
    };
    let trimmed = first.trim();
    if trimmed.is_empty() { "Song".to_owned() } else { trimmed.to_owned() }
}

fn artist_of(renderer: &Value) -> String {
    let Some(runs) = col_runs(renderer, 1) else {
        return String::new();
    };
    if runs.len() <= 1 {
        return String::new();
    }
    let mut raw = String::new();
    for run in &runs[1..] {
        if let Some(text) = run.get("text").and_then(Value::as_str) {
            raw.push_str(text);
        }
    }
    let stripped =
        raw.trim_start_matches(|c: char| c == '\u{2022}' || c == '\u{00b7}' || c.is_whitespace());
    stripped.trim_end().to_owned()
}

fn is_skipped_badge(badge: &str) -> bool {
    matches!(badge, "profile" | "podcast")
}

fn is_browse_badge(badge: &str) -> bool {
    matches!(badge, "album" | "ep" | "artist" | "playlist" | "mix" | "show")
}

fn dispatch_bare_track(renderer: &Value, title: String, artist: String, hits: &mut SearchHits) {
    let Some(video_id) = video_id_of(renderer) else { return };
    let thumb_url = best_thumbnail(renderer);
    hits.tracks.push(RawTrack { video_id, title, artist, duration_secs: None, thumb_url });
}

fn dispatch_bare_browse(
    badge: &str,
    renderer: &Value,
    title: String,
    artist: String,
    hits: &mut SearchHits,
) {
    let Some(browse_id) = browse_of(renderer) else { return };
    let thumb_url = best_thumbnail(renderer);
    match badge {
        "album" | "ep" => hits.albums.push(RawAlbum { browse_id, title, artist, thumb_url }),
        "artist" => hits.artists.push(RawArtist { browse_id, name: title, thumb_url }),
        _ => hits.playlists.push(RawPlaylist { browse_id, title, thumb_url }),
    }
}

fn dispatch_bare_single(renderer: &Value, title: String, artist: String, hits: &mut SearchHits) {
    if let Some(browse_id) = browse_of(renderer) {
        let thumb_url = best_thumbnail(renderer);
        hits.albums.push(RawAlbum { browse_id, title, artist, thumb_url });
    } else {
        dispatch_bare_track(renderer, title, artist, hits);
    }
}

fn dispatch_bare(renderer: &Value, hits: &mut SearchHits) {
    let Some(title) = flex_text(renderer, 0) else { return };
    let artist = artist_of(renderer);
    let badge = badge_of(renderer).to_ascii_lowercase();

    if is_skipped_badge(&badge) {
        return;
    }
    if is_browse_badge(&badge) {
        dispatch_bare_browse(&badge, renderer, title, artist, hits);
    } else if badge == "single" {
        dispatch_bare_single(renderer, title, artist, hits);
    } else {
        dispatch_bare_track(renderer, title, artist, hits);
    }
}

fn collect_item_or_shelf_all(item: &Value, hits: &mut SearchHits) {
    if let Some(shelf) = item.get("musicShelfRenderer") {
        collect_shelf(shelf, hits);
    } else if let Some(renderer) = item.get("musicResponsiveListItemRenderer") {
        dispatch_bare(renderer, hits);
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
/// Bare responsive list items inside `itemSectionRenderer` nodes are classified
/// by badge into tracks, albums, artists, or playlists. Shelf title category
/// routing applies inside named shelves.
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

/// Parses duration string formatted as "mm:ss" or "hh:mm:ss" into total seconds.
pub fn parse_duration_secs(raw: &str) -> Option<u32> {
    let parts: Vec<&str> = raw.trim().split(':').collect();
    match parts.as_slice() {
        [mins, secs] => mins.parse::<u32>().ok()?.checked_mul(60)?.checked_add(secs.parse().ok()?),
        [hours, mins, secs] => {
            let total_m =
                hours.parse::<u32>().ok()?.checked_mul(60)?.checked_add(mins.parse().ok()?)?;
            total_m.checked_mul(60)?.checked_add(secs.parse().ok()?)
        }
        _ => None,
    }
}

fn panel_video_id(renderer: &Value) -> Option<String> {
    if let Some(id) = renderer.get("videoId").and_then(Value::as_str) {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    watch_video_id(renderer)
}

fn panel_title(renderer: &Value) -> Option<String> {
    let text = renderer.pointer("/title/runs/0/text")?.as_str()?.trim();
    if text.is_empty() { None } else { Some(text.to_owned()) }
}

fn panel_artist(renderer: &Value) -> String {
    const PATHS: &[&str] =
        &["/shortBylineText/runs/0/text", "/longBylineText/runs/0/text", "/bylineText/runs/0/text"];
    for path in PATHS {
        if let Some(text) = renderer.pointer(path).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }
    }
    String::new()
}

fn panel_duration(renderer: &Value) -> Option<u32> {
    let raw = renderer
        .pointer("/lengthText/runs/0/text")
        .or_else(|| renderer.pointer("/lengthText/simpleText"))?
        .as_str()?;
    parse_duration_secs(raw)
}

fn parse_radio_item(item: &Value) -> Option<RawTrack> {
    let renderer = item.get("playlistPanelVideoRenderer").unwrap_or(item);
    let video_id = panel_video_id(renderer)?;
    let title = panel_title(renderer)?;
    let artist = panel_artist(renderer);
    let duration_secs = panel_duration(renderer);
    let thumb_url = best_thumbnail(renderer);
    Some(RawTrack { video_id, title, artist, duration_secs, thumb_url })
}

fn playlist_panel_contents(value: &Value) -> Option<&[Value]> {
    const CANDIDATES: &[&str] = &[
        "/contents/singleColumnMusicWatchNextResultsRenderer/tabbedRenderer/watchNextTabbedResultsRenderer/tabs/0/tabRenderer/content/musicQueueRenderer/content/playlistPanelRenderer/contents",
        "/contents/singleColumnMusicWatchNextResultsRenderer/tabbedRenderer/watchNextTabbedResultsRenderer/tabs/0/tabRenderer/content/musicQueueRenderer/playlistPanelRenderer/contents",
        "/tabbedRenderer/watchNextTabbedResultsRenderer/tabs/0/tabRenderer/content/musicQueueRenderer/content/playlistPanelRenderer/contents",
        "/tabbedRenderer/watchNextTabbedResultsRenderer/tabs/0/tabRenderer/content/musicQueueRenderer/playlistPanelRenderer/contents",
        "/musicQueueRenderer/content/playlistPanelRenderer/contents",
        "/musicQueueRenderer/playlistPanelRenderer/contents",
        "/playlistPanelRenderer/contents",
    ];
    CANDIDATES
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_array).map(Vec::as_slice))
}

/// Parses radio track hits from catalog API next response payload.
pub fn parse_radio_all(value: &Value) -> Vec<RawTrack> {
    playlist_panel_contents(value)
        .map_or_else(Vec::new, |items| items.iter().filter_map(parse_radio_item).collect())
}

fn home_sections(value: &Value) -> Option<&Vec<Value>> {
    const PATH: &str = "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
    const ALT_PATH: &str = "/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
    value.pointer(PATH).or_else(|| value.pointer(ALT_PATH)).and_then(Value::as_array)
}

fn carousel_shelf_title(shelf: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text",
        "/title/runs/0/text",
        "/header/musicCarouselShelfBasicHeaderRenderer/title/simpleText",
        "/title/simpleText",
    ];
    for path in PATHS {
        if let Some(text) = shelf.pointer(path).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn classify_home_item(
    id: &str,
    title: &str,
    subtitle: &str,
    artwork: Option<String>,
) -> SearchResult {
    if id.starts_with("UC") {
        return SearchResult::Artist(Artist::new(id, title, artwork));
    }
    if id.starts_with("VL") || id.starts_with("RD") || id.starts_with("FEmusic_") {
        return SearchResult::Playlist(Playlist::new(id, title, 0, artwork));
    }
    SearchResult::Album(Album::new(id, title, subtitle, artwork))
}

fn parse_carousel_item(item: &Value) -> Option<SearchResult> {
    let renderer = item.get("musicTwoRowItemRenderer")?;
    let (title, subtitle, browse_id) = parse_two_row(renderer)?;
    let id = browse_id?;
    let thumb = best_thumbnail(renderer);
    Some(classify_home_item(&id, &title, &subtitle, thumb))
}

fn parse_carousel_shelf(shelf: &Value) -> Option<Shelf> {
    let title = carousel_shelf_title(shelf)?;
    let items_array = shelf.get("contents")?.as_array()?;
    let items: Vec<SearchResult> = items_array.iter().filter_map(parse_carousel_item).collect();
    if items.is_empty() {
        return None;
    }
    Some(Shelf { title, items })
}

fn parse_home_section(section: &Value) -> Option<Shelf> {
    let shelf = section.get("musicCarouselShelfRenderer")?;
    parse_carousel_shelf(shelf)
}

/// Parses home carousel shelves from catalog API browse response payload.
///
/// Browse items are classified using the following heuristic on `browseId`:
/// - Starts with `"UC"`: [`SearchResult::Artist`]
/// - Starts with `"VL"` or `"RD"`: [`SearchResult::Playlist`] (with track_count 0)
/// - Otherwise: [`SearchResult::Album`]
///
/// Featured immersive carousels (`musicImmersiveCarouselShelfRenderer`), shelves
/// with empty titles, and shelves with zero kept items are skipped.
pub fn parse_home_all(value: &Value) -> Vec<Shelf> {
    let Some(sections) = home_sections(value) else {
        return Vec::new();
    };
    sections.iter().filter_map(parse_home_section).collect()
}

fn explore_sections(value: &Value) -> Option<&Vec<Value>> {
    const PATH: &str = "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
    const ALT_PATH: &str = "/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
    const DIRECT_PATH: &str = "/sectionListRenderer/contents";
    value
        .pointer(PATH)
        .or_else(|| value.pointer(ALT_PATH))
        .or_else(|| value.pointer(DIRECT_PATH))
        .and_then(Value::as_array)
}

fn explore_shelf_title(shelf: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text",
        "/header/musicShelfBasicHeaderRenderer/title/runs/0/text",
        "/header/gridHeaderRenderer/title/runs/0/text",
        "/header/musicHeaderRenderer/title/runs/0/text",
        "/title/runs/0/text",
        "/header/musicCarouselShelfBasicHeaderRenderer/title/simpleText",
        "/header/musicShelfBasicHeaderRenderer/title/simpleText",
        "/header/gridHeaderRenderer/title/simpleText",
        "/title/simpleText",
    ];
    for path in PATHS {
        if let Some(text) = shelf.pointer(path).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn explore_shelf_items_raw(shelf: &Value) -> Option<&[Value]> {
    shelf
        .get("contents")
        .or_else(|| shelf.get("items"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
}

fn parse_explore_responsive(renderer: &Value) -> Option<SearchResult> {
    if let Some(track) = build_detail_track(renderer) {
        return Some(SearchResult::Track(track));
    }
    let title = detail_title(renderer)?;
    let browse_id = browse_of(renderer)?;
    let artist = detail_artist(renderer);
    let thumb = best_thumbnail(renderer);
    Some(classify_home_item(&browse_id, &title, &artist, thumb))
}

fn parse_explore_two_row(renderer: &Value) -> Option<SearchResult> {
    let (title, subtitle, browse_id) = parse_two_row(renderer)?;
    let thumb = best_thumbnail(renderer);
    if let Some(id) = browse_id {
        return Some(classify_home_item(&id, &title, &subtitle, thumb));
    }
    let vid = watch_video_id(renderer)?;
    let mut track = Track::new(&vid, &title, &subtitle);
    if let Some(t) = thumb {
        track = track.with_artwork(t);
    }
    Some(SearchResult::Track(track))
}

fn parse_explore_button(renderer: &Value) -> Option<SearchResult> {
    let title = renderer
        .pointer("/buttonText/runs/0/text")
        .or_else(|| renderer.pointer("/buttonText/simpleText"))?
        .as_str()?
        .trim();
    if title.is_empty() {
        return None;
    }
    let browse_id = renderer
        .pointer("/clickCommand/browseEndpoint/browseId")
        .or_else(|| renderer.pointer("/navigationEndpoint/browseEndpoint/browseId"))?
        .as_str()?
        .trim();
    if browse_id.is_empty() {
        return None;
    }
    let thumb = best_thumbnail(renderer);
    Some(SearchResult::Playlist(Playlist::new(browse_id, title, 0, thumb)))
}

fn parse_explore_item(item: &Value) -> Option<SearchResult> {
    item.get("musicResponsiveListItemRenderer")
        .map_or_else(|| item.get("flexColumns").map(|_| item), Some)
        .and_then(parse_explore_responsive)
        .or_else(|| {
            item.get("musicTwoRowItemRenderer")
                .map_or_else(|| item.get("subtitle").map(|_| item), Some)
                .and_then(parse_explore_two_row)
        })
        .or_else(|| {
            item.get("musicNavigationButtonRenderer")
                .map_or_else(|| item.get("buttonText").map(|_| item), Some)
                .and_then(parse_explore_button)
        })
}

fn parse_explore_shelf(shelf: &Value) -> Option<Shelf> {
    let title = explore_shelf_title(shelf)?;
    let raw_items = explore_shelf_items_raw(shelf)?;
    let items: Vec<SearchResult> = raw_items.iter().filter_map(parse_explore_item).collect();
    if items.is_empty() {
        return None;
    }
    Some(Shelf { title, items })
}

fn extract_explore_shelf_node(section: &Value) -> Option<&Value> {
    const CANDIDATES: &[&str] = &[
        "/musicCarouselShelfRenderer",
        "/musicShelfRenderer",
        "/gridRenderer",
        "/musicGridRenderer",
        "/itemSectionRenderer/contents/0/musicShelfRenderer",
        "/itemSectionRenderer/contents/0/gridRenderer",
        "/itemSectionRenderer/contents/0/musicGridRenderer",
        "/itemSectionRenderer/contents/0/musicCarouselShelfRenderer",
    ];
    for path in CANDIDATES {
        if let Some(node) = section.pointer(path) {
            return Some(node);
        }
    }
    None
}

fn parse_explore_section(section: &Value) -> Option<Shelf> {
    if let Some(node) = extract_explore_shelf_node(section) {
        if let Some(shelf) = parse_explore_shelf(node) {
            return Some(shelf);
        }
    }
    parse_explore_shelf(section)
}

/// Parses explore carousel, chart, and genre shelves from catalog API browse response payload.
pub fn parse_explore_all(value: &Value) -> Vec<Shelf> {
    let Some(sections) = explore_sections(value) else {
        return Vec::new();
    };
    sections.iter().filter_map(parse_explore_section).collect()
}

fn detail_header_node(value: &Value) -> Option<&Value> {
    const PATHS: &[&str] = &[
        "/header/musicDetailHeaderRenderer",
        "/header/musicVisualHeaderRenderer",
        "/header/musicEditablePlaylistDetailHeaderRenderer/header/musicDetailHeaderRenderer",
        "/header/musicEditablePlaylistDetailHeaderRenderer/header/musicResponsiveHeaderRenderer",
        "/header/musicResponsiveHeaderRenderer",
        "/header/musicHeaderRenderer",
        "/header/musicEditablePlaylistDetailHeaderRenderer",
        "/header",
    ];
    PATHS.iter().find_map(|path| value.pointer(path))
}

fn detail_header_title(value: &Value) -> String {
    const PATHS: &[&str] = &[
        "/title/runs/0/text",
        "/title/simpleText",
        "/header/musicResponsiveHeaderRenderer/title/runs/0/text",
        "/header/musicResponsiveHeaderRenderer/title/simpleText",
        "/header/musicDetailHeaderRenderer/title/runs/0/text",
        "/header/musicDetailHeaderRenderer/title/simpleText",
    ];
    let Some(header) = detail_header_node(value) else {
        return String::new();
    };
    for path in PATHS {
        if let Some(text) = header.pointer(path).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }
    }
    String::new()
}

fn is_meaningful_subtitle(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed != "\u{2022}"
}

fn detail_header_subtitle(value: &Value) -> String {
    const PATHS: &[&str] = &[
        "/subtitle/runs/2/text",
        "/subtitle/runs/1/text",
        "/subtitle/runs/0/text",
        "/subtitle/simpleText",
        "/secondSubtitle/runs/0/text",
        "/secondSubtitle/simpleText",
        "/straplineTextOne/runs/0/text",
        "/header/musicResponsiveHeaderRenderer/subtitle/runs/2/text",
        "/header/musicResponsiveHeaderRenderer/subtitle/runs/1/text",
        "/header/musicResponsiveHeaderRenderer/subtitle/runs/0/text",
        "/header/musicResponsiveHeaderRenderer/subtitle/simpleText",
        "/header/musicResponsiveHeaderRenderer/secondSubtitle/runs/0/text",
        "/description/runs/0/text",
        "/descriptionSnippet/runs/0/text",
        "/description/simpleText",
    ];
    let Some(header) = detail_header_node(value) else {
        return String::new();
    };
    for path in PATHS {
        if let Some(text) = header.pointer(path).and_then(Value::as_str) {
            if is_meaningful_subtitle(text) {
                return text.trim().to_owned();
            }
        }
    }
    String::new()
}

fn join_runs(runs: &[Value]) -> Option<String> {
    let mut text = String::new();
    for run in runs {
        if let Some(t) = run.get("text").and_then(Value::as_str) {
            text.push_str(t);
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

fn description_from_runs(header: &Value) -> Option<String> {
    const RUNS_PATHS: &[&str] = &[
        "/description/runs",
        "/descriptionSnippet/runs",
        "/header/musicResponsiveHeaderRenderer/description/runs",
        "/header/musicResponsiveHeaderRenderer/descriptionSnippet/runs",
    ];
    for path in RUNS_PATHS {
        if let Some(runs) = header.pointer(path).and_then(Value::as_array) {
            if let Some(desc) = join_runs(runs) {
                return Some(desc);
            }
        }
    }
    None
}

fn description_from_simple(header: &Value) -> Option<String> {
    const SIMPLE_PATHS: &[&str] = &[
        "/description/simpleText",
        "/descriptionSnippet/simpleText",
        "/header/musicResponsiveHeaderRenderer/description/simpleText",
        "/header/musicResponsiveHeaderRenderer/descriptionSnippet/simpleText",
    ];
    for path in SIMPLE_PATHS {
        if let Some(text) = header.pointer(path).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn detail_header_description(value: &Value) -> Option<String> {
    let header = detail_header_node(value)?;
    description_from_runs(header).or_else(|| description_from_simple(header))
}

fn detail_header_artwork(value: &Value) -> Option<String> {
    let header = detail_header_node(value)?;
    best_thumbnail(header)
}

fn detail_video_id(renderer: &Value) -> Option<String> {
    if let Some(id) = renderer.get("videoId").and_then(Value::as_str) {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    watch_video_id(renderer)
}

fn detail_title(renderer: &Value) -> Option<String> {
    flex_text(renderer, 0).or_else(|| {
        let text = renderer.pointer("/title/runs/0/text")?.as_str()?.trim();
        if text.is_empty() { None } else { Some(text.to_owned()) }
    })
}

fn detail_artist(renderer: &Value) -> String {
    if let Some(artist) = flex_text(renderer, 1) {
        return artist;
    }
    panel_artist(renderer)
}

fn detail_duration(renderer: &Value) -> Option<u32> {
    const PATHS: &[&str] = &[
        "/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text",
        "/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/simpleText",
        "/lengthText/runs/0/text",
        "/lengthText/simpleText",
    ];
    for path in PATHS {
        if let Some(raw) = renderer.pointer(path).and_then(Value::as_str) {
            if let Some(secs) = parse_duration_secs(raw) {
                return Some(secs);
            }
        }
    }
    None
}

fn build_detail_track(renderer: &Value) -> Option<Track> {
    let video_id = detail_video_id(renderer)?;
    let title = detail_title(renderer)?;
    let artist = detail_artist(renderer);
    let mut track = Track::new(&video_id, &title, &artist);
    if let Some(dur) = detail_duration(renderer) {
        track = track.with_duration(dur);
    }
    if let Some(thumb) = best_thumbnail(renderer) {
        track = track.with_artwork(thumb);
    }
    Some(track)
}

fn detail_sections(value: &Value) -> Option<&[Value]> {
    const CANDIDATES: &[&str] = &[
        "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents",
        "/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents",
        "/sectionListRenderer/contents",
    ];
    CANDIDATES
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_array).map(Vec::as_slice))
}

fn collect_shelf_tracks(shelf: &Value, out: &mut Vec<Track>) {
    let Some(contents) = shelf.get("contents").and_then(Value::as_array) else {
        return;
    };
    for item in contents {
        let renderer = item.get("musicResponsiveListItemRenderer").unwrap_or(item);
        if let Some(track) = build_detail_track(renderer) {
            out.push(track);
        }
    }
}

fn collect_detail_tracks(value: &Value) -> Vec<Track> {
    let mut tracks = Vec::new();
    if let Some(sections) = detail_sections(value) {
        for section in sections {
            let shelf = section
                .get("musicShelfRenderer")
                .or_else(|| section.get("musicPlaylistShelfRenderer"));
            if let Some(s) = shelf {
                collect_shelf_tracks(s, &mut tracks);
            }
        }
        if !tracks.is_empty() {
            return tracks;
        }
    }
    let direct_shelf =
        value.get("musicShelfRenderer").or_else(|| value.get("musicPlaylistShelfRenderer"));
    if let Some(s) = direct_shelf {
        collect_shelf_tracks(s, &mut tracks);
        return tracks;
    }
    collect_shelf_tracks(value, &mut tracks);
    tracks
}

/// Parses a detail view (album, artist, or playlist) from catalog API browse response payload.
///
/// Returns `None` when neither a header title nor any valid track items are found.
pub fn parse_detail_all(value: &Value, kind: DetailKind) -> Option<Detail> {
    let title = detail_header_title(value);
    let subtitle = detail_header_subtitle(value);
    let tracks = collect_detail_tracks(value);
    if title.is_empty() && tracks.is_empty() {
        return None;
    }
    let mut detail = Detail::new(kind, &title, &subtitle, tracks);
    if let Some(desc) = detail_header_description(value) {
        detail = detail.with_description(desc);
    }
    if let Some(art) = detail_header_artwork(value) {
        detail = detail.with_artwork(art);
    }
    Some(detail)
}

fn parse_track_count(text: &str) -> usize {
    text.split_whitespace().find_map(|w| w.parse::<usize>().ok()).unwrap_or(0)
}

fn find_grid_items(section: &Value) -> Option<&[Value]> {
    const CANDIDATES: &[&str] = &[
        "/gridRenderer/items",
        "/musicGridRenderer/items",
        "/musicShelfRenderer/contents",
        "/itemSectionRenderer/contents",
    ];
    CANDIDATES
        .iter()
        .find_map(|path| section.pointer(path).and_then(Value::as_array).map(Vec::as_slice))
}

fn collect_library_items(value: &Value) -> Vec<&Value> {
    if let Some(sections) = detail_sections(value) {
        let items: Vec<&Value> =
            sections.iter().filter_map(find_grid_items).flat_map(|slice| slice.iter()).collect();
        if !items.is_empty() {
            return items;
        }
    }
    find_grid_items(value).map_or_else(Vec::new, |slice| slice.iter().collect())
}

fn parse_library_playlist_item(item: &Value) -> Option<Playlist> {
    let renderer = item.get("musicTwoRowItemRenderer").unwrap_or(item);
    let (title, subtitle, browse_id) = parse_two_row(renderer)?;
    let id = browse_id?;
    let count = parse_track_count(&subtitle);
    let thumb_url = best_thumbnail(renderer);
    Some(Playlist::new(&id, &title, count, thumb_url))
}

/// Parses saved or owned playlists from catalog API browse response payload.
pub fn parse_library_playlists(value: &Value) -> Vec<Playlist> {
    collect_library_items(value).into_iter().filter_map(parse_library_playlist_item).collect()
}

fn parse_library_album_item(item: &Value) -> Option<Album> {
    let renderer = item.get("musicTwoRowItemRenderer").unwrap_or(item);
    let (title, subtitle, browse_id) = parse_two_row(renderer)?;
    let id = browse_id?;
    let thumb_url = best_thumbnail(renderer);
    Some(Album::new(&id, &title, &subtitle, thumb_url))
}

/// Parses saved albums from catalog API browse response payload.
pub fn parse_library_albums(value: &Value) -> Vec<Album> {
    collect_library_items(value).into_iter().filter_map(parse_library_album_item).collect()
}

/// Parses liked tracks from catalog API browse response payload.
pub fn parse_library_liked_tracks(value: &Value) -> Vec<Track> {
    collect_detail_tracks(value)
}

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

    /// Converts this catalog API client into a [`LiveCatalog`](super::ytm::LiveCatalog) adapter.
    pub fn into_catalog(self) -> super::ytm::LiveCatalog {
        super::ytm::LiveCatalog::with_client(self)
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
                "https://music.youtube.com/youtubei/v1/search?key={CATALOG_API_KEY}&prettyPrint=false"
            )
        );
    }

    #[test]
    fn search_body_structure() {
        for (locale, expected_hl) in [(Locale::English, "en"), (Locale::Spanish, "es")] {
            let body = search_body("midnight", locale);
            assert_eq!(body["query"], "midnight");
            assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
            assert_eq!(body["context"]["client"]["clientVersion"], CATALOG_CLIENT_VERSION);
            assert_eq!(body["context"]["client"]["hl"], expected_hl);
            assert_eq!(body["context"]["client"]["gl"], CATALOG_CLIENT_GL);
        }
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
            .join("catalog_search.json");
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
                duration_secs: None,
                thumb_url: Some("https://lh3.googleusercontent.com/test-thumb-large".to_owned()),
            }
        );
        assert_eq!(
            tracks[1],
            RawTrack {
                video_id: "vid-2".to_owned(),
                title: "Starboy".to_owned(),
                artist: "The Weeknd".to_owned(),
                duration_secs: None,
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
            std::path::Path::new(manifest_dir).join("tests/fixtures/catalog_shelves.json");
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
            .join("catalog_live_shape.json");
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
            .join("catalog_live_shape.json");
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
    fn parse_search_all_badges_golden_fixture() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let fixture_path = std::path::Path::new(manifest_dir)
            .join("tests")
            .join("fixtures")
            .join("catalog_search_badges.json");
        let raw = std::fs::read_to_string(&fixture_path).expect("fixture file readable");
        let payload: Value = serde_json::from_str(&raw).expect("valid fixture json");

        let first = parse_search_all(&payload);
        let second = parse_search_all(&payload);
        assert_eq!(first, second, "parsing must be deterministic");

        assert_eq!(first.tracks.len(), 3);
        assert_eq!(first.albums.len(), 1);
        assert_eq!(first.artists.len(), 1);
        assert_eq!(first.playlists.len(), 1);

        // Song item with multi-artist runs
        assert_eq!(first.tracks[0].video_id, "song-vid-1");
        assert_eq!(first.tracks[0].title, "Bachata Rosa");
        assert_eq!(first.tracks[0].artist, "Juan Luis Guerra & 4.40");
        assert_eq!(first.tracks[0].duration_secs, None);
        assert_eq!(
            first.tracks[0].thumb_url,
            Some("https://lh3.googleusercontent.com/test-thumb-large".to_owned())
        );

        // Video item
        assert_eq!(first.tracks[1].video_id, "video-vid-2");
        assert_eq!(first.tracks[1].title, "Propuesta Indecente (Official Video)");
        assert_eq!(first.tracks[1].artist, "Romeo Santos");
        assert_eq!(first.tracks[1].duration_secs, None);
        assert_eq!(
            first.tracks[1].thumb_url,
            Some("https://lh3.googleusercontent.com/video-thumb-large".to_owned())
        );

        // Episode item
        assert_eq!(first.tracks[2].video_id, "ep-vid-6");
        assert_eq!(first.tracks[2].title, "Episode 42: Latin Music History");
        assert_eq!(first.tracks[2].artist, "Latin Beats Podcast");
        assert_eq!(first.tracks[2].duration_secs, None);
        assert_eq!(
            first.tracks[2].thumb_url,
            Some("https://lh3.googleusercontent.com/episode-thumb".to_owned())
        );

        // Single item classified as RawAlbum with browseId routed
        assert_eq!(first.albums[0].browse_id, "MPREb_single_1");
        assert_eq!(first.albums[0].title, "Eres Mia");
        assert_eq!(first.albums[0].artist, "Romeo Santos \u{2022} 2014");
        assert_eq!(
            first.albums[0].thumb_url,
            Some("https://lh3.googleusercontent.com/single-thumb".to_owned())
        );

        // Artist item with browseId routed
        assert_eq!(first.artists[0].browse_id, "UC_romeo_1");
        assert_eq!(first.artists[0].name, "Romeo Santos");
        assert_eq!(
            first.artists[0].thumb_url,
            Some("https://lh3.googleusercontent.com/artist-thumb".to_owned())
        );

        // Playlist item with browseId routed
        assert_eq!(first.playlists[0].browse_id, "VL_playlist_1");
        assert_eq!(first.playlists[0].title, "Bachata Essentials");
        assert_eq!(
            first.playlists[0].thumb_url,
            Some("https://lh3.googleusercontent.com/playlist-thumb".to_owned())
        );
    }

    #[test]
    fn parse_search_all_badges_skips_profile_and_podcast() {
        let payload = serde_json::json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "itemSectionRenderer": {
                                            "contents": [
                                                {
                                                    "musicResponsiveListItemRenderer": {
                                                        "flexColumns": [
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": { "runs": [{ "text": "Some User" }] }
                                                                }
                                                            },
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": { "runs": [
                                                                        { "text": "Profile" },
                                                                        { "text": " \u{2022} " },
                                                                        { "text": "100 subscribers" }
                                                                    ] }
                                                                }
                                                            }
                                                        ],
                                                        "navigationEndpoint": {
                                                            "browseEndpoint": {
                                                                "browseId": "UC_user_1",
                                                                "pageType": "MUSIC_PAGE_TYPE_USER_CHANNEL"
                                                            }
                                                        }
                                                    }
                                                },
                                                {
                                                    "musicResponsiveListItemRenderer": {
                                                        "flexColumns": [
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": { "runs": [{ "text": "Tech Weekly" }] }
                                                                }
                                                            },
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": { "runs": [
                                                                        { "text": "Podcast" },
                                                                        { "text": " \u{2022} " },
                                                                        { "text": "Host" }
                                                                    ] }
                                                                }
                                                            }
                                                        ],
                                                        "navigationEndpoint": {
                                                            "browseEndpoint": {
                                                                "browseId": "MPSP_pod_1",
                                                                "pageType": "MUSIC_PAGE_TYPE_PODCAST_SHOW"
                                                            }
                                                        }
                                                    }
                                                },
                                                {
                                                    "musicResponsiveListItemRenderer": {
                                                        "flexColumns": [
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": { "runs": [{ "text": "No ID Unknown" }] }
                                                                }
                                                            },
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": { "runs": [
                                                                        { "text": "MysteryKind" }
                                                                    ] }
                                                                }
                                                            }
                                                        ]
                                                    }
                                                }
                                            ]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        });

        let hits = parse_search_all(&payload);
        assert!(
            hits.is_empty(),
            "profile, podcast, and unknown without playable ID must be skipped"
        );
    }

    #[test]
    fn parse_search_all_badges_empty_artist_and_defaults() {
        let payload = serde_json::json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "itemSectionRenderer": {
                                            "contents": [
                                                {
                                                    "musicResponsiveListItemRenderer": {
                                                        "flexColumns": [
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": { "runs": [{ "text": "Solo Track" }] }
                                                                }
                                                            },
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": { "runs": [{ "text": "Song" }] }
                                                                }
                                                            }
                                                        ],
                                                        "playlistItemData": {
                                                            "videoId": "solo-vid-1"
                                                        }
                                                    }
                                                },
                                                {
                                                    "musicResponsiveListItemRenderer": {
                                                        "flexColumns": [
                                                            {
                                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                                    "text": { "runs": [{ "text": "No Badge Track" }] }
                                                                }
                                                            }
                                                        ],
                                                        "playlistItemData": {
                                                            "videoId": "no-badge-vid-2"
                                                        }
                                                    }
                                                }
                                            ]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        });

        let hits = parse_search_all(&payload);
        assert_eq!(hits.tracks.len(), 2);
        assert_eq!(hits.tracks[0].video_id, "solo-vid-1");
        assert_eq!(hits.tracks[0].title, "Solo Track");
        assert_eq!(hits.tracks[0].artist, "");
        assert_eq!(hits.tracks[1].video_id, "no-badge-vid-2");
        assert_eq!(hits.tracks[1].title, "No Badge Track");
        assert_eq!(hits.tracks[1].artist, "");
    }

    #[test]
    fn best_thumbnail_plural_key_and_precedence() {
        let plural_only = serde_json::json!({
            "thumbnail": {
                "musicThumbnailRenderer": {
                    "thumbnail": {
                        "thumbnails": [
                            { "url": "https://img.test/p-small.jpg", "width": 100, "height": 100 },
                            { "url": "https://img.test/p-large.jpg", "width": 600, "height": 600 }
                        ]
                    }
                }
            }
        });
        assert_eq!(best_thumbnail(&plural_only), Some("https://img.test/p-large.jpg".to_owned()));

        let both_keys = serde_json::json!({
            "thumbnail": {
                "musicThumbnailRenderer": {
                    "thumbnail": {
                        "thumbnails": [
                            { "url": "https://img.test/plural-wins.jpg", "width": 300, "height": 300 }
                        ],
                        "thumbs": [
                            { "url": "https://img.test/thumbs-fallback.jpg", "width": 800, "height": 800 }
                        ]
                    }
                }
            }
        });
        assert_eq!(best_thumbnail(&both_keys), Some("https://img.test/plural-wins.jpg".to_owned()));
    }

    #[test]
    fn next_url_format_and_key() {
        assert_eq!(
            next_url(),
            format!(
                "https://music.youtube.com/youtubei/v1/next?key={CATALOG_API_KEY}&prettyPrint=false"
            )
        );
    }

    #[test]
    fn next_body_structure() {
        for (locale, expected_hl) in [(Locale::English, "en"), (Locale::Spanish, "es")] {
            let body = next_body("abc-123", locale);
            assert_eq!(body["videoId"], "abc-123");
            assert_eq!(body["playlistId"], "RDabc-123");
            assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
            assert_eq!(body["context"]["client"]["clientVersion"], CATALOG_CLIENT_VERSION);
            assert_eq!(body["context"]["client"]["hl"], expected_hl);
            assert_eq!(body["context"]["client"]["gl"], CATALOG_CLIENT_GL);
        }
    }

    #[test]
    fn parse_duration_secs_table() {
        for (input, expected) in [
            ("3:30", Some(210)),
            ("0:45", Some(45)),
            ("1:02:15", Some(3735)),
            ("0:00", Some(0)),
            ("", None),
            ("invalid", None),
            ("1:2:3:4", None),
        ] {
            assert_eq!(parse_duration_secs(input), expected, "failed for: {input}");
        }
    }

    #[test]
    fn parse_radio_all_tolerates_empty_and_malformed() {
        assert!(parse_radio_all(&serde_json::json!({})).is_empty());
        assert!(parse_radio_all(&serde_json::json!({"musicQueueRenderer": null})).is_empty());
        assert!(
            parse_radio_all(&serde_json::json!({
                "musicQueueRenderer": { "content": { "playlistPanelRenderer": { "contents": [] } } }
            }))
            .is_empty()
        );
    }

    #[test]
    fn parse_radio_all_golden_fixture() {
        let raw = include_str!("../../tests/fixtures/catalog_radio.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let tracks = parse_radio_all(&payload);
        assert_eq!(tracks.len(), 2);
        assert_eq!(
            tracks[0],
            RawTrack {
                video_id: "radio-1".to_owned(),
                title: "Radio Song 1".to_owned(),
                artist: "Radio Artist 1".to_owned(),
                duration_secs: Some(210),
                thumb_url: Some("https://lh3.googleusercontent.com/radio-thumb-1".to_owned()),
            }
        );
        assert_eq!(
            tracks[1],
            RawTrack {
                video_id: "radio-2".to_owned(),
                title: "Radio Song 2".to_owned(),
                artist: "Radio Artist 2".to_owned(),
                duration_secs: None,
                thumb_url: None,
            }
        );
    }

    #[test]
    fn browse_url_format_and_key() {
        assert_eq!(
            browse_url(),
            format!(
                "https://music.youtube.com/youtubei/v1/browse?key={CATALOG_API_KEY}&prettyPrint=false"
            )
        );
    }

    #[test]
    fn browse_body_structure() {
        for (locale, expected_hl) in [(Locale::English, "en"), (Locale::Spanish, "es")] {
            let body = browse_body(HOME_BROWSE_ID, locale);
            assert_eq!(body["browseId"], HOME_BROWSE_ID);
            assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
            assert_eq!(body["context"]["client"]["clientVersion"], CATALOG_CLIENT_VERSION);
            assert_eq!(body["context"]["client"]["hl"], expected_hl);
            assert_eq!(body["context"]["client"]["gl"], CATALOG_CLIENT_GL);
        }
    }

    #[test]
    fn parse_home_all_tolerates_empty_and_malformed() {
        assert!(parse_home_all(&serde_json::json!({})).is_empty());
        assert!(parse_home_all(&serde_json::json!({"contents": null})).is_empty());
        assert!(
            parse_home_all(&serde_json::json!({
                "contents": {
                    "singleColumnBrowseResultsRenderer": {
                        "tabs": [{
                            "tabRenderer": {
                                "content": {
                                    "sectionListRenderer": { "contents": [] }
                                }
                            }
                        }]
                    }
                }
            }))
            .is_empty()
        );
    }

    #[test]
    fn parse_home_all_golden_fixture() {
        let raw = include_str!("../../tests/fixtures/catalog_home.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let shelves = parse_home_all(&payload);
        assert_eq!(shelves.len(), 2);

        assert_eq!(shelves[0].title, "Quick picks");
        assert_eq!(shelves[0].items.len(), 2);
        assert_eq!(
            shelves[0].items[0],
            SearchResult::Album(Album::new(
                "MPREb_album_1",
                "Midnight City",
                "M83",
                Some("https://img.test/album1.jpg".to_owned())
            ))
        );
        assert_eq!(
            shelves[0].items[1],
            SearchResult::Artist(Artist::new("UC_artist_1", "M83", None))
        );

        assert_eq!(shelves[1].title, "Recommended mixes");
        assert_eq!(shelves[1].items.len(), 2);
        assert_eq!(
            shelves[1].items[0],
            SearchResult::Playlist(Playlist::new("VL_mix_1", "Chill Mix", 0, None))
        );
        assert_eq!(
            shelves[1].items[1],
            SearchResult::Playlist(Playlist::new("RD_radio_2", "Energy Radio", 0, None))
        );
    }

    #[test]
    fn parse_home_carousel_item_with_thumbnail_renderer_real_shape() {
        let payload = serde_json::json!({
            "musicTwoRowItemRenderer": {
                "title": { "runs": [{ "text": "Home Album Title" }] },
                "subtitle": { "runs": [{ "text": "Home Artist" }] },
                "navigationEndpoint": {
                    "browseEndpoint": { "browseId": "MPREb_home_1" }
                },
                "thumbnail": null,
                "thumbnailRenderer": {
                    "musicThumbnailRenderer": {
                        "thumbnail": {
                            "thumbnails": [
                                {
                                    "url": "https://yt3.googleusercontent.com/home-small.jpg",
                                    "width": 60,
                                    "height": 60
                                },
                                {
                                    "url": "https://yt3.googleusercontent.com/home-large.jpg",
                                    "width": 544,
                                    "height": 544
                                }
                            ]
                        }
                    }
                }
            }
        });

        let item = parse_carousel_item(&payload).expect("carousel item parsed");
        assert_eq!(
            item,
            SearchResult::Album(Album::new(
                "MPREb_home_1",
                "Home Album Title",
                "Home Artist",
                Some("https://yt3.googleusercontent.com/home-large.jpg".to_owned())
            ))
        );
    }

    #[test]
    fn parse_detail_all_golden_fixture_and_empty() {
        let raw = include_str!("../../tests/fixtures/catalog_detail.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let detail = parse_detail_all(&payload, DetailKind::Album).expect("parsed detail");
        assert_eq!(detail.kind, DetailKind::Album);
        assert_eq!(detail.title, "Test Album Title");
        assert_eq!(detail.subtitle, "Test Artist");
        assert_eq!(detail.description, None);
        assert_eq!(detail.artwork, None);
        assert_eq!(detail.tracks.len(), 2);
        assert_eq!(
            detail.tracks[0],
            Track::new("track-vid-1", "Full Track", "Full Artist")
                .with_duration(225)
                .with_artwork("https://img.local/t1.jpg")
        );
        assert_eq!(detail.tracks[1], Track::new("track-vid-2", "No Length Track", "Second Artist"));

        assert_eq!(parse_detail_all(&serde_json::json!({}), DetailKind::Album), None);
        assert_eq!(
            parse_detail_all(
                &serde_json::json!({"header": null, "contents": null}),
                DetailKind::Artist
            ),
            None
        );
    }

    #[test]
    fn parse_detail_all_playlist_golden_fixture() {
        let raw = include_str!("../../tests/fixtures/catalog_playlist_detail.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let detail = parse_detail_all(&payload, DetailKind::Playlist).expect("parsed detail");
        assert_eq!(detail.kind, DetailKind::Playlist);
        assert_eq!(detail.title, "Test Playlist Title");
        assert_eq!(detail.subtitle, "Test Creator");
        assert_eq!(detail.description.as_deref(), Some("Test Playlist Description"));
        assert_eq!(detail.artwork.as_deref(), Some("https://img.local/playlist_header_large.jpg"));
        assert_eq!(detail.tracks.len(), 1);
        assert_eq!(
            detail.tracks[0],
            Track::new("p-track-1", "Playlist Track 1", "Playlist Artist 1")
                .with_duration(252)
                .with_artwork("https://img.local/p_t1.jpg")
        );
    }

    #[test]
    fn parse_detail_header_description_runs_and_fallbacks() {
        let multi_runs = serde_json::json!({
            "header": {
                "musicResponsiveHeaderRenderer": {
                    "description": {
                        "runs": [
                            { "text": "Part 1 - " },
                            { "text": "Part 2" }
                        ]
                    }
                }
            }
        });
        assert_eq!(detail_header_description(&multi_runs), Some("Part 1 - Part 2".to_owned()));

        let snippet_runs = serde_json::json!({
            "header": {
                "musicDetailHeaderRenderer": {
                    "descriptionSnippet": {
                        "runs": [{ "text": "Snippet text" }]
                    }
                }
            }
        });
        assert_eq!(detail_header_description(&snippet_runs), Some("Snippet text".to_owned()));

        let empty_desc = serde_json::json!({
            "header": {
                "musicResponsiveHeaderRenderer": {
                    "description": { "runs": [{ "text": "   " }] }
                }
            }
        });
        assert_eq!(detail_header_description(&empty_desc), None);
    }

    #[test]
    fn catalog_client_detail_blank_id_returns_none() {
        let client = CatalogClient::new();
        assert_eq!(client.detail("", DetailKind::Album), None);
        assert_eq!(client.detail("   ", DetailKind::Artist), None);
    }

    #[test]
    fn parse_library_playlists_golden_fixture_and_determinism() {
        let raw = include_str!("../../tests/fixtures/catalog_library_playlists.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let first = parse_library_playlists(&payload);
        let second = parse_library_playlists(&payload);
        assert_eq!(first, second, "parsing must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], Playlist::new("VL_fav_mix", "Favorites Mix", 25, None));
        assert_eq!(first[1], Playlist::new("VL_workout", "Workout Beats", 10, None));
    }

    #[test]
    fn parse_library_playlists_tolerates_empty_and_malformed() {
        assert!(parse_library_playlists(&serde_json::json!({})).is_empty());
        assert!(parse_library_playlists(&serde_json::json!({"contents": null})).is_empty());
    }

    #[test]
    fn parse_library_liked_tracks_golden_fixture_and_determinism() {
        let raw = include_str!("../../tests/fixtures/catalog_library_liked.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let first = parse_library_liked_tracks(&payload);
        let second = parse_library_liked_tracks(&payload);
        assert_eq!(first, second, "parsing must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(
            first[0],
            Track::new("lib-t1", "Liked Song 1", "Artist One")
                .with_duration(200)
                .with_artwork("https://img.local/t1.jpg")
        );
        assert_eq!(first[1], Track::new("lib-t2", "Liked Song 2", "Artist Two"));
    }

    #[test]
    fn parse_library_liked_tracks_tolerates_empty_and_malformed() {
        assert!(parse_library_liked_tracks(&serde_json::json!({})).is_empty());
        assert!(parse_library_liked_tracks(&serde_json::json!({"contents": null})).is_empty());
    }

    #[test]
    fn parse_library_albums_golden_fixture_and_determinism() {
        let raw = include_str!("../../tests/fixtures/catalog_library_albums.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let first = parse_library_albums(&payload);
        let second = parse_library_albums(&payload);
        assert_eq!(first, second, "parsing must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], Album::new("MPREb_saved_1", "Saved Album 1", "Album Artist 1", None));
        assert_eq!(first[1], Album::new("MPREb_saved_2", "Saved Album 2", "Album Artist 2", None));
    }

    #[test]
    fn parse_library_albums_tolerates_empty_and_malformed() {
        assert!(parse_library_albums(&serde_json::json!({})).is_empty());
        assert!(parse_library_albums(&serde_json::json!({"contents": null})).is_empty());
    }

    #[test]
    fn like_endpoints_and_mutation_parsing() {
        assert!(like_url(true).contains("/like/like?key="));
        assert!(like_url(false).contains("/like/removelike?key="));
        for (locale, expected_hl) in [(Locale::English, "en"), (Locale::Spanish, "es")] {
            let body = like_body("lib-t1", locale);
            assert_eq!(body["target"]["videoId"], "lib-t1");
            assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
            assert_eq!(body["context"]["client"]["hl"], expected_hl);
            assert_eq!(body["context"]["client"]["gl"], CATALOG_CLIENT_GL);
        }
        assert!(parse_mutation_response(&serde_json::json!({})));
        assert!(!parse_mutation_response(&serde_json::json!({"error": {"code": 401}})));
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

    #[test]
    fn explore_browse_id_constant() {
        assert_eq!(EXPLORE_BROWSE_ID, "FEmusic_explore");
    }

    #[test]
    fn parse_explore_all_tolerates_empty_and_malformed() {
        assert!(parse_explore_all(&serde_json::json!({})).is_empty());
        assert!(parse_explore_all(&serde_json::json!({"contents": null})).is_empty());
        assert!(
            parse_explore_all(&serde_json::json!({
                "contents": {
                    "singleColumnBrowseResultsRenderer": {
                        "tabs": [{
                            "tabRenderer": {
                                "content": {
                                    "sectionListRenderer": { "contents": [] }
                                }
                            }
                        }]
                    }
                }
            }))
            .is_empty()
        );
    }

    #[test]
    fn parse_explore_all_golden_fixture() {
        let payload = serde_json::json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicShelfRenderer": {
                                                "title": { "runs": [{ "text": "Charts" }] },
                                                "contents": [
                                                    {
                                                        "musicResponsiveListItemRenderer": {
                                                            "navigationEndpoint": {
                                                                "watchEndpoint": { "videoId": "chart-vid-1" }
                                                            },
                                                            "flexColumns": [
                                                                {
                                                                    "musicResponsiveListItemFlexColumnRenderer": {
                                                                        "text": { "runs": [{ "text": "Chart Song 1" }] }
                                                                    }
                                                                },
                                                                {
                                                                    "musicResponsiveListItemFlexColumnRenderer": {
                                                                        "text": { "runs": [{ "text": "Chart Artist" }] }
                                                                    }
                                                                }
                                                            ],
                                                            "thumbnailRenderer": {
                                                                "musicThumbnailRenderer": {
                                                                    "thumbnail": {
                                                                        "thumbnails": [
                                                                            {
                                                                                "url": "https://img.test/chart1.jpg",
                                                                                "width": 300,
                                                                                "height": 300
                                                                            }
                                                                        ]
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        },
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": { "runs": [{ "text": "New releases" }] }
                                                    }
                                                },
                                                "contents": [
                                                    {
                                                        "musicTwoRowItemRenderer": {
                                                            "title": { "runs": [{ "text": "New Album" }] },
                                                            "subtitle": { "runs": [{ "text": "Album Artist" }] },
                                                            "navigationEndpoint": {
                                                                "browseEndpoint": { "browseId": "MPREb_new_album" }
                                                            },
                                                            "thumbnailRenderer": {
                                                                "musicThumbnailRenderer": {
                                                                    "thumbnail": {
                                                                        "thumbnails": [
                                                                            {
                                                                                "url": "https://img.test/new_album.jpg",
                                                                                "width": 500,
                                                                                "height": 500
                                                                            }
                                                                        ]
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        },
                                        {
                                            "gridRenderer": {
                                                "header": {
                                                    "gridHeaderRenderer": {
                                                        "title": { "runs": [{ "text": "Moods & genres" }] }
                                                    }
                                                },
                                                "items": [
                                                    {
                                                        "musicNavigationButtonRenderer": {
                                                            "buttonText": { "runs": [{ "text": "Chill" }] },
                                                            "clickCommand": {
                                                                "browseEndpoint": { "browseId": "FEmusic_moods_and_genres_category" }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        }
                                    ]
                                }
                            }
                        }}]
                    }
                }
        });

        let shelves = parse_explore_all(&payload);
        assert_eq!(shelves.len(), 3);

        assert_eq!(shelves[0].title, "Charts");
        assert_eq!(shelves[0].items.len(), 1);
        assert_eq!(
            shelves[0].items[0],
            SearchResult::Track(
                Track::new("chart-vid-1", "Chart Song 1", "Chart Artist")
                    .with_artwork("https://img.test/chart1.jpg")
            )
        );

        assert_eq!(shelves[1].title, "New releases");
        assert_eq!(shelves[1].items.len(), 1);
        assert_eq!(
            shelves[1].items[0],
            SearchResult::Album(Album::new(
                "MPREb_new_album",
                "New Album",
                "Album Artist",
                Some("https://img.test/new_album.jpg".to_owned())
            ))
        );

        assert_eq!(shelves[2].title, "Moods & genres");
        assert_eq!(shelves[2].items.len(), 1);
        assert_eq!(
            shelves[2].items[0],
            SearchResult::Playlist(Playlist::new(
                "FEmusic_moods_and_genres_category",
                "Chill",
                0,
                None
            ))
        );
    }

    #[test]
    fn parse_explore_charts_and_moods_genres_extended() {
        let payload = serde_json::json!({
            "sectionListRenderer": {
                "contents": [
                    {
                        "musicShelfRenderer": {
                            "title": { "simpleText": "Top Charts" },
                            "contents": [
                                {
                                    "musicResponsiveListItemRenderer": {
                                        "playlistItemData": { "videoId": "chart-vid-2" },
                                        "flexColumns": [
                                            {
                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                    "text": { "runs": [{ "text": "Chart Hit 2" }] }
                                                }
                                            },
                                            {
                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                    "text": { "runs": [{ "text": "Chart Artist 2" }] }
                                                }
                                            }
                                        ]
                                    }
                                }
                            ]
                        }
                    },
                    {
                        "musicGridRenderer": {
                            "header": {
                                "gridHeaderRenderer": {
                                    "title": { "simpleText": "Moods & Genres" }
                                }
                            },
                            "items": [
                                {
                                    "musicNavigationButtonRenderer": {
                                        "buttonText": { "simpleText": "Workout" },
                                        "navigationEndpoint": {
                                            "browseEndpoint": {
                                                "browseId": "FEmusic_moods_and_genres_category_workout"
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                ]
            }
        });

        let shelves = parse_explore_all(&payload);
        assert_eq!(shelves.len(), 2);
        assert_eq!(shelves[0].title, "Top Charts");
        assert_eq!(shelves[0].items.len(), 1);
        assert_eq!(
            shelves[0].items[0],
            SearchResult::Track(Track::new("chart-vid-2", "Chart Hit 2", "Chart Artist 2"))
        );
        assert_eq!(shelves[1].title, "Moods & Genres");
        assert_eq!(shelves[1].items.len(), 1);
        assert_eq!(
            shelves[1].items[0],
            SearchResult::Playlist(Playlist::new(
                "FEmusic_moods_and_genres_category_workout",
                "Workout",
                0,
                None
            ))
        );
    }

    #[test]
    fn parse_explore_all_skips_empty_shelves() {
        let payload = serde_json::json!({
            "sectionListRenderer": {
                "contents": [
                    {
                        "musicShelfRenderer": {
                            "title": { "runs": [{ "text": "Empty Shelf" }] },
                            "contents": []
                        }
                    },
                    {
                        "musicShelfRenderer": {
                            "contents": [
                                {
                                    "musicTwoRowItemRenderer": {
                                        "title": { "runs": [{ "text": "Item without Shelf Title" }] }
                                    }
                                }
                            ]
                        }
                    }
                ]
            }
        });
        assert!(parse_explore_all(&payload).is_empty());
    }
}
