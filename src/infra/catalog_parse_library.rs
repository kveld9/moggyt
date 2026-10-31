//! Library browse response parser for playlists, albums, and liked tracks.

use super::catalog_parse_common::{best_thumbnail, parse_track_count, parse_two_row};
use super::catalog_parse_detail::detail_sections;
use crate::domain::{Album, Playlist};
use serde_json::Value;

fn find_grid_items(section: &Value) -> Option<&[Value]> {
    const CANDIDATES: &[&str] = &[
        "/gridRenderer/items",
        "/musicGridRenderer/items",
        "/musicShelfRenderer/contents",
        "/itemSectionRenderer/contents",
    ];
    CANDIDATES
        .iter()
        .find_map(|path| section.pointer(path).and_then(Value::as_array).map(Vec::as_slice))
}

fn collect_library_items(value: &Value) -> Vec<&Value> {
    if let Some(sections) = detail_sections(value) {
        let items: Vec<&Value> =
            sections.iter().filter_map(find_grid_items).flat_map(|slice| slice.iter()).collect();
        if !items.is_empty() {
            return items;
        }
    }
    find_grid_items(value).map_or_else(Vec::new, |slice| slice.iter().collect())
}

fn parse_library_playlist_item(item: &Value) -> Option<Playlist> {
    let renderer = item.get("musicTwoRowItemRenderer").unwrap_or(item);
    let (title, subtitle, browse_id) = parse_two_row(renderer)?;
    let id = browse_id?;
    let count = parse_track_count(&subtitle);
    let thumb_url = best_thumbnail(renderer);
    Some(Playlist::new(&id, &title, count, thumb_url))
}

/// Parses saved or owned playlists from catalog API browse response payload.
pub fn parse_library_playlists(value: &Value) -> Vec<Playlist> {
    collect_library_items(value).into_iter().filter_map(parse_library_playlist_item).collect()
}

fn parse_library_album_item(item: &Value) -> Option<Album> {
    let renderer = item.get("musicTwoRowItemRenderer").unwrap_or(item);
    let (title, subtitle, browse_id) = parse_two_row(renderer)?;
    let id = browse_id?;
    let thumb_url = best_thumbnail(renderer);
    Some(Album::new(&id, &title, &subtitle, thumb_url))
}

/// Parses saved albums from catalog API browse response payload.
pub fn parse_library_albums(value: &Value) -> Vec<Album> {
    collect_library_items(value).into_iter().filter_map(parse_library_album_item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_library_playlists_golden_fixture_and_determinism() {
        let raw = include_str!("../../tests/fixtures/catalog_library_playlists.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let first = parse_library_playlists(&payload);
        let second = parse_library_playlists(&payload);
        assert_eq!(first, second, "parsing must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], Playlist::new("VL_fav_mix", "Favorites Mix", 25, None));
        assert_eq!(first[1], Playlist::new("VL_workout", "Workout Beats", 10, None));
    }

    #[test]
    fn parse_library_playlists_tolerates_empty_and_malformed() {
        assert!(parse_library_playlists(&serde_json::json!({})).is_empty());
        assert!(parse_library_playlists(&serde_json::json!({"contents": null})).is_empty());
    }

    #[test]
    fn parse_library_albums_golden_fixture_and_determinism() {
        let raw = include_str!("../../tests/fixtures/catalog_library_albums.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let first = parse_library_albums(&payload);
        let second = parse_library_albums(&payload);
        assert_eq!(first, second, "parsing must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], Album::new("MPREb_saved_1", "Saved Album 1", "Album Artist 1", None));
        assert_eq!(first[1], Album::new("MPREb_saved_2", "Saved Album 2", "Album Artist 2", None));
    }

    #[test]
    fn parse_library_albums_tolerates_empty_and_malformed() {
        assert!(parse_library_albums(&serde_json::json!({})).is_empty());
        assert!(parse_library_albums(&serde_json::json!({"contents": null})).is_empty());
    }
}
