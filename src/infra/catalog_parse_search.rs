//! Search response parser for tracks and heterogeneous shelves.

use super::catalog_model::{RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits};
use super::catalog_parse_common::{
    best_thumbnail, browse_of, flex_text, parse_two_row, watch_video_id,
};
use crate::domain::SearchFilter;
use serde_json::Value;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
