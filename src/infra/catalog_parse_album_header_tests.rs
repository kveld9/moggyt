//! Album header parser tests for responsive headers and credit lines.

use super::*;
use serde_json::json;

#[test]
fn album_header_reads_artists_and_container_label_from_responsive_header() {
    let payload = json!({
        "contents": {
            "twoColumnBrowseResultsRenderer": {
                "tabs": [{
                    "tabRenderer": {
                        "content": {
                            "sectionListRenderer": {
                                "contents": [{
                                    "musicResponsiveHeaderRenderer": {
                                        "title": { "runs": [{ "text": "Session cali cali" }] },
                                        "subtitle": {
                                            "runs": [
                                                { "text": "Single" },
                                                { "text": " \u{2022} " },
                                                { "text": "2026" }
                                            ]
                                        },
                                        "secondSubtitle": {
                                            "runs": [
                                                { "text": "1 canci\u{00f3}n" },
                                                { "text": " \u{2022} " },
                                                { "text": "11 minutos y 8 segundos" }
                                            ]
                                        },
                                        "straplineTextOne": {
                                            "runs": [
                                                { "text": "La Piedra Urbana" },
                                                { "text": " y " },
                                                { "text": "La 24 7 del sur" }
                                            ]
                                        }
                                    }
                                }]
                            }
                        }
                    }
                }]
            }
        }
    });

    let header = album_header(&payload);
    assert_eq!(header.artists.as_deref(), Some("La Piedra Urbana y La 24 7 del sur"));
    assert_eq!(header.container_label.as_deref(), Some("Single \u{2022} 2026"));
    assert_eq!(header.track_count, Some(1));
    assert_eq!(header.total_duration_secs, Some(668));
}

#[test]
fn album_header_returns_empty_when_strapline_is_missing_or_blank() {
    let missing_strapline = json!({
        "header": {
            "musicResponsiveHeaderRenderer": {
                "title": { "runs": [{ "text": "Album Title" }] },
                "subtitle": { "runs": [{ "text": "Single \u{2022} 2026" }] }
            }
        }
    });
    assert_eq!(album_header(&missing_strapline), AlbumHeader::default());

    let blank_strapline = json!({
        "header": {
            "musicResponsiveHeaderRenderer": {
                "title": { "runs": [{ "text": "Album Title" }] },
                "subtitle": { "runs": [{ "text": "Single \u{2022} 2026" }] },
                "straplineTextOne": {
                    "runs": [{ "text": "   " }]
                }
            }
        }
    });
    assert_eq!(album_header(&blank_strapline), AlbumHeader::default());

    let empty_payload = json!({});
    assert_eq!(album_header(&empty_payload), AlbumHeader::default());
}

#[test]
fn album_header_joins_multi_run_credits_with_original_separators() {
    let payload = json!({
        "header": {
            "musicResponsiveHeaderRenderer": {
                "title": { "runs": [{ "text": "Collab Album" }] },
                "subtitle": { "runs": [{ "text": "Album \u{2022} 2025" }] },
                "straplineTextOne": {
                    "runs": [
                        { "text": "Artist A" },
                        { "text": ", " },
                        { "text": "Artist B" },
                        { "text": " & " },
                        { "text": "Artist C" }
                    ]
                }
            }
        }
    });

    let header = album_header(&payload);
    assert_eq!(header.artists.as_deref(), Some("Artist A, Artist B & Artist C"));
    assert_eq!(header.container_label.as_deref(), Some("Album \u{2022} 2025"));
}

#[test]
fn album_header_drops_container_label_when_matching_artists_or_blank() {
    let matching = json!({
        "header": {
            "musicResponsiveHeaderRenderer": {
                "straplineTextOne": { "runs": [{ "text": "Solo Artist" }] },
                "subtitle": { "runs": [{ "text": "Solo Artist" }] }
            }
        }
    });
    let header_matching = album_header(&matching);
    assert_eq!(header_matching.artists.as_deref(), Some("Solo Artist"));
    assert_eq!(header_matching.container_label, None);

    let blank_sub = json!({
        "header": {
            "musicResponsiveHeaderRenderer": {
                "straplineTextOne": { "runs": [{ "text": "Solo Artist" }] },
                "subtitle": { "runs": [{ "text": "   " }] }
            }
        }
    });
    let header_blank = album_header(&blank_sub);
    assert_eq!(header_blank.artists.as_deref(), Some("Solo Artist"));
    assert_eq!(header_blank.container_label, None);
}

#[test]
fn album_header_preserves_aggregates_when_strapline_is_missing() {
    let payload = json!({
        "header": {
            "musicResponsiveHeaderRenderer": {
                "title": { "runs": [{ "text": "Session cali cali" }] },
                "subtitle": {
                    "runs": [
                        { "text": "Single" },
                        { "text": " \u{2022} " },
                        { "text": "2026" }
                    ]
                },
                "secondSubtitle": {
                    "runs": [
                        { "text": "1 canci\u{00f3}n" },
                        { "text": " \u{2022} " },
                        { "text": "11 minutos y 8 segundos" }
                    ]
                }
            }
        }
    });

    let header = album_header(&payload);
    assert_eq!(header.artists, None);
    assert_eq!(header.container_label, None);
    assert_eq!(header.track_count, Some(1));
    assert_eq!(header.total_duration_secs, Some(668));
}
