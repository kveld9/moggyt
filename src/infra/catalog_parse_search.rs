//! Search response parser for tracks and heterogeneous shelves.

use super::catalog_model::{RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits};
use super::catalog_parse_album::{album_credits, is_type_label};
use super::catalog_parse_common::{
    best_thumbnail, browse_of, browse_page_type, flex_text, music_video_type, parse_duration_secs,
    parse_runs_duration, parse_two_row, watch_video_id,
};
use crate::domain::SearchFilter;
use serde_json::Value;

const DURATION_PATHS: &[&str] = &[
    "/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text",
    "/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/simpleText",
    "/lengthText/runs/0/text",
    "/lengthText/simpleText",
];

fn fixed_or_length_duration(renderer: &Value) -> Option<u32> {
    DURATION_PATHS
        .iter()
        .find_map(|p| renderer.pointer(p).and_then(Value::as_str).and_then(parse_duration_secs))
}

fn col_duration(col: &Value) -> Option<u32> {
    col.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs")
        .and_then(Value::as_array)
        .and_then(|r| parse_runs_duration(r))
}

fn flex_duration(renderer: &Value) -> Option<u32> {
    renderer.get("flexColumns")?.as_array()?.iter().find_map(col_duration)
}

fn search_duration(renderer: &Value) -> Option<u32> {
    fixed_or_length_duration(renderer).or_else(|| flex_duration(renderer))
}

fn col_runs(renderer: &Value, col_idx: usize) -> Option<&[Value]> {
    renderer
        .get("flexColumns")?
        .get(col_idx)?
        .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs")?
        .as_array()
        .map(Vec::as_slice)
}

/// Joins every run of a flex column into one string.
fn column_text(renderer: &Value, col_idx: usize) -> Option<String> {
    let runs = col_runs(renderer, col_idx)?;
    let mut out = String::new();
    for run in runs {
        if let Some(text) = run.get("text").and_then(Value::as_str) {
            out.push_str(text);
        }
    }
    if out.trim().is_empty() { None } else { Some(out) }
}

fn badge_of(renderer: &Value) -> String {
    let Some(runs) = col_runs(renderer, 1) else { return "Song".to_owned() };
    let Some(first) = runs.first().and_then(|r| r.get("text")).and_then(Value::as_str) else {
        return "Song".to_owned();
    };
    let trimmed = first.trim();
    if trimmed.is_empty() { "Song".to_owned() } else { trimmed.to_owned() }
}

const VIDEO_BADGES: &[&str] = &["video", "music video", "video musical", "videoclip"];

pub fn is_video_badge(badge: &str) -> bool {
    let normalized = crate::domain::normalize_for_match(badge);
    VIDEO_BADGES.contains(&normalized.as_str())
}

fn resolve_video_type(renderer: &Value) -> Option<crate::domain::MusicVideoType> {
    music_video_type(renderer).or_else(|| {
        let badge = badge_of(renderer);
        is_video_badge(&badge).then_some(crate::domain::MusicVideoType::Omv)
    })
}

fn is_video_shelf(title: &str) -> bool {
    crate::domain::normalize_for_match(title).contains("video")
}

fn parse_item(renderer: &Value) -> Option<RawTrack> {
    let video_id = watch_video_id(renderer)?;
    let title = flex_text(renderer, 0)?;
    let artist = track_artist(renderer);
    let duration_secs = search_duration(renderer);
    let thumb_url = best_thumbnail(renderer);
    let video_type = resolve_video_type(renderer);
    Some(RawTrack { video_id, title, artist, duration_secs, thumb_url, video_type })
}

