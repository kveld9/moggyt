//! Radio continuation response parser for watch next payloads.

use super::catalog_model::RawTrack;
use super::catalog_parse_common::{
    best_thumbnail, panel_artist, parse_duration_secs, watch_video_id,
};
use serde_json::Value;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
