//! Detail track row parsing: converts a responsive list item into a domain track.

use super::catalog_parse_common::{
    best_thumbnail, flex_text, music_video_type, panel_artist, parse_duration_secs, watch_video_id,
};
use super::catalog_parse_playlist_row::{row_added, row_album};
use crate::domain::Track;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now_epoch_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
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

/// Extracts track title from a detail item renderer.
pub fn detail_title(renderer: &Value) -> Option<String> {
    flex_text(renderer, 0).or_else(|| {
        let text = renderer.pointer("/title/runs/0/text")?.as_str()?.trim();
        if text.is_empty() { None } else { Some(text.to_owned()) }
    })
}

/// Extracts artist name from a detail item renderer.
pub fn detail_artist(renderer: &Value) -> String {
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

/// Applies the playlist-only album and date-added cells to a parsed track.
fn with_playlist_columns(track: Track, renderer: &Value, now_secs: i64) -> Track {
    let track = match row_album(renderer) {
        Some(album) => track.with_album(album),
        None => track,
    };
    match row_added(renderer, now_secs) {
        Some(added) => track.with_added(added.label, added.epoch_secs),
        None => track,
    }
}

/// Constructs a domain Track from a detail item renderer at a specified epoch timestamp.
pub fn build_detail_track_at(renderer: &Value, now_secs: i64) -> Option<Track> {
    let video_id = detail_video_id(renderer)?;
    let title = detail_title(renderer)?;
    let artist = detail_artist(renderer);
    let mut track = Track::new(&video_id, &title, &artist);
    if let Some(dur) = detail_duration(renderer) {
        track = track.with_duration(dur);
    }
    if let Some(video_type) = music_video_type(renderer) {
        track = track.with_video_type(video_type);
    }
    if let Some(thumb) = best_thumbnail(renderer) {
        track = track.with_artwork(thumb);
    }
    Some(with_playlist_columns(track, renderer, now_secs))
}

/// Constructs a domain Track from a detail item renderer using the current clock.
pub fn build_detail_track(renderer: &Value) -> Option<Track> {
    build_detail_track_at(renderer, now_epoch_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_detail_track_at_reads_album_and_added_columns() {
        let now = 1_800_000_000;
        let renderer = json!({
            "videoId": "video-1",
            "flexColumns": [
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": "Song Title" }] }
                    }
                },
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": "Artist Name" }] }
                    }
                },
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [{
                                "text": "Album Name",
                                "navigationEndpoint": {
                                    "browseEndpoint": {
                                        "browseEndpointContextSupportedConfigs": {
                                            "browseEndpointContextMusicConfig": {
                                                "pageType": "MUSIC_PAGE_TYPE_ALBUM"
                                            }
                                        }
                                    }
                                }
                            }]
                        }
                    }
                },
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": "Added 1 week ago" }] }
                    }
                }
            ],
            "fixedColumns": [{
                "musicResponsiveListItemFixedColumnRenderer": {
                    "text": { "runs": [{ "text": "3:28" }] }
                }
            }]
        });
        let track = build_detail_track_at(&renderer, now).expect("track");
        assert_eq!(track.title, "Song Title");
        assert_eq!(track.artist, "Artist Name");
        assert_eq!(track.album.as_deref(), Some("Album Name"));
        assert_eq!(track.added_label.as_deref(), Some("1 week ago"));
        assert_eq!(track.added_at_secs, Some(now - 604_800));
        assert_eq!(track.duration_secs, Some(208));
    }
}
