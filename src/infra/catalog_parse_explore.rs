//! Explore feed response parser for charts, releases, and genre shelves.

use super::catalog_parse_common::{
    best_thumbnail, browse_of, classify_home_item, parse_two_row, watch_video_id,
};
use super::catalog_parse_detail::{build_detail_track, detail_artist, detail_title};
use crate::domain::{Playlist, SearchResult, Shelf, Track};
use serde_json::Value;

fn explore_sections(value: &Value) -> Option<&Vec<Value>> {
    const PATH: &str = "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
    const ALT_PATH: &str = "/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
    const DIRECT_PATH: &str = "/sectionListRenderer/contents";
    value
        .pointer(PATH)
        .or_else(|| value.pointer(ALT_PATH))
        .or_else(|| value.pointer(DIRECT_PATH))
        .and_then(Value::as_array)
}

fn explore_shelf_title(shelf: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/header/musicCarouselShelfBasicHeaderRenderer/title/runs/0/text",
        "/header/musicShelfBasicHeaderRenderer/title/runs/0/text",
        "/header/gridHeaderRenderer/title/runs/0/text",
        "/header/musicHeaderRenderer/title/runs/0/text",
        "/title/runs/0/text",
        "/header/musicCarouselShelfBasicHeaderRenderer/title/simpleText",
        "/header/musicShelfBasicHeaderRenderer/title/simpleText",
        "/header/gridHeaderRenderer/title/simpleText",
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

fn explore_shelf_items_raw(shelf: &Value) -> Option<&[Value]> {
    shelf
        .get("contents")
        .or_else(|| shelf.get("items"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
}

fn parse_explore_responsive(renderer: &Value) -> Option<SearchResult> {
    if let Some(track) = build_detail_track(renderer) {
        return Some(SearchResult::Track(track));
    }
    let title = detail_title(renderer)?;
    let browse_id = browse_of(renderer)?;
    let artist = detail_artist(renderer);
    let thumb = best_thumbnail(renderer);
    Some(classify_home_item(&browse_id, &title, &artist, thumb))
}

fn parse_explore_two_row(renderer: &Value) -> Option<SearchResult> {
    let (title, subtitle, browse_id) = parse_two_row(renderer)?;
    let thumb = best_thumbnail(renderer);
    if let Some(id) = browse_id {
        return Some(classify_home_item(&id, &title, &subtitle, thumb));
    }
    let vid = watch_video_id(renderer)?;
    let mut track = Track::new(&vid, &title, &subtitle);
    if let Some(t) = thumb {
        track = track.with_artwork(t);
    }
    Some(SearchResult::Track(track))
}

fn parse_explore_button(renderer: &Value) -> Option<SearchResult> {
    let title = renderer
        .pointer("/buttonText/runs/0/text")
        .or_else(|| renderer.pointer("/buttonText/simpleText"))?
        .as_str()?
        .trim();
    if title.is_empty() {
        return None;
    }
    let browse_id = renderer
        .pointer("/clickCommand/browseEndpoint/browseId")
        .or_else(|| renderer.pointer("/navigationEndpoint/browseEndpoint/browseId"))?
        .as_str()?
        .trim();
    if browse_id.is_empty() {
        return None;
    }
    let thumb = best_thumbnail(renderer);
    Some(SearchResult::Playlist(Playlist::new(browse_id, title, 0, thumb)))
}

fn parse_explore_item(item: &Value) -> Option<SearchResult> {
    item.get("musicResponsiveListItemRenderer")
        .map_or_else(|| item.get("flexColumns").map(|_| item), Some)
        .and_then(parse_explore_responsive)
        .or_else(|| {
            item.get("musicTwoRowItemRenderer")
                .map_or_else(|| item.get("subtitle").map(|_| item), Some)
                .and_then(parse_explore_two_row)
        })
        .or_else(|| {
            item.get("musicNavigationButtonRenderer")
                .map_or_else(|| item.get("buttonText").map(|_| item), Some)
                .and_then(parse_explore_button)
        })
}

fn parse_explore_shelf(shelf: &Value) -> Option<Shelf> {
    let title = explore_shelf_title(shelf)?;
    let raw_items = explore_shelf_items_raw(shelf)?;
    let items: Vec<SearchResult> = raw_items.iter().filter_map(parse_explore_item).collect();
    if items.is_empty() {
        return None;
    }
    Some(Shelf { title, items })
}

fn extract_explore_shelf_node(section: &Value) -> Option<&Value> {
    const CANDIDATES: &[&str] = &[
        "/musicCarouselShelfRenderer",
        "/musicShelfRenderer",
        "/gridRenderer",
        "/musicGridRenderer",
        "/itemSectionRenderer/contents/0/musicShelfRenderer",
        "/itemSectionRenderer/contents/0/gridRenderer",
        "/itemSectionRenderer/contents/0/musicGridRenderer",
        "/itemSectionRenderer/contents/0/musicCarouselShelfRenderer",
    ];
    for path in CANDIDATES {
        if let Some(node) = section.pointer(path) {
            return Some(node);
        }
    }
    None
}

fn parse_explore_section(section: &Value) -> Option<Shelf> {
    if let Some(node) = extract_explore_shelf_node(section) {
        if let Some(shelf) = parse_explore_shelf(node) {
            return Some(shelf);
        }
    }
    parse_explore_shelf(section)
}

/// Parses explore carousel, chart, and genre shelves from catalog API browse response payload.
pub fn parse_explore_all(value: &Value) -> Vec<Shelf> {
    let Some(sections) = explore_sections(value) else {
        return Vec::new();
    };
    sections.iter().filter_map(parse_explore_section).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_explore_all_tolerates_empty_and_malformed() {
        assert!(parse_explore_all(&serde_json::json!({})).is_empty());
        assert!(parse_explore_all(&serde_json::json!({"contents": null})).is_empty());
        assert!(
            parse_explore_all(&serde_json::json!({
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
    fn parse_explore_all_golden_fixture() {
        let payload = serde_json::json!({
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicShelfRenderer": {
                                                "title": { "runs": [{ "text": "Charts" }] },
                                                "contents": [
                                                    {
                                                        "musicResponsiveListItemRenderer": {
                                                            "navigationEndpoint": {
                                                                "watchEndpoint": { "videoId": "chart-vid-1" }
                                                            },
                                                            "flexColumns": [
                                                                {
                                                                    "musicResponsiveListItemFlexColumnRenderer": {
                                                                        "text": { "runs": [{ "text": "Chart Song 1" }] }
                                                                    }
                                                                },
                                                                {
                                                                    "musicResponsiveListItemFlexColumnRenderer": {
                                                                        "text": { "runs": [{ "text": "Chart Artist" }] }
                                                                    }
                                                                }
                                                            ],
                                                            "thumbnailRenderer": {
                                                                "musicThumbnailRenderer": {
                                                                    "thumbnail": {
                                                                        "thumbnails": [
                                                                            {
                                                                                "url": "https://img.test/chart1.jpg",
                                                                                "width": 300,
                                                                                "height": 300
                                                                            }
                                                                        ]
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        },
                                        {
                                            "musicCarouselShelfRenderer": {
                                                "header": {
                                                    "musicCarouselShelfBasicHeaderRenderer": {
                                                        "title": { "runs": [{ "text": "New releases" }] }
                                                    }
                                                },
                                                "contents": [
                                                    {
                                                        "musicTwoRowItemRenderer": {
                                                            "title": { "runs": [{ "text": "New Album" }] },
                                                            "subtitle": { "runs": [{ "text": "Album Artist" }] },
                                                            "navigationEndpoint": {
                                                                "browseEndpoint": { "browseId": "MPREb_new_album" }
                                                            },
                                                            "thumbnailRenderer": {
                                                                "musicThumbnailRenderer": {
                                                                    "thumbnail": {
                                                                        "thumbnails": [
                                                                            {
                                                                                "url": "https://img.test/new_album.jpg",
                                                                                "width": 500,
                                                                                "height": 500
                                                                            }
                                                                        ]
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        },
                                        {
                                            "gridRenderer": {
                                                "header": {
                                                    "gridHeaderRenderer": {
                                                        "title": { "runs": [{ "text": "Moods & genres" }] }
                                                    }
                                                },
                                                "items": [
                                                    {
                                                        "musicNavigationButtonRenderer": {
                                                            "buttonText": { "runs": [{ "text": "Chill" }] },
                                                            "clickCommand": {
                                                                "browseEndpoint": { "browseId": "FEmusic_moods_and_genres_category" }
                                                            }
                                                        }
                                                    }
                                                ]
                                            }
                                        }
                                    ]
                                }
                            }
                        }}]
                    }
                }
        });

        let shelves = parse_explore_all(&payload);
        assert_eq!(shelves.len(), 3);

        assert_eq!(shelves[0].title, "Charts");
        assert_eq!(shelves[0].items.len(), 1);
        assert_eq!(
            shelves[0].items[0],
            SearchResult::Track(
                Track::new("chart-vid-1", "Chart Song 1", "Chart Artist")
                    .with_artwork("https://img.test/chart1.jpg")
            )
        );

        assert_eq!(shelves[1].title, "New releases");
        assert_eq!(shelves[1].items.len(), 1);
        assert_eq!(
            shelves[1].items[0],
            SearchResult::Album(Album::new(
                "MPREb_new_album",
                "New Album",
                "Album Artist",
                Some("https://img.test/new_album.jpg".to_owned())
            ))
        );

        assert_eq!(shelves[2].title, "Moods & genres");
        assert_eq!(shelves[2].items.len(), 1);
        assert_eq!(
            shelves[2].items[0],
            SearchResult::Playlist(Playlist::new(
                "FEmusic_moods_and_genres_category",
                "Chill",
                0,
                None
            ))
        );
    }

    #[test]
    fn parse_explore_charts_and_moods_genres_extended() {
        let payload = serde_json::json!({
            "sectionListRenderer": {
                "contents": [
                    {
                        "musicShelfRenderer": {
                            "title": { "simpleText": "Top Charts" },
                            "contents": [
                                {
                                    "musicResponsiveListItemRenderer": {
                                        "playlistItemData": { "videoId": "chart-vid-2" },
                                        "flexColumns": [
                                            {
                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                    "text": { "runs": [{ "text": "Chart Hit 2" }] }
                                                }
                                            },
                                            {
                                                "musicResponsiveListItemFlexColumnRenderer": {
                                                    "text": { "runs": [{ "text": "Chart Artist 2" }] }
                                                }
                                            }
                                        ]
                                    }
                                }
                            ]
                        }
                    },
                    {
                        "musicGridRenderer": {
                            "header": {
                                "gridHeaderRenderer": {
                                    "title": { "simpleText": "Moods & Genres" }
                                }
                            },
                            "items": [
                                {
                                    "musicNavigationButtonRenderer": {
                                        "buttonText": { "simpleText": "Workout" },
                                        "navigationEndpoint": {
                                            "browseEndpoint": {
                                                "browseId": "FEmusic_moods_and_genres_category_workout"
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                ]
            }
        });

        let shelves = parse_explore_all(&payload);
        assert_eq!(shelves.len(), 2);
        assert_eq!(shelves[0].title, "Top Charts");
        assert_eq!(shelves[0].items.len(), 1);
        assert_eq!(
            shelves[0].items[0],
            SearchResult::Track(Track::new("chart-vid-2", "Chart Hit 2", "Chart Artist 2"))
        );
        assert_eq!(shelves[1].title, "Moods & Genres");
        assert_eq!(shelves[1].items.len(), 1);
        assert_eq!(
            shelves[1].items[0],
            SearchResult::Playlist(Playlist::new(
                "FEmusic_moods_and_genres_category_workout",
                "Workout",
                0,
                None
            ))
        );
    }

    #[test]
    fn parse_explore_all_skips_empty_shelves() {
        let payload = serde_json::json!({
            "sectionListRenderer": {
                "contents": [
                    {
                        "musicShelfRenderer": {
                            "title": { "runs": [{ "text": "Empty Shelf" }] },
                            "contents": []
                        }
                    },
                    {
                        "musicShelfRenderer": {
                            "contents": [
                                {
                                    "musicTwoRowItemRenderer": {
                                        "title": { "runs": [{ "text": "Item without Shelf Title" }] }
                                    }
                                }
                            ]
                        }
                    }
                ]
            }
        });
        assert!(parse_explore_all(&payload).is_empty());
    }
}
