//! Playlist detail header parsing tests for two-column browse payloads.

use super::*;

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
