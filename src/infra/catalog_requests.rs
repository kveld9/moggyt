//! Catalog API request builders, endpoint URLs, and mutation handlers.

use crate::i18n::Locale;
use serde_json::Value;

/// Home feed browse identifier for YouTube Music.
pub const HOME_BROWSE_ID: &str = "FEmusic_home";

/// Explore feed browse identifier for YouTube Music.
pub const EXPLORE_BROWSE_ID: &str = "FEmusic_explore";

/// Library playlists browse identifier for YouTube Music.
pub const LIBRARY_PLAYLISTS_BROWSE_ID: &str = "FEmusic_liked_playlists";

/// Library liked songs browse identifier for YouTube Music.
pub const LIBRARY_LIKED_BROWSE_ID: &str = "FEmusic_liked_videos";

/// Library saved albums browse identifier for YouTube Music.
pub const LIBRARY_ALBUMS_BROWSE_ID: &str = "FEmusic_liked_albums";

/// Public catalog API web key for the WEB_REMIX client context.
///
/// Embedded by every unofficial client; grants no private-data or billable-resource
/// access and cannot be rotated per installation (see `.gitleaks.toml` allowlist).
pub const CATALOG_API_KEY: &str = "AIzaSyC1xlcUPO2SCEmJEWeeIg4X6KyWnhDk-4M";

/// Catalog API client version for the WEB_REMIX client context.
///
/// Verified against live API on 2026-10-24. WEB_REMIX client versions
/// rotate over time; bump this value when YouTube starts rejecting search
/// requests with HTTP 400 FAILED_PRECONDITION. No auto-discovery in v1.
pub const CATALOG_CLIENT_VERSION: &str = "1.20251022.01.00";

/// Default region for catalog client requests.
pub const CATALOG_CLIENT_GL: &str = "US";

/// Returns the catalog API search endpoint URL.
pub fn search_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/search?key={CATALOG_API_KEY}&prettyPrint=false")
}

/// Returns the catalog API next endpoint URL.
pub fn next_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/next?key={CATALOG_API_KEY}&prettyPrint=false")
}

/// Builds the client context payload for WEB_REMIX requests.
pub fn client_context(locale: Locale) -> Value {
    serde_json::json!({
        "clientName": "WEB_REMIX",
        "clientVersion": CATALOG_CLIENT_VERSION,
        "hl": locale.tag(),
        "gl": CATALOG_CLIENT_GL,
    })
}

/// Builds the minimal WEB_REMIX search payload for `query` with `locale`.
pub fn search_body(query: &str, locale: Locale) -> Value {
    serde_json::json!({
        "context": {
            "client": client_context(locale),
        },
        "query": query,
    })
}

/// Builds the minimal WEB_REMIX next payload for `video_id` with `locale`.
pub fn next_body(video_id: &str, locale: Locale) -> Value {
    serde_json::json!({
        "context": {
            "client": client_context(locale),
        },
        "videoId": video_id,
        "playlistId": format!("RD{video_id}"),
    })
}

/// Returns the catalog API browse endpoint URL.
pub fn browse_url() -> String {
    format!("https://music.youtube.com/youtubei/v1/browse?key={CATALOG_API_KEY}&prettyPrint=false")
}

/// Builds the minimal WEB_REMIX browse payload for `browse_id` with `locale`.
pub fn browse_body(browse_id: &str, locale: Locale) -> Value {
    serde_json::json!({
        "context": {
            "client": client_context(locale),
        },
        "browseId": browse_id,
    })
}

/// Returns the catalog API like endpoint URL.
pub fn like_url(liked: bool) -> String {
    let action = if liked { "like" } else { "removelike" };
    format!(
        "https://music.youtube.com/youtubei/v1/like/{}?key={}&prettyPrint=false",
        action, CATALOG_API_KEY
    )
}

/// Builds the minimal WEB_REMIX like/removelike payload for `video_id` with `locale`.
pub fn like_body(video_id: &str, locale: Locale) -> Value {
    serde_json::json!({
        "context": {
            "client": client_context(locale),
        },
        "target": {
            "videoId": video_id,
        }
    })
}

/// Validates if a catalog API mutation response succeeded (no error object).
pub fn parse_mutation_response(value: &Value) -> bool {
    value.get("error").is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_url_format_and_key() {
        let url = search_url();
        assert_eq!(
            url,
            format!(
                "https://music.youtube.com/youtubei/v1/search?key={CATALOG_API_KEY}&prettyPrint=false"
            )
        );
    }

    #[test]
    fn search_body_structure() {
        for (locale, expected_hl) in [(Locale::English, "en"), (Locale::Spanish, "es")] {
            let body = search_body("midnight", locale);
            assert_eq!(body["query"], "midnight");
            assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
            assert_eq!(body["context"]["client"]["clientVersion"], CATALOG_CLIENT_VERSION);
            assert_eq!(body["context"]["client"]["hl"], expected_hl);
            assert_eq!(body["context"]["client"]["gl"], CATALOG_CLIENT_GL);
        }
    }

    #[test]
    fn next_url_format_and_key() {
        assert_eq!(
            next_url(),
            format!(
                "https://music.youtube.com/youtubei/v1/next?key={CATALOG_API_KEY}&prettyPrint=false"
            )
        );
    }

    #[test]
    fn next_body_structure() {
        for (locale, expected_hl) in [(Locale::English, "en"), (Locale::Spanish, "es")] {
            let body = next_body("abc-123", locale);
            assert_eq!(body["videoId"], "abc-123");
            assert_eq!(body["playlistId"], "RDabc-123");
            assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
            assert_eq!(body["context"]["client"]["clientVersion"], CATALOG_CLIENT_VERSION);
            assert_eq!(body["context"]["client"]["hl"], expected_hl);
            assert_eq!(body["context"]["client"]["gl"], CATALOG_CLIENT_GL);
        }
    }

    #[test]
    fn browse_url_format_and_key() {
        assert_eq!(
            browse_url(),
            format!(
                "https://music.youtube.com/youtubei/v1/browse?key={CATALOG_API_KEY}&prettyPrint=false"
            )
        );
    }

    #[test]
    fn browse_body_structure() {
        for (locale, expected_hl) in [(Locale::English, "en"), (Locale::Spanish, "es")] {
            let body = browse_body(HOME_BROWSE_ID, locale);
            assert_eq!(body["browseId"], HOME_BROWSE_ID);
            assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
            assert_eq!(body["context"]["client"]["clientVersion"], CATALOG_CLIENT_VERSION);
            assert_eq!(body["context"]["client"]["hl"], expected_hl);
            assert_eq!(body["context"]["client"]["gl"], CATALOG_CLIENT_GL);
        }
    }

    #[test]
    fn like_endpoints_and_mutation_parsing() {
        assert!(like_url(true).contains("/like/like?key="));
        assert!(like_url(false).contains("/like/removelike?key="));
        for (locale, expected_hl) in [(Locale::English, "en"), (Locale::Spanish, "es")] {
            let body = like_body("lib-t1", locale);
            assert_eq!(body["target"]["videoId"], "lib-t1");
            assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
            assert_eq!(body["context"]["client"]["hl"], expected_hl);
            assert_eq!(body["context"]["client"]["gl"], CATALOG_CLIENT_GL);
        }
        assert!(parse_mutation_response(&serde_json::json!({})));
        assert!(!parse_mutation_response(&serde_json::json!({"error": {"code": 401}})));
    }

    #[test]
    fn explore_browse_id_constant() {
        assert_eq!(EXPLORE_BROWSE_ID, "FEmusic_explore");
    }
}
