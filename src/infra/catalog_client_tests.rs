//! Unit tests for CatalogClient.

use super::*;
use crate::infra::catalog_requests::CATALOG_API_KEY;

#[test]
fn cookie_value_reads_named_pairs() {
    let cookies = "SID=abc; SAPISID=xyz;  SSID=q";
    assert_eq!(cookie_value(cookies, "SAPISID"), Some("xyz"));
    assert_eq!(cookie_value(cookies, "SID"), Some("abc"));
    assert_eq!(cookie_value(cookies, "MISSING"), None);
    assert_eq!(cookie_value("SAPISID=  ", "SAPISID"), None);
}

#[test]
fn sapisid_authorization_signs_the_catalog_origin() {
    let cookies = "SID=abc; SAPISID=Y8DaQK1-VRZxrpWu/AOBQYOHADPvQdIhvU; SSID=q";
    assert_eq!(
        sapisid_authorization(cookies, 1_700_000_000).as_deref(),
        Some("SAPISIDHASH 1700000000_50bc7ad615487c9316ec93da978bcb26411ca453")
    );
    assert_eq!(sapisid_authorization("SID=abc", 1_700_000_000), None);
}

#[test]
fn catalog_client_detail_blank_id_returns_none() {
    let client = CatalogClient::new();
    assert_eq!(client.detail("", DetailKind::Album), Ok(None));
    assert_eq!(client.detail("   ", DetailKind::Artist), Ok(None));
}

#[test]
fn catalog_client_locale_configuration() {
    let client = CatalogClient::new();
    assert_eq!(client.locale(), Locale::English);

    let es_client = CatalogClient::new().with_locale(Locale::Spanish);
    assert_eq!(es_client.locale(), Locale::Spanish);

    let mut mut_client = CatalogClient::new();
    mut_client.set_locale(Locale::Spanish);
    assert_eq!(mut_client.locale(), Locale::Spanish);
}

#[test]
fn catalog_client_cookies_masking_and_unauthenticated_guard() {
    assert!(!CatalogClient::new().is_authenticated());
    assert!(!CatalogClient::with_cookies("   ").is_authenticated());
    let client = CatalogClient::with_cookies("secret-cookie-val");
    assert!(client.is_authenticated());
    let debug_str = format!("{client:?}");
    assert!(!debug_str.contains("secret-cookie-val"));
    assert!(debug_str.contains("has_cookies: true"));

    let anon = CatalogClient::new();
    assert_eq!(anon.library_playlists(), Err(CatalogError::Unauthenticated));
    assert_eq!(anon.library_saved_albums(), Err(CatalogError::Unauthenticated));
}

#[test]
fn unauthenticated_library_is_distinct_from_empty() {
    let anon = CatalogClient::new();
    assert_eq!(anon.library_playlists(), Err(CatalogError::Unauthenticated));
    assert_eq!(anon.library_saved_albums(), Err(CatalogError::Unauthenticated));
}

#[test]
fn empty_library_section_is_ok_empty() {
    let empty_payload = serde_json::json!({
        "sectionListRenderer": {
            "contents": [{
                "gridRenderer": {
                    "items": []
                }
            }]
        }
    });
    let playlists = parse_library_playlists(&empty_payload);
    assert_eq!(validate_library_result(playlists, &empty_payload), Ok(Vec::new()));

    let albums = parse_library_albums(&empty_payload);
    assert_eq!(validate_library_result(albums, &empty_payload), Ok(Vec::new()));

    let malformed_payload = serde_json::json!({});
    let malformed_playlists = parse_library_playlists(&malformed_payload);
    assert_eq!(
        validate_library_result(malformed_playlists, &malformed_payload),
        Err(CatalogError::Malformed)
    );
}

#[test]
fn logged_out_library_payload_is_unauthenticated() {
    let raw = include_str!("../../tests/fixtures/catalog_library_logged_out.json");
    let payload: Value = serde_json::from_str(raw).expect("valid fixture json");

    let playlists = parse_library_playlists(&payload);
    assert_eq!(validate_library_payload(playlists, &payload), Err(CatalogError::Unauthenticated));

    let albums = parse_library_albums(&payload);
    assert_eq!(validate_library_payload(albums, &payload), Err(CatalogError::Unauthenticated));
}

