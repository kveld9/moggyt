//! Search response parser for tracks and heterogeneous shelves.

use super::catalog_model::{RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits};
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

/// Item-type labels the catalog prepends to the metadata column.
const TYPE_LABELS: &[&str] = &[
    "album",
    "artist",
    "artista",
    "cancion",
    "canci\u{f3}n",
    "episode",
    "episodio",
    "lista de reproduccion",
    "lista de reproducci\u{f3}n",
    "perfil",
    "playlist",
    "podcast",
    "p\u{f3}dcast",
    "profile",
    "song",
    "video",
    "v\u{ed}deo",
];

/// Resolves a track row artist from the metadata column.
///
/// The column is prefixed with the item type and suffixed with the duration, so both
/// edges are removed while the separators inside the artist list stay untouched.
fn track_artist(renderer: &Value) -> String {
    let Some(raw) = flex_text(renderer, 1) else { return String::new() };
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

fn is_type_label(raw: &str) -> bool {
    let lowered = raw.trim().to_lowercase();
    TYPE_LABELS.contains(&lowered.as_str())
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
    hits.tracks.push(RawTrack {
        video_id,
        title,
        artist,
        duration_secs: search_duration(renderer),
        thumb_url,
        video_type,
    });
}

fn dispatch_category(
    cat: SearchFilter,
    renderer: &Value,
    title: String,
    artist: String,
    hits: &mut SearchHits,
) {
    let Some(browse_id) = browse_of(renderer) else {
        dispatch_bare_track(renderer, title, artist, hits);
        return;
    };
    let thumb_url = best_thumbnail(renderer);
    match cat {
        SearchFilter::Albums => hits.albums.push(RawAlbum { browse_id, title, artist, thumb_url }),
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
mod tests {
    use super::*;

    fn fixture_json(name: &str) -> Value {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture readable"))
            .expect("valid fixture")
    }

    fn check_track(t: &RawTrack, id: &str, title: &str, artist: &str) {
        assert_eq!((t.video_id.as_str(), &t.title[..], &t.artist[..]), (id, title, artist));
    }

    fn check_album(a: &RawAlbum, id: &str, title: &str, artist: &str) {
        assert_eq!((a.browse_id.as_str(), &a.title[..], &a.artist[..]), (id, title, artist));
    }

    fn check_artist(a: &RawArtist, id: &str, name: &str) {
        assert_eq!((a.browse_id.as_str(), a.name.as_str()), (id, name));
    }

    fn check_playlist(p: &RawPlaylist, id: &str, title: &str) {
        assert_eq!((p.browse_id.as_str(), p.title.as_str()), (id, title));
    }

    fn check_lh3(url: Option<&str>, path: &str) {
        assert_eq!(url, Some(format!("https://lh3.googleusercontent.com/{path}").as_str()));
    }

    #[test]
    fn parse_search_tracks_tolerates_empty_and_malformed() {
        assert!(parse_search_tracks(&serde_json::json!({})).is_empty());
        assert!(parse_search_tracks(&serde_json::json!({"contents": null})).is_empty());
        let tabs = serde_json::json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": []}}});
        assert!(parse_search_tracks(&tabs).is_empty());
    }

    #[test]
    fn parse_search_tracks_golden_fixture() {
        let tracks = parse_search_tracks(&fixture_json("catalog_search.json"));
        assert_eq!(tracks.len(), 2);
        check_track(&tracks[0], "vid-1", "Midnight City", "M83");
        check_lh3(tracks[0].thumb_url.as_deref(), "test-thumb-large");
        check_track(&tracks[1], "vid-2", "Starboy", "The Weeknd");
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
    fn test_parse_search_tracks_resolves_artist_past_type_and_duration() {
        let (resp, w) = (test_responsive_item, test_watch);
        let items = vec![
            resp("Bohemian Rhapsody", &["Cancion \u{2022} Queen"], w("vid-type-artist")),
            resp("Tear It Up", &["Cancion \u{2022} 3:25"], w("vid-type-only")),
            resp("Loser", &["Tame Impala \u{2022} Deadbeat \u{2022} 3:44"], w("vid-artist-first")),
            resp("Bohemian Rhapsody (Live)", &["Canci\u{f3}n \u{2022} Queen"], w("vid-accented")),
        ];
        let shelf = serde_json::json!({"musicShelfRenderer": {"title": {"runs": [{"text": "Canciones"}]}, "contents": items}});
        let tracks = parse_search_tracks(&tabbed_sections(vec![shelf]));
        assert_eq!(tracks.len(), 4);
        assert_eq!(tracks[0].artist, "Queen");
        assert_eq!(tracks[1].artist, "", "a row with only a type and a duration has no artist");
        assert_eq!(
            tracks[2].artist, "Tame Impala \u{2022} Deadbeat",
            "artist-first rows keep their artist"
        );
        assert_eq!(tracks[3].artist, "Queen", "accented Spanish type labels are stripped too");
    }

    #[test]
    fn search_duration_extracts_from_fixed_length_and_flex_columns() {
        let fixed_r = |t| serde_json::json!({"fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"runs": [{"text": t}]}}}]});
        let fixed_s = |t| serde_json::json!({"fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"simpleText": t}}}]});
        let len_r = |t| serde_json::json!({"lengthText": {"runs": [{"text": t}]}});
        let len_s = |t| serde_json::json!({"lengthText": {"simpleText": t}});
        let flex_r = serde_json::json!({"flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Artist"}, {"text": "3:44"}]}}}]});
        let cases = [
            (fixed_r("3:45"), Some(225)),
            (fixed_s("1:20"), Some(80)),
            (len_r("2:30"), Some(150)),
            (len_s("4:00"), Some(240)),
            (flex_r, Some(224)),
            (serde_json::json!({}), None),
        ];
        for (renderer, expected) in cases {
            assert_eq!(search_duration(&renderer), expected);
        }
    }

    fn tabbed_sections(sections: Vec<Value>) -> Value {
        serde_json::json!({"contents": {"tabbedSearchResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": sections}}}}]}}})
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
        let item = serde_json::json!({"musicResponsiveListItemRenderer": {
            "navigationEndpoint": { "watchEndpoint": { "videoId": "v1" } }
        }});
        let payload = tabbed_sections(vec![serde_json::json!({
            "musicShelfRenderer": { "contents": [item] }
        })]);
        assert!(parse_search_tracks(&payload).is_empty());
    }

    #[test]
    fn shelf_category_routing_table() {
        for title in ["Songs", "Top result", "Top songs", "Videos", "V\u{ed}deos", "Music videos"] {
            assert_eq!(shelf_category(title), Some(SearchFilter::Tracks));
        }
        assert_eq!(shelf_category("Albums"), Some(SearchFilter::Albums));
        assert_eq!(shelf_category("Artists"), Some(SearchFilter::Artists));
        for title in ["Playlists", "Community mixes", "Featured shows"] {
            assert_eq!(shelf_category(title), Some(SearchFilter::Playlists));
        }
        assert_eq!(shelf_category("Podcasts"), None);
        assert_eq!(shelf_category(""), None);
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
        check_track(&first.tracks[0], "solar-1", "Solaris", "Photay");
        assert_eq!(first.tracks[0].thumb_url, None);
        check_album(&first.albums[0], "MPREb_album_1", "Waking Season", "Photay");
        check_artist(&first.artists[0], "UC_photay_1", "Photay");
    }

    #[test]
    fn parse_search_filtered_albums_golden_fixture() {
        let hits = parse_search_all(&fixture_json("catalog_search_filtered_albums.json"));
        assert_eq!(hits.albums.len(), 1);
        check_album(
            &hits.albums[0],
            "MPREb_test_album_1",
            "Test Album Title",
            "Test Artist \u{2022} 2024",
        );
        check_lh3(hits.albums[0].thumb_url.as_deref(), "test-album-thumb");
    }

    fn test_endpoint(id: &str, page_type: &str) -> Value {
        serde_json::json!({ "browseEndpoint": { "browseId": id, "pageType": page_type } })
    }

    fn test_browse_item(title: &str, badge: &str, id: &str, pt: &str) -> Value {
        test_responsive_item(title, &[badge], test_endpoint(id, pt))
    }

    #[test]
    fn parse_search_spanish_badges_and_page_types() {
        let browse = test_browse_item;
        let artist =
            browse("Artista Destacado", "Artista", "UC_es_artist", "MUSIC_PAGE_TYPE_ARTIST");
        let playlist = browse(
            "Exitos Espanol",
            "Lista de reproduccion",
            "VL_es_playlist",
            "MUSIC_PAGE_TYPE_PLAYLIST",
        );
        let single =
            browse("Sencillo Nuevo", "Sencillo", "MPREb_es_single", "MUSIC_PAGE_TYPE_ALBUM");
        let profile =
            browse("Perfil Usuario", "Perfil", "UC_profile", "MUSIC_PAGE_TYPE_USER_CHANNEL");
        let payload = tabbed_sections(vec![serde_json::json!({
            "itemSectionRenderer": { "contents": [artist, playlist, single, profile] }
        })]);
        let hits = parse_search_all(&payload);
        assert_eq!(hits.artists[0].browse_id, "UC_es_artist");
        assert_eq!(hits.playlists[0].browse_id, "VL_es_playlist");
        assert_eq!(hits.albums[0].browse_id, "MPREb_es_single");
    }

    #[test]
    fn parse_search_all_ignores_unknown_shelves_and_tolerates_empty() {
        assert!(parse_search_all(&serde_json::json!({})).is_empty());
        let item =
            serde_json::json!({"musicTwoRowItemRenderer": {"title": {"runs": [{"text": "Item"}]}}});
        let shelf = serde_json::json!({"musicShelfRenderer": {
            "title": {"runs": [{"text": "Podcasts"}]}, "contents": [item]
        }});
        assert!(parse_search_all(&tabbed_sections(vec![shelf])).is_empty());
    }

    #[test]
    fn parse_search_live_shape_and_determinism() {
        let payload = fixture_json("catalog_live_shape.json");
        let tracks = parse_search_tracks(&payload);
        assert_eq!(tracks, parse_search_tracks(&payload));
        assert_eq!(tracks.len(), 2);
        check_track(&tracks[0], "live-bare-1", "Live Track Bare", "Live Artist Bare");
        check_track(&tracks[1], "live-shelf-1", "Live Track Shelved", "Live Artist Shelved");

        let all = parse_search_all(&payload);
        assert_eq!(all, parse_search_all(&payload));
        assert_eq!(all.tracks.len(), 2);
        assert!(all.albums.is_empty() && all.artists.is_empty() && all.playlists.is_empty());
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
        check_track(&first.tracks[0], "song-vid-1", "Bachata Rosa", "Juan Luis Guerra & 4.40");
        check_lh3(first.tracks[0].thumb_url.as_deref(), "test-thumb-large");

        // Video item
        check_track(
            &first.tracks[1],
            "video-vid-2",
            "Propuesta Indecente (Official Video)",
            "Romeo Santos",
        );
        check_lh3(first.tracks[1].thumb_url.as_deref(), "video-thumb-large");
        assert_eq!(first.tracks[1].video_type, Some(crate::domain::MusicVideoType::Omv));

        // Episode item
        check_track(
            &first.tracks[2],
            "ep-vid-6",
            "Episode 42: Latin Music History",
            "Latin Beats Podcast",
        );
        check_lh3(first.tracks[2].thumb_url.as_deref(), "episode-thumb");

        // Single item classified as RawAlbum with browseId routed
        check_album(&first.albums[0], "MPREb_single_1", "Eres Mia", "Romeo Santos \u{2022} 2014");
        check_lh3(first.albums[0].thumb_url.as_deref(), "single-thumb");

        // Artist item with browseId routed
        check_artist(&first.artists[0], "UC_romeo_1", "Romeo Santos");
        check_lh3(first.artists[0].thumb_url.as_deref(), "artist-thumb");

        // Playlist item with browseId routed
        check_playlist(&first.playlists[0], "VL_playlist_1", "Bachata Essentials");
        check_lh3(first.playlists[0].thumb_url.as_deref(), "playlist-thumb");
    }

    fn test_track_item(title: &str, vid: &str, badge: Option<&str>) -> Value {
        let run_col = |t| serde_json::json!({"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": t}]}}});
        let mut cols = vec![run_col(title)];
        if let Some(b) = badge {
            cols.push(run_col(b));
        }
        serde_json::json!({"musicResponsiveListItemRenderer": {"flexColumns": cols, "playlistItemData": {"videoId": vid}}})
    }

    fn test_watch(vid: &str) -> Value {
        serde_json::json!({ "watchEndpoint": { "videoId": vid } })
    }

    #[test]
    fn parse_search_all_badges_skips_profile_and_podcast() {
        let (browse, resp) = (test_browse_item, test_responsive_item);
        let items = [
            browse("User", "Profile", "UC_1", "MUSIC_PAGE_TYPE_USER_CHANNEL"),
            browse("Pod", "Podcast", "MPSP_1", "MUSIC_PAGE_TYPE_PODCAST_SHOW"),
            resp("Unknown", &["MysteryKind"], Value::Null),
        ];
        let payload =
            tabbed_sections(vec![serde_json::json!({"itemSectionRenderer": {"contents": items}})]);
        assert!(parse_search_all(&payload).is_empty(), "profile and podcast must be skipped");
    }

    #[test]
    fn parse_search_all_badges_empty_artist_and_defaults() {
        let items = [
            test_track_item("Solo Track", "solo-vid-1", Some("Song")),
            test_track_item("No Badge Track", "no-badge-vid-2", None),
        ];
        let payload =
            tabbed_sections(vec![serde_json::json!({"itemSectionRenderer": {"contents": items}})]);
        let hits = parse_search_all(&payload);
        assert_eq!(hits.tracks.len(), 2);
        check_track(&hits.tracks[0], "solo-vid-1", "Solo Track", "");
        check_track(&hits.tracks[1], "no-badge-vid-2", "No Badge Track", "");
    }

    #[test]
    fn is_video_badge_classification() {
        for badge in VIDEO_BADGES {
            assert!(is_video_badge(badge), "expected video badge for {badge}");
        }
        for badge in ["Video", "Vídeo", "VÍDEO", "Music Video", "Videoclip"] {
            assert!(is_video_badge(badge), "expected video badge for {badge}");
        }
        for badge in ["song", "Song", "album", "artist", "playlist", "podcast", ""] {
            assert!(!is_video_badge(badge), "expected non-video badge for {badge}");
        }
    }

    #[test]
    fn parse_search_detects_video_badge_and_video_shelf() {
        assert!(is_video_shelf("Videos") && is_video_shelf("Vídeos") && !is_video_shelf("Songs"));
        let s_item = test_responsive_item("Shelf Video", &["Artist"], test_watch("v-shelf-1"));
        let b_item = test_responsive_item("Badge Video", &["Music video"], test_watch("v-badge-1"));
        let shelf = serde_json::json!({"musicShelfRenderer": {"title": {"runs": [{"text": "Videos"}]}, "contents": [s_item]}});
        let bare = serde_json::json!({"itemSectionRenderer": {"contents": [b_item]}});
        let hits = parse_search_all(&tabbed_sections(vec![shelf, bare]));
        assert_eq!(hits.tracks.len(), 2);
        assert_eq!(
            (hits.tracks[0].video_id.as_str(), hits.tracks[0].video_type),
            ("v-shelf-1", Some(crate::domain::MusicVideoType::Omv))
        );
        assert_eq!(
            (hits.tracks[1].video_id.as_str(), hits.tracks[1].video_type),
            ("v-badge-1", Some(crate::domain::MusicVideoType::Omv))
        );
    }
}
