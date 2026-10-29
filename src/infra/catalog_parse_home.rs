//! Home carousel shelf response parser for browse payloads.

use super::catalog_parse_common::{best_thumbnail, classify_home_item, parse_two_row};
use crate::domain::{SearchResult, Shelf};
use serde_json::Value;

fn home_sections(value: &Value) -> Option<&Vec<Value>> {
    const PATH: &str = "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
    const ALT_PATH: &str = "/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
    value.pointer(PATH).or_else(|| value.pointer(ALT_PATH)).and_then(Value::as_array)
}

fn carousel_shelf_title(shelf: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text",
        "/title/runs/0/text",
        "/header/musicCarouselShelfBasicHeaderRenderer/title/simpleText",
        "/title/simpleText",
    ];
    for path in PATHS {
        if let Some(text) = shelf.pointer(path).and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn parse_carousel_item(item: &Value) -> Option<SearchResult> {
    let renderer = item.get("musicTwoRowItemRenderer")?;
    let (title, subtitle, browse_id) = parse_two_row(renderer)?;
    let id = browse_id?;
    let thumb = best_thumbnail(renderer);
    Some(classify_home_item(&id, &title, &subtitle, thumb))
}

fn parse_carousel_shelf(shelf: &Value) -> Option<Shelf> {
    let title = carousel_shelf_title(shelf)?;
    let items_array = shelf.get("contents")?.as_array()?;
    let items: Vec<SearchResult> = items_array.iter().filter_map(parse_carousel_item).collect();
    if items.is_empty() {
        return None;
    }
    Some(Shelf { title, items })
}

fn parse_home_section(section: &Value) -> Option<Shelf> {
    let shelf = section.get("musicCarouselShelfRenderer")?;
    parse_carousel_shelf(shelf)
}

/// Parses home carousel shelves from catalog API browse response payload.
///
/// Browse items are classified using the following heuristic on `browseId`:
/// - Starts with `"UC"`: [`SearchResult::Artist`]
/// - Starts with `"VL"` or `"RD"`: [`SearchResult::Playlist`] (with track_count 0)
/// - Otherwise: [`SearchResult::Album`]
///
/// Featured immersive carousels (`musicImmersiveCarouselShelfRenderer`), shelves
/// with empty titles, and shelves with zero kept items are skipped.
pub fn parse_home_all(value: &Value) -> Vec<Shelf> {
    let Some(sections) = home_sections(value) else {
        return Vec::new();
    };
    sections.iter().filter_map(parse_home_section).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_home_all_tolerates_empty_and_malformed() {
        assert!(parse_home_all(&serde_json::json!({})).is_empty());
        assert!(parse_home_all(&serde_json::json!({"contents": null})).is_empty());
        assert!(
            parse_home_all(&serde_json::json!({
                "contents": {
                    "singleColumnBrowseResultsRenderer": {
                        "tabs": [{
                            "tabRenderer": {
                                "content": {
                                    "sectionListRenderer": { "contents": [] }
                                }
                            }
                        }]
                    }
                }
            }))
            .is_empty()
        );
    }

    #[test]
    fn parse_home_all_golden_fixture() {
        let raw = include_str!("../../tests/fixtures/catalog_home.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let shelves = parse_home_all(&payload);
        assert_eq!(shelves.len(), 2);

        assert_eq!(shelves[0].title, "Quick picks");
        assert_eq!(shelves[0].items.len(), 2);
        assert_eq!(
            shelves[0].items[0],
            SearchResult::Album(Album::new(
                "MPREb_album_1",
                "Midnight City",
                "M83",
                Some("https://img.test/album1.jpg".to_owned())
            ))
        );
        assert_eq!(
            shelves[0].items[1],
            SearchResult::Artist(Artist::new("UC_artist_1", "M83", None))
        );

        assert_eq!(shelves[1].title, "Recommended mixes");
        assert_eq!(shelves[1].items.len(), 2);
        assert_eq!(
            shelves[1].items[0],
            SearchResult::Playlist(Playlist::new("VL_mix_1", "Chill Mix", 0, None))
        );
        assert_eq!(
            shelves[1].items[1],
            SearchResult::Playlist(Playlist::new("RD_radio_2", "Energy Radio", 0, None))
        );
    }

    #[test]
    fn parse_home_carousel_item_with_thumbnail_renderer_real_shape() {
        let payload = serde_json::json!({
            "musicTwoRowItemRenderer": {
                "title": { "runs": [{ "text": "Home Album Title" }] },
                "subtitle": { "runs": [{ "text": "Home Artist" }] },
                "navigationEndpoint": {
                    "browseEndpoint": { "browseId": "MPREb_home_1" }
                },
                "thumbnail": null,
                "thumbnailRenderer": {
                    "musicThumbnailRenderer": {
                        "thumbnail": {
                            "thumbnails": [
                                {
                                    "url": "https://yt3.googleusercontent.com/home-small.jpg",
                                    "width": 60,
                                    "height": 60
                                },
                                {
                                    "url": "https://yt3.googleusercontent.com/home-large.jpg",
                                    "width": 544,
                                    "height": 544
                                }
                            ]
                        }
                    }
                }
            }
        });

        let item = parse_carousel_item(&payload).expect("carousel item parsed");
        assert_eq!(
            item,
            SearchResult::Album(Album::new(
                "MPREb_home_1",
                "Home Album Title",
                "Home Artist",
                Some("https://yt3.googleusercontent.com/home-large.jpg".to_owned())
            ))
        );
    }
}