#[test]
fn empty_search_section_is_ok_empty() {
    let empty_payload = serde_json::json!({
        "contents": {
            "tabbedSearchResultsRenderer": {
                "tabs": []
            }
        }
    });
    let tracks: Vec<Track> = Vec::new();
    assert_eq!(validate_search_result(tracks, &empty_payload), Ok(Vec::new()));

    let malformed_payload = serde_json::json!({});
    let malformed_tracks: Vec<Track> = Vec::new();
    assert_eq!(
        validate_search_result(malformed_tracks, &malformed_payload),
        Err(CatalogError::Malformed)
    );
}

#[test]
fn catalog_client_history_detail_unauthenticated_returns_unauthenticated() {
    let client = CatalogClient::new();
    assert_eq!(
        client.detail(HISTORY_BROWSE_ID, DetailKind::Playlist),
        Err(CatalogError::Unauthenticated)
    );
}

#[test]
fn empty_explore_section_is_ok_empty() {
    let empty_payload = serde_json::json!({
        "sectionListRenderer": { "contents": [] }
    });
    let shelves: Vec<Shelf> = Vec::new();
    assert_eq!(validate_explore_result(shelves, &empty_payload), Ok(Vec::new()));

    let malformed_payload = serde_json::json!({});
    let malformed_shelves: Vec<Shelf> = Vec::new();
    assert_eq!(
        validate_explore_result(malformed_shelves, &malformed_payload),
        Err(CatalogError::Malformed)
    );
}

#[test]
fn empty_radio_section_is_ok_empty() {
    let empty_payload = serde_json::json!({
        "playlistPanelRenderer": { "contents": [] }
    });
    let tracks: Vec<Track> = Vec::new();
    assert_eq!(validate_radio_result(tracks, &empty_payload), Ok(Vec::new()));

    let malformed_payload = serde_json::json!({});
    let malformed_tracks: Vec<Track> = Vec::new();
    assert_eq!(
        validate_radio_result(malformed_tracks, &malformed_payload),
        Err(CatalogError::Malformed)
    );
}

#[test]
fn catalog_client_category_shelves_parses_minimal_payload_and_guards() {
    let client = CatalogClient::new();
    assert_eq!(client.category_shelves("", None), Ok(Vec::new()));
    assert_eq!(client.category_shelves("   ", Some("p")), Ok(Vec::new()));

    let payload = serde_json::json!({
        "sectionListRenderer": { "contents": [{
            "musicCarouselShelfRenderer": {
                "header": {
                    "musicCarouselShelfBasicHeaderRenderer": {
                        "title": { "runs": [{ "text": "Calm" }] }
                    }
                },
                "contents": [{
                    "musicTwoRowItemRenderer": {
                        "title": { "runs": [{ "text": "Piano" }] },
                        "navigationEndpoint": {
                            "browseEndpoint": { "browseId": "VL_piano" }
                        }
                    }
                }]
            }
        }] }
    });
    let shelves = parse_explore_all(&payload);
    assert_eq!(shelves.len(), 1);
    assert_eq!(shelves[0].title, "Calm");
    assert_eq!(
        shelves[0].items[0],
        crate::domain::SearchResult::Playlist(Playlist::new("VL_piano", "Piano", 0, None))
    );
    assert!(parse_explore_all(&serde_json::json!({})).is_empty());
    assert!(parse_explore_all(&serde_json::json!({"error": {"code": 404}})).is_empty());
}

#[test]
fn catalog_client_create_playlist_unauthenticated_returns_unauthenticated() {
    assert_eq!(CatalogClient::new().create_playlist("Mix"), Err(CatalogError::Unauthenticated));
}

#[test]
fn debug_output_does_not_leak_the_api_key() {
    let debug = format!("{:?}", CatalogClient::new());
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains(CATALOG_API_KEY));
}
