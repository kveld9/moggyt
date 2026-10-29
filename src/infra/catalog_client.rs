//! Catalog API client and response parser for YouTube Music search.
//!
//! Thin re-export hub providing backwards-compatible paths to modularized
//! request builders, raw data models, parser functions, and client logic.

pub use super::catalog_client_impl::CatalogClient;
pub use super::catalog_model::{RawAlbum, RawArtist, RawPlaylist, RawTrack, SearchHits};
pub use super::catalog_parse_common::{
    best_thumbnail, browse_id, browse_of, classify_home_item, flex_text, panel_artist,
    parse_duration_secs, parse_track_count, parse_two_row, thumbnail_candidates, watch_video_id,
};
pub use super::catalog_parse_detail::{
    build_detail_track, collect_detail_tracks, collect_shelf_tracks, detail_artist,
    detail_header_description, detail_sections, detail_title, parse_detail_all,
};
pub use super::catalog_parse_explore::parse_explore_all;
pub use super::catalog_parse_home::parse_home_all;
pub use super::catalog_parse_library::{
    parse_library_albums, parse_library_liked_tracks, parse_library_playlists,
};
pub use super::catalog_parse_radio::parse_radio_all;
pub use super::catalog_parse_search::{parse_search_all, parse_search_tracks, shelf_category};
pub use super::catalog_requests::{
    CATALOG_API_KEY, CATALOG_CLIENT_GL, CATALOG_CLIENT_VERSION, EXPLORE_BROWSE_ID, HOME_BROWSE_ID,
    LIBRARY_ALBUMS_BROWSE_ID, LIBRARY_LIKED_BROWSE_ID, LIBRARY_PLAYLISTS_BROWSE_ID, browse_body,
    browse_url, client_context, like_body, like_url, next_body, next_url, parse_mutation_response,
    search_body, search_url,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DetailKind, SearchFilter};
    use crate::i18n::Locale;

    #[test]
    fn hub_reexports_resolve_correctly() {
        assert_eq!(HOME_BROWSE_ID, "FEmusic_home");
        assert_eq!(EXPLORE_BROWSE_ID, "FEmusic_explore");
        assert_eq!(LIBRARY_PLAYLISTS_BROWSE_ID, "FEmusic_liked_playlists");
        assert_eq!(LIBRARY_LIKED_BROWSE_ID, "FEmusic_liked_videos");
        assert_eq!(LIBRARY_ALBUMS_BROWSE_ID, "FEmusic_liked_albums");
        assert!(!CATALOG_API_KEY.is_empty());
        assert!(!CATALOG_CLIENT_VERSION.is_empty());
        assert_eq!(CATALOG_CLIENT_GL, "US");

        assert!(search_url().contains("search"));
        assert!(next_url().contains("next"));
        assert!(browse_url().contains("browse"));
        assert!(like_url(true).contains("like"));

        let client = CatalogClient::new();
        assert_eq!(client.locale(), Locale::English);

        let hits = SearchHits::empty();
        assert!(hits.is_empty());

        assert_eq!(parse_duration_secs("1:30"), Some(90));
        assert_eq!(shelf_category("Songs"), Some(SearchFilter::Tracks));
        assert_eq!(parse_detail_all(&serde_json::json!({}), DetailKind::Album), None);
        assert!(parse_search_tracks(&serde_json::json!({})).is_empty());
        assert!(parse_radio_all(&serde_json::json!({})).is_empty());
        assert!(parse_home_all(&serde_json::json!({})).is_empty());
        assert!(parse_explore_all(&serde_json::json!({})).is_empty());
        assert!(parse_library_playlists(&serde_json::json!({})).is_empty());
        assert!(parse_library_albums(&serde_json::json!({})).is_empty());
        assert!(parse_library_liked_tracks(&serde_json::json!({})).is_empty());
    }
}
