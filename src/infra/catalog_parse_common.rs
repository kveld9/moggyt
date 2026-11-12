//! Shared parsing primitives and renderer extractors for catalog payloads.

use crate::domain::{Album, Artist, MusicVideoType, Playlist, SearchResult};
use crate::infra::spotify_search::video_type_from;
use serde_json::Value;

/// Extracts the music video classification from renderer endpoints if present.
pub fn music_video_type(renderer: &Value) -> Option<MusicVideoType> {
    const PATHS: &[&str] = &[
        "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType",
        "/navigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType",
    ];
    let raw = PATHS.iter().find_map(|p| renderer.pointer(p).and_then(Value::as_str))?;
    Some(video_type_from(Some(raw)))
}

/// Extracts a playable YouTube video identifier from renderer endpoints.
pub fn watch_video_id(item: &Value) -> Option<String> {
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

/// Extracts text from a flex column run.
pub fn flex_text(item: &Value, col_idx: usize) -> Option<String> {
    let col = item.get("flexColumns")?.get(col_idx)?;
    let text = col
        .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")?
        .as_str()?
        .trim();
    if text.is_empty() { None } else { Some(text.to_owned()) }
}

/// Locates thumbnail candidate arrays across known renderer layouts.
pub fn thumbnail_candidates(value: &Value) -> Option<&[Value]> {
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

/// Extracts title text from a two-row item renderer.
pub fn two_row_title(renderer: &Value) -> Option<String> {
    let title = renderer.pointer("/title/runs/0/text")?.as_str()?.trim();
    if title.is_empty() { None } else { Some(title.to_owned()) }
}

/// Extracts subtitle text from a two-row item renderer.
pub fn subtitle_text(renderer: &Value) -> String {
    renderer
        .pointer("/subtitle/runs/0/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_owned()
}

/// Extracts a browse identifier from navigation endpoints.
pub fn browse_of(renderer: &Value) -> Option<String> {
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

/// Alias for [`browse_of`].
pub fn browse_id(renderer: &Value) -> Option<String> {
    browse_of(renderer)
}

/// Extracts a browse pageType string from navigation endpoints.
pub fn browse_page_type(renderer: &Value) -> Option<&str> {
    const PATHS: &[&str] = &[
        "/navigationEndpoint/browseEndpoint/pageType",
        "/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType",
        "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/browseEndpoint/pageType",
        "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType",
        "/title/runs/0/navigationEndpoint/browseEndpoint/pageType",
        "/title/runs/0/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType",
    ];
    for path in PATHS {
        if let Some(pt) = renderer.pointer(path).and_then(Value::as_str) {
            let trimmed = pt.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
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

/// Browse id of the artist credited by a catalog item's metadata runs.
///
/// Release and album rows place the item type first ("Single - Toosii",
/// "Album - Bad Bunny - 2025"), so the credited artist is the run whose browse target is an
/// artist page rather than the first run.
pub fn item_artist_id(renderer: &Value) -> Option<String> {
    const RUN_PATHS: [&str; 2] =
        ["/subtitle/runs", "/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs"];
    RUN_PATHS
        .iter()
        .find_map(|path| renderer.pointer(path)?.as_array()?.iter().find_map(artist_run_id))
}

fn artist_run_id(run: &Value) -> Option<String> {
    if browse_page_type(run) != Some("MUSIC_PAGE_TYPE_ARTIST") {
        return None;
    }
    let id = run.pointer("/navigationEndpoint/browseEndpoint/browseId")?.as_str()?.trim();
    if id.is_empty() { None } else { Some(id.to_owned()) }
}

fn parse_mm_ss(mins: &str, secs: &str) -> Option<u32> {
    mins.parse::<u32>().ok()?.checked_mul(60)?.checked_add(secs.parse().ok()?)
}

fn parse_hh_mm_ss(hours: &str, mins: &str, secs: &str) -> Option<u32> {
    let total_m = hours.parse::<u32>().ok()?.checked_mul(60)?.checked_add(mins.parse().ok()?)?;
    total_m.checked_mul(60)?.checked_add(secs.parse().ok()?)
}

/// Parses duration string formatted as "mm:ss" or "hh:mm:ss" into total seconds.
pub fn parse_duration_secs(raw: &str) -> Option<u32> {
    let parts: Vec<&str> = raw.trim().split(':').collect();
    match parts.as_slice() {
        [mins, secs] => parse_mm_ss(mins, secs),
        [hours, mins, secs] => parse_hh_mm_ss(hours, mins, secs),
        _ => None,
    }
}

/// Finds the first duration string in a slice of text runs.
pub fn parse_runs_duration(runs: &[Value]) -> Option<u32> {
    runs.iter()
        .find_map(|run| run.get("text").and_then(Value::as_str).and_then(parse_duration_secs))
}

/// Extracts duration in seconds from a two-row item renderer (e.g. subtitle runs or lengthText).
pub fn extract_two_row_duration(renderer: &Value) -> Option<u32> {
    renderer
        .pointer("/subtitle/runs")
        .and_then(Value::as_array)
        .and_then(|runs| parse_runs_duration(runs))
        .or_else(|| {
            renderer
                .pointer("/lengthText/runs")
                .and_then(Value::as_array)
                .and_then(|runs| parse_runs_duration(runs))
        })
        .or_else(|| {
            renderer
                .pointer("/lengthText/simpleText")
                .and_then(Value::as_str)
                .and_then(parse_duration_secs)
        })
        .or_else(|| {
            renderer
                .pointer("/subtitle/simpleText")
                .and_then(Value::as_str)
                .and_then(parse_duration_secs)
        })
}

/// Extracts panel artist text from byline text runs.
pub fn panel_artist(renderer: &Value) -> String {
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

/// Classifies a browse entity into an artist, playlist, or album search result.
pub fn classify_home_item(
    id: &str,
    title: &str,
    subtitle: &str,
    artwork: Option<String>,
    artist_id: Option<String>,
) -> SearchResult {
    if id.starts_with("UC") {
        return SearchResult::Artist(Artist::new(id, title, artwork));
    }
    if id.starts_with("VL") || id.starts_with("RD") || id.starts_with("FEmusic_") {
        return SearchResult::Playlist(Playlist::new(id, title, 0, artwork));
    }
    SearchResult::Album(Album::new(id, title, subtitle, artwork).with_artist_id(artist_id))
}

/// Parses track count integer from subtitle text strings.
pub fn parse_track_count(text: &str) -> usize {
    text.split_whitespace().find_map(|w| w.parse::<usize>().ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse_page_type_extraction() {
        let direct = serde_json::json!({
            "navigationEndpoint": {
                "browseEndpoint": {
                    "browseId": "MPREb_1",
                    "pageType": "MUSIC_PAGE_TYPE_ALBUM"
                }
            }
        });
        assert_eq!(browse_page_type(&direct), Some("MUSIC_PAGE_TYPE_ALBUM"));

        let nested = serde_json::json!({
            "navigationEndpoint": {
                "browseEndpoint": {
                    "browseEndpointContextSupportedConfigs": {
                        "browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                        }
                    }
                }
            }
        });
        assert_eq!(browse_page_type(&nested), Some("MUSIC_PAGE_TYPE_ARTIST"));

        let col = serde_json::json!({
            "flexColumns": [
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [
                                {
                                    "text": "Playlist",
                                    "navigationEndpoint": {
                                        "browseEndpoint": {
                                            "pageType": "MUSIC_PAGE_TYPE_PLAYLIST"
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            ]
        });
        assert_eq!(browse_page_type(&col), Some("MUSIC_PAGE_TYPE_PLAYLIST"));

        let empty = serde_json::json!({});
        assert_eq!(browse_page_type(&empty), None);
    }

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
    fn parse_runs_duration_and_extract_two_row_duration_variants() {
        let runs = vec![
            serde_json::json!({ "text": "Artist Name" }),
            serde_json::json!({ "text": " • " }),
            serde_json::json!({ "text": "3:45" }),
        ];
        assert_eq!(parse_runs_duration(&runs), Some(225));
        assert_eq!(parse_runs_duration(&[]), None);
        assert_eq!(parse_runs_duration(&[serde_json::json!({ "text": "no duration here" })]), None);

        let renderer_subtitle = serde_json::json!({
            "subtitle": {
                "runs": [
                    { "text": "Band" },
                    { "text": "4:20" }
                ]
            }
        });
        assert_eq!(extract_two_row_duration(&renderer_subtitle), Some(260));

        let renderer_length = serde_json::json!({
            "lengthText": {
                "runs": [{ "text": "1:15" }]
            }
        });
        assert_eq!(extract_two_row_duration(&renderer_length), Some(75));

        let renderer_simple = serde_json::json!({
            "lengthText": {
                "simpleText": "2:30"
            }
        });
        assert_eq!(extract_two_row_duration(&renderer_simple), Some(150));

        assert_eq!(extract_two_row_duration(&serde_json::json!({})), None);
    }

    #[test]
    fn test_parse_mm_ss_valid_overflow_non_numeric() {
        assert_eq!(parse_mm_ss("3", "30"), Some(210));
        assert_eq!(parse_mm_ss("0", "0"), Some(0));
        assert_eq!(parse_mm_ss("71582789", "0"), None);
        assert_eq!(parse_mm_ss("abc", "30"), None);
        assert_eq!(parse_mm_ss("3", "xyz"), None);
    }

    #[test]
    fn test_parse_hh_mm_ss_valid_overflow_non_numeric() {
        assert_eq!(parse_hh_mm_ss("1", "02", "15"), Some(3735));
        assert_eq!(parse_hh_mm_ss("0", "0", "0"), Some(0));
        assert_eq!(parse_hh_mm_ss("71582789", "0", "0"), None);
        assert_eq!(parse_hh_mm_ss("foo", "02", "15"), None);
        assert_eq!(parse_hh_mm_ss("1", "bar", "15"), None);
        assert_eq!(parse_hh_mm_ss("1", "02", "baz"), None);
    }

    #[test]
    fn classify_home_item_routing() {
        let artist = classify_home_item("UC_art_1", "ArtTitle", "ArtSub", None, None);
        assert_eq!(artist, SearchResult::Artist(Artist::new("UC_art_1", "ArtTitle", None)));

        let playlist_vl = classify_home_item("VL_pl_1", "PLTitle", "PLSub", None, None);
        assert_eq!(
            playlist_vl,
            SearchResult::Playlist(Playlist::new("VL_pl_1", "PLTitle", 0, None))
        );

        let playlist_rd = classify_home_item("RD_radio_1", "RadioTitle", "RadioSub", None, None);
        assert_eq!(
            playlist_rd,
            SearchResult::Playlist(Playlist::new("RD_radio_1", "RadioTitle", 0, None))
        );

        let playlist_fe = classify_home_item("FEmusic_mix", "MixTitle", "MixSub", None, None);
        assert_eq!(
            playlist_fe,
            SearchResult::Playlist(Playlist::new("FEmusic_mix", "MixTitle", 0, None))
        );

        let album = classify_home_item(
            "MPREb_alb_1",
            "AlbTitle",
            "AlbSub",
            None,
            Some("UC_alb_artist".to_owned()),
        );
        assert_eq!(
            album,
            SearchResult::Album(
                Album::new("MPREb_alb_1", "AlbTitle", "AlbSub", None)
                    .with_artist_id(Some("UC_alb_artist".to_owned()))
            )
        );
        if let SearchResult::Album(album) = &album {
            assert_eq!(album.artist_id.as_deref(), Some("UC_alb_artist"));
        }
    }

    #[test]
    fn item_artist_id_picks_the_credited_artist_run() {
        let renderer = serde_json::json!({
            "subtitle": { "runs": [
                { "text": "Single" },
                { "text": " • " },
                { "text": "Toosii", "navigationEndpoint": { "browseEndpoint": {
                    "browseId": "UC_toosii",
                    "browseEndpointContextSupportedConfigs": {
                        "browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                        }
                    }
                } } }
            ] }
        });
        assert_eq!(item_artist_id(&renderer).as_deref(), Some("UC_toosii"));
    }

    #[test]
    fn item_artist_id_reads_responsive_album_rows() {
        let renderer = serde_json::json!({
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Un Verano Sin Ti" }
                ] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Album" },
                    { "text": " • " },
                    { "text": "Bad Bunny", "navigationEndpoint": { "browseEndpoint": {
                        "browseId": "UC_bunny",
                        "browseEndpointContextSupportedConfigs": {
                            "browseEndpointContextMusicConfig": {
                                "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                            }
                        }
                    } } },
                    { "text": " • " },
                    { "text": "2022" }
                ] } } }
            ]
        });
        assert_eq!(item_artist_id(&renderer).as_deref(), Some("UC_bunny"));
    }

    #[test]
    fn item_artist_id_is_absent_for_plain_subtitles() {
        let plain = serde_json::json!({ "subtitle": { "runs": [{ "text": "Album Artist 1" }] } });
        assert_eq!(item_artist_id(&plain), None);

        let empty_id = serde_json::json!({
            "subtitle": { "runs": [{ "text": "Single" }, { "text": " • " }, {
                "text": "Nobody",
                "navigationEndpoint": { "browseEndpoint": {
                    "browseId": "   ",
                    "browseEndpointContextSupportedConfigs": {
                        "browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                        }
                    }
                } }
            }] }
        });
        assert_eq!(item_artist_id(&empty_id), None);
    }

    #[test]
    fn parse_track_count_samples() {
        assert_eq!(parse_track_count("25 tracks"), 25);
        assert_eq!(parse_track_count("1 track"), 1);
        assert_eq!(parse_track_count("no tracks"), 0);
        assert_eq!(parse_track_count(""), 0);
    }

    #[test]
    fn music_video_type_extraction() {
        let atv_flex = serde_json::json!({
            "flexColumns": [{
                "musicResponsiveListItemFlexColumnRenderer": {
                    "text": {
                        "runs": [{
                            "text": "Song",
                            "navigationEndpoint": {
                                "watchEndpoint": {
                                    "watchEndpointMusicSupportedConfigs": {
                                        "watchEndpointMusicConfig": {
                                            "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"
                                        }
                                    }
                                }
                            }
                        }]
                    }
                }
            }]
        });
        assert_eq!(music_video_type(&atv_flex), Some(MusicVideoType::Atv));

        let omv_endpoint = serde_json::json!({
            "navigationEndpoint": {
                "watchEndpoint": {
                    "watchEndpointMusicSupportedConfigs": {
                        "watchEndpointMusicConfig": {
                            "musicVideoType": "MUSIC_VIDEO_TYPE_OMV"
                        }
                    }
                }
            }
        });
        assert_eq!(music_video_type(&omv_endpoint), Some(MusicVideoType::Omv));

        assert_eq!(music_video_type(&serde_json::json!({})), None);
    }
}
