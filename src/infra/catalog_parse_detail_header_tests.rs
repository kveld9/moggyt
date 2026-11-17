//! Detail header parsing tests for two-column browse payloads.

use super::*;

fn album_payload(with_strapline: bool) -> serde_json::Value {
    let mut header = serde_json::json!({
        "musicResponsiveHeaderRenderer": {
            "title": { "runs": [{ "text": "Session cali cali" }] },
            "subtitle": { "runs": [
                { "text": "Single" }, { "text": " \u{2022} " }, { "text": "2026" }
            ] },
            "secondSubtitle": { "runs": [
                { "text": "1 canci\u{00f3}n" }, { "text": " \u{2022} " },
                { "text": "11 minutos y 8 segundos" }
            ] }
        }
    });
    if with_strapline {
        header["musicResponsiveHeaderRenderer"]["straplineTextOne"] = serde_json::json!({
            "runs": [
                { "text": "La Piedra Urbana" },
                { "text": " y " },
                { "text": "La 24 7 del sur" }
            ]
        });
    }
    serde_json::json!({
        "contents": { "twoColumnBrowseResultsRenderer": {
            "tabs": [{ "tabRenderer": { "content": { "sectionListRenderer": {
                "contents": [header]
            } } } }],
            "secondaryContents": { "sectionListRenderer": { "contents": [{
                "musicShelfRenderer": { "contents": [{ "musicResponsiveListItemRenderer": {
                    "videoId": "6lBc0P6cJGs",
                    "flexColumns": [
                        { "musicResponsiveListItemFlexColumnRenderer": {
                            "text": { "runs": [{ "text": "Session cali cali" }] }
                        } },
                        { "musicResponsiveListItemFlexColumnRenderer": { "text": {} } }
                    ],
                    "fixedColumns": [{ "musicResponsiveListItemFixedColumnRenderer": {
                        "text": { "runs": [{ "text": "11:08" }] }
                    } }]
                } }] }
            }] } }
        } }
    })
}

#[test]
fn parse_detail_all_reads_the_two_column_playlist_header() {
    let payload = serde_json::json!({
        "contents": { "twoColumnBrowseResultsRenderer": { "tabs": [{ "tabRenderer": {
            "content": { "sectionListRenderer": { "contents": [{
                "musicEditablePlaylistDetailHeaderRenderer": {
                    "header": { "musicResponsiveHeaderRenderer": {
                        "thumbnail": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [
                            { "url": "https://img.local/pl.jpg", "width": 544, "height": 544 }
                        ] } } },
                        "title": { "runs": [{ "text": "Saved Playlist" }] },
                        "subtitle": { "runs": [
                            { "text": "Playlist" }, { "text": " \u{2022} " }, { "text": "Public" },
                            { "text": " \u{2022} " }, { "text": "2026" }
                        ] },
                        "secondSubtitle": { "runs": [
                            { "text": "66 songs" }, { "text": " \u{2022} " },
                            { "text": "1 save" }, { "text": " \u{2022} " },
                            { "text": "3 hours, 43 minutes" }
                        ] },
                        "facepile": { "avatarStackViewModel": { "text": { "content": "Owner" } } }
                    } },
                    "editHeader": { "musicPlaylistEditHeaderRenderer": { "privacy": "PUBLIC" } }
                }
            }] } }
        } }] } }
    });
    let detail = parse_detail_all(&payload, DetailKind::Playlist).expect("detail");
    assert_eq!(detail.title, "Saved Playlist");
    assert_eq!(detail.subtitle, "Owner");
    assert_eq!(detail.track_count, Some(66));
    assert_eq!(detail.total_duration_secs, Some(13_380));
    assert_eq!(detail.privacy, Some(PlaylistPrivacy::Public));
    assert_eq!(detail.saves.as_deref(), Some("1 save"));
    assert_eq!(detail.artwork.as_deref(), Some("https://img.local/pl.jpg"));
}

#[test]
fn parse_detail_all_reads_two_column_album_header_and_backfills_track_artists() {
    let payload = album_payload(true);
    let detail = parse_detail_all(&payload, DetailKind::Album).expect("parsed detail");
    assert_eq!(detail.kind, DetailKind::Album);
    assert_eq!(detail.subtitle, "La Piedra Urbana y La 24 7 del sur");
    assert_eq!(detail.container_label.as_deref(), Some("Single \u{2022} 2026"));
    assert_eq!(detail.tracks.len(), 1);
    assert_eq!(detail.tracks[0].artist, "La Piedra Urbana y La 24 7 del sur");
    assert_eq!(detail.tracks[0].duration_secs, Some(668));
}

#[test]
fn parse_detail_all_album_without_strapline_keeps_old_subtitle_behavior() {
    let payload = album_payload(false);
    let detail = parse_detail_all(&payload, DetailKind::Album).expect("parsed detail");
    assert_eq!(detail.kind, DetailKind::Album);
    assert_eq!(detail.subtitle, "2026");
    assert_eq!(detail.container_label, None);
}