fn collect_shelf_items(shelf: &Value, out: &mut Vec<RawTrack>) {
    let Some(items) = shelf.get("contents").and_then(Value::as_array) else {
        return;
    };
    let title = shelf.pointer("/title/runs/0/text").and_then(Value::as_str).unwrap_or("");
    let is_video = is_video_shelf(title);
    for item in items {
        if let Some(mut track) = item.get("musicResponsiveListItemRenderer").and_then(parse_item) {
            if is_video {
                track.video_type = Some(crate::domain::MusicVideoType::Omv);
            }
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

fn collect_section_tracks(section: &Value, out: &mut Vec<RawTrack>) {
    if let Some(shelf) = section.get("musicShelfRenderer") {
        collect_shelf_items(shelf, out);
    } else if let Some(items) =
        section.pointer("/itemSectionRenderer/contents").and_then(Value::as_array)
    {
        for item in items {
            collect_item_or_shelf_track(item, out);
        }
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
    (&["song", "top result", "video"], SearchFilter::Tracks),
    (&["album"], SearchFilter::Albums),
    (&["artist"], SearchFilter::Artists),
    (&["playlist", "mix", "show"], SearchFilter::Playlists),
];

/// Maps a shelf header title to a catalog category filter.
pub fn shelf_category(title: &str) -> Option<SearchFilter> {
    let normalized = crate::domain::normalize_for_match(title);
    CATEGORY_KEYWORDS
        .iter()
        .find(|(kw, _)| kw.iter().any(|k| normalized.contains(k)))
        .map(|(_, f)| *f)
}

fn dispatch_two_row(category: SearchFilter, renderer: &Value, hits: &mut SearchHits) {
    let Some((title, subtitle, browse_id)) = parse_two_row(renderer) else { return };
    let id = browse_id.unwrap_or_default();
    let thumb_url = best_thumbnail(renderer);
    match category {
        SearchFilter::Albums => {
            let credits = album_credits(renderer);
            hits.albums.push(RawAlbum {
                browse_id: id,
                title,
                artist: credits.artist.unwrap_or(subtitle),
                thumb_url,
                artist_id: credits.artist_id,
                release_type: credits.release_type,
            });
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

fn collect_shelf_track_item(is_video: bool, item: &Value, hits: &mut SearchHits) {
    if let Some(mut track) = item.get("musicResponsiveListItemRenderer").and_then(parse_item) {
        if is_video {
            track.video_type = Some(crate::domain::MusicVideoType::Omv);
        }
        hits.tracks.push(track);
    }
}

fn dispatch_shelf_responsive(category: SearchFilter, renderer: &Value, hits: &mut SearchHits) {
    let Some(title) = flex_text(renderer, 0) else { return };
    let artist = artist_of(renderer);
    let cat = browse_page_type(renderer).and_then(page_type_category).unwrap_or(category);
    dispatch_category(cat, renderer, title, artist, hits);
}

fn collect_shelf_item(category: SearchFilter, is_video: bool, item: &Value, hits: &mut SearchHits) {
    if category == SearchFilter::Tracks {
        collect_shelf_track_item(is_video, item, hits);
    } else if let Some(renderer) = item.get("musicTwoRowItemRenderer") {
        dispatch_two_row(category, renderer, hits);
    } else if let Some(renderer) = item.get("musicResponsiveListItemRenderer") {
        dispatch_shelf_responsive(category, renderer, hits);
    }
}

fn collect_shelf(shelf: &Value, hits: &mut SearchHits) {
    let title = shelf.pointer("/title/runs/0/text").and_then(Value::as_str).unwrap_or("");
    let (Some(category), Some(items)) =
        (shelf_category(title), shelf.get("contents").and_then(Value::as_array))
    else {
        return;
    };
    let is_video = is_video_shelf(title);
    for item in items {
        collect_shelf_item(category, is_video, item, hits);
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

/// Resolves a track row artist from the metadata column.
///
/// The column is prefixed with the item type and suffixed with the duration, so both
/// edges are removed while the separators inside the artist list stay untouched.
fn track_artist(renderer: &Value) -> String {
    let Some(raw) = column_text(renderer, 1) else { return String::new() };
    let trimmed = strip_duration_suffix(strip_type_prefix(&raw)).trim();
    if is_clock_duration(trimmed) { String::new() } else { trimmed.to_owned() }
}

fn strip_type_prefix(raw: &str) -> &str {
    let Some((head, tail)) = raw.split_once('\u{2022}') else { return raw };
    if is_type_label(head) { tail.trim() } else { raw }
}

fn strip_duration_suffix(raw: &str) -> &str {
    let Some((head, tail)) = raw.rsplit_once('\u{2022}') else { return raw };
    if is_clock_duration(tail.trim()) { head.trim() } else { raw }
}

fn is_clock_duration(raw: &str) -> bool {
    let mut parts = raw.split(':');
    let (Some(minutes), Some(seconds), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !minutes.is_empty()
        && seconds.len() == 2
        && minutes.chars().all(|c| c.is_ascii_digit())
        && seconds.chars().all(|c| c.is_ascii_digit())
}

fn is_skipped_badge(badge: &str) -> bool {
    matches!(badge, "profile" | "perfil" | "podcast")
}

fn is_album_badge(badge: &str) -> bool {
    matches!(badge, "album" | "ep" | "single" | "sencillo")
}

fn is_artist_badge(badge: &str) -> bool {
    matches!(badge, "artist" | "artista")
}

fn is_playlist_badge(badge: &str) -> bool {
    matches!(badge, "playlist" | "lista de reproduccion" | "mix" | "show")
}

fn page_type_category(page_type: &str) -> Option<SearchFilter> {
    match page_type {
        "MUSIC_PAGE_TYPE_ALBUM" => Some(SearchFilter::Albums),
        "MUSIC_PAGE_TYPE_ARTIST" => Some(SearchFilter::Artists),
        "MUSIC_PAGE_TYPE_PLAYLIST" => Some(SearchFilter::Playlists),
        _ => None,
    }
}

fn badge_category(badge_norm: &str) -> Option<SearchFilter> {
    if is_album_badge(badge_norm) {
        Some(SearchFilter::Albums)
    } else if is_artist_badge(badge_norm) {
        Some(SearchFilter::Artists)
    } else if is_playlist_badge(badge_norm) {
        Some(SearchFilter::Playlists)
    } else {
        None
    }
}

fn is_skipped_item(renderer: &Value, badge_norm: &str) -> bool {
    if is_skipped_badge(badge_norm) {
        return true;
    }
    matches!(
        browse_page_type(renderer),
        Some("MUSIC_PAGE_TYPE_USER_CHANNEL" | "MUSIC_PAGE_TYPE_PODCAST_SHOW")
    )
}

fn resolve_item_category(renderer: &Value, badge_norm: &str) -> Option<SearchFilter> {
    browse_page_type(renderer).and_then(page_type_category).or_else(|| badge_category(badge_norm))
}

fn dispatch_bare_track(renderer: &Value, title: String, artist: String, hits: &mut SearchHits) {
    let Some(video_id) = video_id_of(renderer) else { return };
    let thumb_url = best_thumbnail(renderer);
    let video_type = resolve_video_type(renderer);
    let duration_secs = search_duration(renderer);
    hits.tracks.push(RawTrack { video_id, title, artist, duration_secs, thumb_url, video_type });
}

fn dispatch_category(
    cat: SearchFilter,
    renderer: &Value,
    title: String,
    artist: String,
    hits: &mut SearchHits,
) {
    let Some(browse_id) = browse_of(renderer) else {
        return dispatch_bare_track(renderer, title, artist, hits);
    };
    let thumb_url = best_thumbnail(renderer);
    match cat {
        SearchFilter::Albums => {
            let credits = album_credits(renderer);
            hits.albums.push(RawAlbum {
                browse_id,
                title,
                artist: credits.artist.unwrap_or(artist),
                thumb_url,
                artist_id: credits.artist_id,
                release_type: credits.release_type,
            });
        }
        SearchFilter::Artists => hits.artists.push(RawArtist { browse_id, name: title, thumb_url }),
        SearchFilter::Playlists => hits.playlists.push(RawPlaylist { browse_id, title, thumb_url }),
        _ => dispatch_bare_track(renderer, title, artist, hits),
    }
}

fn dispatch_bare(renderer: &Value, hits: &mut SearchHits) {
    let Some(title) = flex_text(renderer, 0) else { return };
    let artist = artist_of(renderer);
    let badge_norm = crate::domain::normalize_for_match(&badge_of(renderer));

    if is_skipped_item(renderer, &badge_norm) {
        return;
    }
    if let Some(cat) = resolve_item_category(renderer, &badge_norm) {
        dispatch_category(cat, renderer, title, artist, hits);
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

fn collect_section_all(section: &Value, hits: &mut SearchHits) {
    if let Some(shelf) = section.get("musicShelfRenderer") {
        collect_shelf(shelf, hits);
    } else if let Some(items) =
        section.pointer("/itemSectionRenderer/contents").and_then(Value::as_array)
    {
        for item in items {
            collect_item_or_shelf_all(item, hits);
        }
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

#[cfg(test)]
#[path = "catalog_parse_search_tests.rs"]
mod tests;
