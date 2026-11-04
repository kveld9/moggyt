//! Search response parser for tracks and heterogeneous shelves.

use super::catalog_model::{RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits};
use super::catalog_parse_common::{
    best_thumbnail, browse_of, flex_text, music_video_type, parse_duration_secs, parse_two_row,
    watch_video_id,
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

fn run_duration(run: &Value) -> Option<u32> {
    run.get("text").and_then(Value::as_str).and_then(parse_duration_secs)
}

fn col_duration(col: &Value) -> Option<u32> {
    let runs = col.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs")?.as_array()?;
    runs.iter().find_map(run_duration)
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
    let artist = flex_text(renderer, 1).unwrap_or_default();
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

fn collect_shelf_item(category: SearchFilter, is_video: bool, item: &Value, hits: &mut SearchHits) {
    if category == SearchFilter::Tracks {
        if let Some(mut track) = item.get("musicResponsiveListItemRenderer").and_then(parse_item) {
            if is_video {
                track.video_type = Some(crate::domain::MusicVideoType::Omv);
            }
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

fn is_skipped_badge(badge: &str) -> bool {
    matches!(badge, "profile" | "podcast")
}

fn is_browse_badge(badge: &str) -> bool {
    matches!(badge, "album" | "ep" | "artist" | "playlist" | "mix" | "show")
}

fn dispatch_bare_track(renderer: &Value, title: String, artist: String, hits: &mut SearchHits) {
    let Some(video_id) = video_id_of(renderer) else { return };
    let thumb_url = best_thumbnail(renderer);
    let video_type = resolve_video_type(renderer);
    hits.tracks.push(RawTrack {
        video_id,
        title,
        artist,
        duration_secs: search_duration(renderer),
        thumb_url,
        video_type,
    });
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

    fn fixture_json(name: &str) -> Value {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture readable"))
            .expect("valid fixture")
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
        let tracks = parse_search_tracks(&fixture_json("catalog_search.json"));
        assert_eq!(tracks.len(), 2);
        assert_eq!(
            (tracks[0].video_id.as_str(), tracks[0].title.as_str(), tracks[0].artist.as_str()),
            ("vid-1", "Midnight City", "M83")
        );
        assert_eq!(
            tracks[0].thumb_url.as_deref(),
            Some("https://lh3.googleusercontent.com/test-thumb-large")
        );
        assert_eq!(
            (tracks[1].video_id.as_str(), tracks[1].title.as_str(), tracks[1].artist.as_str()),
            ("vid-2", "Starboy", "The Weeknd")
        );
        assert_eq!(tracks[1].thumb_url, None);
    }

    #[test]
    fn parse_search_tracks_songs_fixture_extracts_video_type() {
        let tracks = parse_search_tracks(&fixture_json("catalog_search_songs.json"));
        assert!(!tracks.is_empty());
        assert!(tracks.iter().all(|t| t.video_type == Some(crate::domain::MusicVideoType::Atv)));
        assert_eq!(tracks[0].duration_secs, Some(224));
        assert_eq!(tracks[2].duration_secs, Some(156));
    }

    #[test]
    fn search_duration_extracts_from_fixed_length_and_flex_columns() {
        let fixed_run = serde_json::json!({
            "fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"runs": [{"text": "3:45"}]}}}]
        });
        let fixed_simple = serde_json::json!({
            "fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"simpleText": "1:20"}}}]
        });
        let length_run = serde_json::json!({"lengthText": {"runs": [{"text": "2:30"}]}});
        let length_simple = serde_json::json!({"lengthText": {"simpleText": "4:00"}});
        let flex = serde_json::json!({
            "flexColumns": [{
                "musicResponsiveListItemFlexColumnRenderer": {
                    "text": {"runs": [{"text": "Artist"}, {"text": "3:44"}]}
                }
            }]
        });

        assert_eq!(search_duration(&fixed_run), Some(225));
        assert_eq!(search_duration(&fixed_simple), Some(80));
        assert_eq!(search_duration(&length_run), Some(150));
        assert_eq!(search_duration(&length_simple), Some(240));
        assert_eq!(search_duration(&flex), Some(224));
        assert_eq!(search_duration(&serde_json::json!({})), None);
    }

    fn tabbed_sections(sections: Vec<Value>) -> Value {
        serde_json::json!({
            "contents": { "tabbedSearchResultsRenderer": { "tabs": [{
                "tabRenderer": { "content": { "sectionListRenderer": { "contents": sections } } }
            }] } }
        })
    }

    fn test_responsive_item(title: &str, badge_runs: &[&str], endpoint: Value) -> Value {
        let runs: Vec<_> = badge_runs.iter().map(|t| serde_json::json!({ "text": t })).collect();
        serde_json::json!({
            "musicResponsiveListItemRenderer": {
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": title }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": runs } } }
                ],
                "navigationEndpoint": endpoint
            }
        })
    }

    #[test]
    fn parse_search_tracks_skips_item_without_title() {
        let item = serde_json::json!({
            "musicResponsiveListItemRenderer": {
                "navigationEndpoint": { "watchEndpoint": { "videoId": "v1" } }
            }
        });
        let payload = tabbed_sections(vec![serde_json::json!({
            "musicShelfRenderer": { "contents": [item] }
        })]);
        assert!(parse_search_tracks(&payload).is_empty());
    }

    #[test]
    fn shelf_category_routing_table() {
        for (title, expected) in [
            ("Songs", Some(SearchFilter::Tracks)),
            ("Top result", Some(SearchFilter::Tracks)),
            ("Top songs", Some(SearchFilter::Tracks)),
            ("Videos", Some(SearchFilter::Tracks)),
            ("Vídeos", Some(SearchFilter::Tracks)),
            ("Music videos", Some(SearchFilter::Tracks)),
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
        let payload = fixture_json("catalog_shelves.json");
        let first = parse_search_all(&payload);
        assert_eq!(first, parse_search_all(&payload), "parsing must be deterministic");
        assert_eq!(
            (first.tracks.len(), first.albums.len(), first.artists.len(), first.playlists.len()),
            (1, 1, 1, 0)
        );
        assert_eq!(
            (
                first.tracks[0].video_id.as_str(),
                first.tracks[0].title.as_str(),
                first.tracks[0].artist.as_str()
            ),
            ("solar-1", "Solaris", "Photay")
        );
        assert_eq!(first.tracks[0].thumb_url, None);
        assert_eq!(
            (
                first.albums[0].browse_id.as_str(),
                first.albums[0].title.as_str(),
                first.albums[0].artist.as_str()
            ),
            ("MPREb_album_1", "Waking Season", "Photay")
        );
        assert_eq!(
            (first.artists[0].browse_id.as_str(), first.artists[0].name.as_str()),
            ("UC_photay_1", "Photay")
        );
    }

    #[test]
    fn parse_search_all_ignores_unknown_shelves_and_tolerates_empty() {
        assert!(parse_search_all(&serde_json::json!({})).is_empty());

        let payload = tabbed_sections(vec![serde_json::json!({
            "musicShelfRenderer": {
                "title": { "runs": [{ "text": "Podcasts" }] },
                "contents": [{ "musicTwoRowItemRenderer": { "title": { "runs": [{ "text": "Item" }] } } }]
            }
        })]);
        assert!(parse_search_all(&payload).is_empty());
    }

    #[test]
    fn parse_search_tracks_live_shape_and_determinism() {
        let payload = fixture_json("catalog_live_shape.json");
        let first = parse_search_tracks(&payload);
        assert_eq!(first, parse_search_tracks(&payload), "parsing must be deterministic");

        let ids: Vec<&str> = first.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(ids, vec!["live-bare-1", "live-shelf-1"]);
        assert_eq!(
            (first[0].title.as_str(), first[0].artist.as_str()),
            ("Live Track Bare", "Live Artist Bare")
        );
        assert_eq!(first[0].thumb_url, None);
        assert_eq!(
            (first[1].title.as_str(), first[1].artist.as_str()),
            ("Live Track Shelved", "Live Artist Shelved")
        );
    }

    #[test]
    fn parse_search_all_live_shape_and_determinism() {
        let payload = fixture_json("catalog_live_shape.json");
        let first = parse_search_all(&payload);
        assert_eq!(first, parse_search_all(&payload), "parsing must be deterministic");

        let track_ids: Vec<&str> = first.tracks.iter().map(|t| t.video_id.as_str()).collect();
        assert_eq!(track_ids, vec!["live-bare-1", "live-shelf-1"]);
        assert!(first.albums.is_empty());
        assert!(first.artists.is_empty());
        assert!(first.playlists.is_empty());
    }

    #[test]
    fn parse_search_all_badges_golden_fixture() {
        let payload = fixture_json("catalog_search_badges.json");
        let first = parse_search_all(&payload);
        let second = parse_search_all(&payload);
        assert_eq!(first, second, "parsing must be deterministic");

        assert_eq!(first.tracks.len(), 3);
        assert_eq!(first.albums.len(), 1);
        assert_eq!(first.artists.len(), 1);
        assert_eq!(first.playlists.len(), 1);

        // Song item with multi-artist runs
        assert_eq!(
            (
                first.tracks[0].video_id.as_str(),
                first.tracks[0].title.as_str(),
                first.tracks[0].artist.as_str()
            ),
            ("song-vid-1", "Bachata Rosa", "Juan Luis Guerra & 4.40")
        );
        assert_eq!(
            first.tracks[0].thumb_url.as_deref(),
            Some("https://lh3.googleusercontent.com/test-thumb-large")
        );

        // Video item
        assert_eq!(
            (
                first.tracks[1].video_id.as_str(),
                first.tracks[1].title.as_str(),
                first.tracks[1].artist.as_str()
            ),
            ("video-vid-2", "Propuesta Indecente (Official Video)", "Romeo Santos")
        );
        assert_eq!(
            first.tracks[1].thumb_url.as_deref(),
            Some("https://lh3.googleusercontent.com/video-thumb-large")
        );
        assert_eq!(first.tracks[1].video_type, Some(crate::domain::MusicVideoType::Omv));

        // Episode item
        assert_eq!(
            (
                first.tracks[2].video_id.as_str(),
                first.tracks[2].title.as_str(),
                first.tracks[2].artist.as_str()
            ),
            ("ep-vid-6", "Episode 42: Latin Music History", "Latin Beats Podcast")
        );
        assert_eq!(
            first.tracks[2].thumb_url.as_deref(),
            Some("https://lh3.googleusercontent.com/episode-thumb")
        );

        // Single item classified as RawAlbum with browseId routed
        assert_eq!(
            (
                first.albums[0].browse_id.as_str(),
                first.albums[0].title.as_str(),
                first.albums[0].artist.as_str()
            ),
            ("MPREb_single_1", "Eres Mia", "Romeo Santos \u{2022} 2014")
        );
        assert_eq!(
            first.albums[0].thumb_url.as_deref(),
            Some("https://lh3.googleusercontent.com/single-thumb")
        );

        // Artist item with browseId routed
        assert_eq!(
            (first.artists[0].browse_id.as_str(), first.artists[0].name.as_str()),
            ("UC_romeo_1", "Romeo Santos")
        );
        assert_eq!(
            first.artists[0].thumb_url.as_deref(),
            Some("https://lh3.googleusercontent.com/artist-thumb")
        );

        // Playlist item with browseId routed
        assert_eq!(
            (first.playlists[0].browse_id.as_str(), first.playlists[0].title.as_str()),
            ("VL_playlist_1", "Bachata Essentials")
        );
        assert_eq!(
            first.playlists[0].thumb_url.as_deref(),
            Some("https://lh3.googleusercontent.com/playlist-thumb")
        );
    }

    #[test]
    fn parse_search_all_badges_skips_profile_and_podcast() {
        let profile = test_responsive_item(
            "Some User",
            &["Profile", " \u{2022} ", "100 subscribers"],
            serde_json::json!({ "browseEndpoint": { "browseId": "UC_user_1", "pageType": "MUSIC_PAGE_TYPE_USER_CHANNEL" } }),
        );
        let podcast = test_responsive_item(
            "Tech Weekly",
            &["Podcast", " \u{2022} ", "Host"],
            serde_json::json!({ "browseEndpoint": { "browseId": "MPSP_pod_1", "pageType": "MUSIC_PAGE_TYPE_PODCAST_SHOW" } }),
        );
        let unknown = serde_json::json!({
            "musicResponsiveListItemRenderer": {
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "No ID Unknown" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "MysteryKind" }] } } }
                ]
            }
        });
        let payload = tabbed_sections(vec![serde_json::json!({
            "itemSectionRenderer": { "contents": [profile, podcast, unknown] }
        })]);

        let hits = parse_search_all(&payload);
        assert!(
            hits.is_empty(),
            "profile, podcast, and unknown without playable ID must be skipped"
        );
    }

    #[test]
    fn parse_search_all_badges_empty_artist_and_defaults() {
        let solo = serde_json::json!({
            "musicResponsiveListItemRenderer": {
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Solo Track" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Song" }] } } }
                ],
                "playlistItemData": { "videoId": "solo-vid-1" }
            }
        });
        let no_badge = serde_json::json!({
            "musicResponsiveListItemRenderer": {
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "No Badge Track" }] } } }
                ],
                "playlistItemData": { "videoId": "no-badge-vid-2" }
            }
        });
        let payload = tabbed_sections(vec![serde_json::json!({
            "itemSectionRenderer": { "contents": [solo, no_badge] }
        })]);

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
    fn is_video_badge_classification() {
        for badge in [
            "video",
            "vídeo",
            "music video",
            "video musical",
            "videoclip",
            "Video",
            "Vídeo",
            "VÍDEO",
            "Music Video",
            "Video Musical",
            "Videoclip",
        ] {
            assert!(is_video_badge(badge), "expected video badge for {badge}");
        }
        for badge in ["song", "Song", "album", "artist", "playlist", "podcast", ""] {
            assert!(!is_video_badge(badge), "expected non-video badge for {badge}");
        }
    }

    #[test]
    fn parse_search_detects_video_badge_and_video_shelf() {
        assert!(is_video_shelf("Videos"));
        assert!(is_video_shelf("Vídeos"));
        assert!(!is_video_shelf("Songs"));
        let shelf = serde_json::json!({
            "musicShelfRenderer": {
                "title": { "runs": [{ "text": "Videos" }] },
                "contents": [test_responsive_item(
                    "Shelf Video",
                    &["Artist"],
                    serde_json::json!({ "watchEndpoint": { "videoId": "v-shelf-1" } }),
                )]
            }
        });
        let bare = serde_json::json!({
            "itemSectionRenderer": {
                "contents": [test_responsive_item(
                    "Badge Video",
                    &["Music video", " \u{2022} ", "Artist"],
                    serde_json::json!({ "watchEndpoint": { "videoId": "v-badge-1" } }),
                )]
            }
        });
        let payload = tabbed_sections(vec![shelf, bare]);
        let hits = parse_search_all(&payload);
        assert_eq!(hits.tracks.len(), 2);
        assert_eq!(hits.tracks[0].video_id, "v-shelf-1");
        assert_eq!(hits.tracks[0].video_type, Some(crate::domain::MusicVideoType::Omv));
        assert_eq!(hits.tracks[1].video_id, "v-badge-1");
        assert_eq!(hits.tracks[1].video_type, Some(crate::domain::MusicVideoType::Omv));

        let shelf_tracks = parse_search_tracks(&payload);
        assert_eq!(shelf_tracks.len(), 2);
        assert_eq!(shelf_tracks[0].video_type, Some(crate::domain::MusicVideoType::Omv));
        assert_eq!(shelf_tracks[1].video_type, Some(crate::domain::MusicVideoType::Omv));
    }
}
