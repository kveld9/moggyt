//! Detail view response parser for albums, artists, and playlists.

use super::catalog_parse_common::{
    best_thumbnail, flex_text, panel_artist, parse_duration_secs, watch_video_id,
};
use crate::domain::{Detail, DetailKind, Track};
use serde_json::Value;

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

/// Extracts header description from runs or simple text fallback.
pub fn detail_header_description(value: &Value) -> Option<String> {
    let header = detail_header_node(value)?;
    description_from_runs(header).or_else(|| description_from_simple(header))
}

fn detail_microformat_title(value: &Value) -> Option<String> {
    let text = value.pointer("/microformat/microformatDataRenderer/title")?.as_str()?.trim();
    if text.is_empty() { None } else { Some(text.to_owned()) }
}

fn detail_microformat_description(value: &Value) -> Option<String> {
    let text = value.pointer("/microformat/microformatDataRenderer/description")?.as_str()?.trim();
    if text.is_empty() { None } else { Some(text.to_owned()) }
}

fn detail_microformat_artwork(value: &Value) -> Option<String> {
    let renderer = value.pointer("/microformat/microformatDataRenderer")?;
    best_thumbnail(renderer)
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

/// Constructs a domain Track from a detail item renderer.
pub fn build_detail_track(renderer: &Value) -> Option<Track> {
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

/// Extracts section list array from a detail browse payload.
pub fn detail_sections(value: &Value) -> Option<&[Value]> {
    const CANDIDATES: &[&str] = &[
        "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents",
        "/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents",
        "/sectionListRenderer/contents",
    ];
    CANDIDATES
        .iter()
        .find_map(|path| value.pointer(path).and_then(Value::as_array).map(Vec::as_slice))
}

/// Collects tracks from a shelf contents array.
pub fn collect_shelf_tracks(shelf: &Value, out: &mut Vec<Track>) {
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

/// Collects all tracks from detail browse sections or shelves.
pub fn collect_detail_tracks(value: &Value) -> Vec<Track> {
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

fn resolve_detail_title(value: &Value) -> String {
    let title = detail_header_title(value);
    if !title.is_empty() {
        return title;
    }
    detail_microformat_title(value).unwrap_or_default()
}

fn with_optional_detail_fields(mut detail: Detail, value: &Value) -> Detail {
    let desc = detail_header_description(value).or_else(|| detail_microformat_description(value));
    if let Some(desc) = desc {
        detail = detail.with_description(desc);
    }
    let art = detail_header_artwork(value).or_else(|| detail_microformat_artwork(value));
    if let Some(art) = art {
        detail = detail.with_artwork(art);
    }
    detail
}

/// Parses a detail view (album, artist, or playlist) from catalog API browse response payload.
///
/// Returns `None` when neither a header title nor any valid track items are found.
pub fn parse_detail_all(value: &Value, kind: DetailKind) -> Option<Detail> {
    let title = resolve_detail_title(value);
    let subtitle = detail_header_subtitle(value);
    let tracks = collect_detail_tracks(value);
    if title.is_empty() && tracks.is_empty() {
        return None;
    }
    let detail = Detail::new(kind, &title, &subtitle, tracks);
    Some(with_optional_detail_fields(detail, value))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn parse_detail_all_mix_golden_fixture() {
        let raw = include_str!("../../tests/fixtures/catalog_mix_detail.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let detail = parse_detail_all(&payload, DetailKind::Playlist).expect("parsed detail");
        assert_eq!(detail.kind, DetailKind::Playlist);
        assert_eq!(detail.title, "Clásicos de la Cumbia Argentina");
        assert_eq!(detail.subtitle, "");
        assert_eq!(detail.description.as_deref(), Some("Lista de reproducción • YouTube Music"));
        assert_eq!(
            detail.artwork.as_deref(),
            Some(
                "https://yt3.googleusercontent.com/xkWPhHWhGL36aRI8LVgeavJRd0GohhE2RZYMswdwuk9zqMjlNztDew3QpYi7VAS4DaSpbMW_lIUJ3Vg=w544-h544-l90-rj"
            )
        );
        assert_eq!(detail.tracks.len(), 2);
        assert_eq!(
            detail.tracks[0],
            Track::new("U1FKs6yBbmo", "Yo Tomo Licor", "Amar Azul")
                .with_duration(151)
                .with_artwork("https://yt3.googleusercontent.com/5vFLgD23SHCS1DpEOaviRl9UtOavRu2cv_DPVFuoveHZpPyNmuEGwZX0_otVrfzsO6N3jElN_vOI2cTz=w120-h120-l90-rj")
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
    fn test_resolve_detail_title_precedence() {
        let header_only = serde_json::json!({
            "header": {
                "musicDetailHeaderRenderer": {
                    "title": { "runs": [{ "text": "Header Title" }] }
                }
            }
        });
        assert_eq!(resolve_detail_title(&header_only), "Header Title");

        let microformat_only = serde_json::json!({
            "microformat": {
                "microformatDataRenderer": {
                    "title": "Microformat Title"
                }
            }
        });
        assert_eq!(resolve_detail_title(&microformat_only), "Microformat Title");

        let empty = serde_json::json!({});
        assert_eq!(resolve_detail_title(&empty), "");
    }

    #[test]
    fn test_with_optional_detail_fields_applies_both() {
        let detail = Detail::new(DetailKind::Album, "Title", "Sub", Vec::new());
        let payload = serde_json::json!({
            "header": {
                "musicDetailHeaderRenderer": {
                    "description": { "runs": [{ "text": "Desc" }] },
                    "thumbnail": {
                        "musicThumbnailRenderer": {
                            "thumbnail": {
                                "thumbnails": [{ "url": "https://img.local/art.jpg", "width": 500 }]
                            }
                        }
                    }
                }
            }
        });
        let enriched = with_optional_detail_fields(detail, &payload);
        assert_eq!(enriched.description.as_deref(), Some("Desc"));
        assert_eq!(enriched.artwork.as_deref(), Some("https://img.local/art.jpg"));
    }
}
