//! Artist page response parser: top-songs shelf and carousel sections.

use serde_json::Value;

use super::catalog_parse_common::{best_thumbnail, browse_page_type};
use super::catalog_parse_detail::detail_sections;
use crate::domain::{ArtistPage, DetailCard, DetailCardKind, DetailSection};

/// Joins every text run under `node["runs"]` (separator runs are preserved verbatim).
fn joined_runs(node: Option<&Value>) -> String {
    let Some(runs) = node.and_then(|n| n.get("runs")).and_then(Value::as_array) else {
        return String::new();
    };
    let mut text = String::new();
    for run in runs {
        if let Some(part) = run.get("text").and_then(Value::as_str) {
            text.push_str(part);
        }
    }
    text.trim().to_owned()
}

fn songs_shelf(value: &Value) -> Option<&Value> {
    detail_sections(value)?.iter().find_map(|section| section.get("musicShelfRenderer"))
}

fn songs_title(value: &Value) -> String {
    joined_runs(songs_shelf(value).and_then(|shelf| shelf.get("title")))
}

fn songs_more_id(value: &Value) -> Option<String> {
    let id = songs_shelf(value)?.pointer("/bottomEndpoint/browseEndpoint/browseId")?.as_str()?;
    let trimmed = id.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

fn card_kind(renderer: &Value) -> Option<DetailCardKind> {
    match browse_page_type(renderer)? {
        "MUSIC_PAGE_TYPE_ALBUM" => Some(DetailCardKind::Album),
        "MUSIC_PAGE_TYPE_ARTIST" => Some(DetailCardKind::Artist),
        _ => None,
    }
}

fn parse_card(item: &Value) -> Option<DetailCard> {
    let renderer = item.get("musicTwoRowItemRenderer")?;
    let kind = card_kind(renderer)?;
    let title = joined_runs(renderer.get("title"));
    if title.is_empty() {
        return None;
    }
    let id = renderer.pointer("/navigationEndpoint/browseEndpoint/browseId")?.as_str()?.trim();
    if id.is_empty() {
        return None;
    }
    Some(DetailCard::new(
        id,
        &title,
        &joined_runs(renderer.get("subtitle")),
        best_thumbnail(renderer),
        kind,
    ))
}

fn parse_carousel(section: &Value) -> Option<DetailSection> {
    let carousel = section.get("musicCarouselShelfRenderer")?;
    let title =
        joined_runs(carousel.pointer("/header/musicCarouselShelfBasicHeaderRenderer/title"));
    if title.is_empty() {
        return None;
    }
    let cards: Vec<DetailCard> =
        carousel.get("contents")?.as_array()?.iter().filter_map(parse_card).collect();
    if cards.is_empty() { None } else { Some(DetailSection { title, cards }) }
}

/// Parses the artist-page payload into its top-songs heading, "show all" target, and sections.
///
/// Carousels whose items are neither album nor artist browse pages (for example "Videos")
/// are skipped because the player cannot open them as detail containers.
pub fn parse_artist_page(value: &Value) -> ArtistPage {
    let sections = detail_sections(value)
        .map(|sections| sections.iter().filter_map(parse_carousel).collect())
        .unwrap_or_default();
    ArtistPage { songs_title: songs_title(value), songs_more_id: songs_more_id(value), sections }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DetailKind;
    use crate::infra::catalog_parse_detail::parse_detail_all;
    use serde_json::json;

    fn runs(parts: &[&str]) -> Value {
        json!({ "runs": parts.iter().map(|p| json!({ "text": p })).collect::<Vec<_>>() })
    }

    fn square_thumb(url: &str) -> Value {
        json!({
            "thumbnailRenderer": {
                "musicThumbnailRenderer": {
                    "thumbnail": { "thumbnails": [{ "url": url, "width": 544, "height": 544 }] }
                }
            }
        })
    }

    fn album_card(id: &str, title: &str, subtitle: &[&str], artwork: &str) -> Value {
        let mut card = square_thumb(artwork);
        card["title"] = runs(&[title]);
        card["subtitle"] = runs(subtitle);
        card["navigationEndpoint"] = json!({
            "browseEndpoint": {
                "browseId": id,
                "browseEndpointContextSupportedConfigs": {
                    "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_ALBUM" }
                }
            }
        });
        json!({ "musicTwoRowItemRenderer": card })
    }

    fn artist_card(id: &str, title: &str, subtitle: &str, artwork: &str) -> Value {
        let mut card = square_thumb(artwork);
        card["title"] = runs(&[title]);
        card["subtitle"] = runs(&[subtitle]);
        card["navigationEndpoint"] = json!({
            "browseEndpoint": {
                "browseId": id,
                "browseEndpointContextSupportedConfigs": {
                    "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_ARTIST" }
                }
            }
        });
        json!({ "musicTwoRowItemRenderer": card })
    }

    fn video_card() -> Value {
        let mut card = square_thumb("https://img.local/video.jpg");
        card["title"] = runs(&["Video"]);
        card["subtitle"] = runs(&["Channel"]);
        card["navigationEndpoint"] = json!({ "watchEndpoint": { "videoId": "video-1" } });
        json!({ "musicTwoRowItemRenderer": card })
    }

    fn song_item(video_id: &str, title: &str, artwork: &str) -> Value {
        let column = |text: &str| json!({ "musicResponsiveListItemFlexColumnRenderer": { "text": runs(&[text]) } });
        json!({
            "musicResponsiveListItemRenderer": {
                "thumbnail": {
                    "musicThumbnailRenderer": {
                        "thumbnail": {
                            "thumbnails": [{ "url": artwork, "width": 120, "height": 120 }]
                        }
                    }
                },
                "overlay": {
                    "musicItemThumbnailOverlayRenderer": {
                        "content": {
                            "musicPlayButtonRenderer": {
                                "playNavigationEndpoint": {
                                    "watchEndpoint": { "videoId": video_id }
                                }
                            }
                        }
                    }
                },
                "flexColumns": [
                    column(title),
                    column("Los angeles de Santa Fe"),
                    column("1,2 M reproducciones"),
                    column("Nina Sin Corazon")
                ],
                "playlistItemData": { "videoId": video_id }
            }
        })
    }

    fn carousel(title: &str, items: Vec<Value>) -> Value {
        json!({
            "musicCarouselShelfRenderer": {
                "header": {
                    "musicCarouselShelfBasicHeaderRenderer": { "title": runs(&[title]) }
                },
                "contents": items
            }
        })
    }

    fn artist_payload() -> Value {
        json!({
            "header": {
                "musicImmersiveHeaderRenderer": {
                    "title": runs(&["Los angeles de Santa Fe"]),
                    "thumbnail": {
                        "musicThumbnailRenderer": {
                            "thumbnail": {
                                "thumbnails": [{ "url": "https://img.local/banner.jpg", "width": 2880, "height": 1200 }]
                            }
                        }
                    }
                }
            },
            "contents": {
                "singleColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [
                                        {
                                            "musicShelfRenderer": {
                                                "title": runs(&["Canciones mas escuchadas"]),
                                                "contents": [
                                                    song_item("vid-1", "Con Amor", "https://img.local/s1.jpg"),
                                                    song_item("vid-2", "Promesas de Amor", "https://img.local/s2.jpg")
                                                ],
                                                "bottomEndpoint": {
                                                    "browseEndpoint": { "browseId": "VLOLAK5uy_songs" }
                                                }
                                            }
                                        },
                                        carousel("Albumes", vec![
                                            album_card("MPREb_1", "Siempre Alto", &["2026"], "https://img.local/a1.jpg"),
                                            album_card("MPREb_2", "Nina Sin Corazon", &["2024"], "https://img.local/a2.jpg")
                                        ]),
                                        carousel("Singles y EPs", vec![
                                            album_card("MPREb_3", "Vivir a Tu Lado", &["Single", " . ", "2026"], "https://img.local/single.jpg")
                                        ]),
                                        carousel("Videos", vec![video_card()]),
                                        carousel("Puede que tambien te guste", vec![
                                            artist_card("UC_artist_1", "Simplemente Naguel", "254 K usuarios mensuales", "https://img.local/ar1.jpg"),
                                            artist_card("UC_artist_2", "Los Del Fuego", "6,41 M usuarios mensuales", "https://img.local/ar2.jpg")
                                        ])
                                    ]
                                }
                            }
                        }
                    }]
                }
            },
            "microformat": {
                "microformatDataRenderer": {
                    "title": "Los angeles de Santa Fe",
                    "description": "Escucha musica de Los angeles de Santa Fe en YouTube Music.",
                    "thumbnail": {
                        "thumbnails": [{ "url": "https://img.local/avatar.jpg", "width": 544, "height": 544 }]
                    }
                }
            }
        })
    }

    #[test]
    fn parse_artist_page_golden_shape() {
        let page = parse_artist_page(&artist_payload());
        assert_eq!(page.songs_title, "Canciones mas escuchadas");
        assert_eq!(page.songs_more_id.as_deref(), Some("VLOLAK5uy_songs"));
        assert_eq!(page.sections.len(), 3);
        assert_eq!(page.sections[0].title, "Albumes");
        assert_eq!(page.sections[0].cards.len(), 2);
        assert_eq!(page.sections[0].cards[0].kind, DetailCardKind::Album);
        assert_eq!(page.sections[0].cards[0].id, "MPREb_1");
        assert_eq!(page.sections[0].cards[0].title, "Siempre Alto");
        assert_eq!(page.sections[0].cards[0].subtitle, "2026");
        assert_eq!(page.sections[0].cards[0].artwork.as_deref(), Some("https://img.local/a1.jpg"));
        assert_eq!(page.sections[1].title, "Singles y EPs");
        assert_eq!(page.sections[1].cards[0].subtitle, "Single . 2026");
        assert_eq!(page.sections[2].title, "Puede que tambien te guste");
        assert_eq!(page.sections[2].cards[0].kind, DetailCardKind::Artist);
        assert_eq!(page.sections[2].cards[0].id, "UC_artist_1");
        assert_eq!(page.sections[2].cards[1].subtitle, "6,41 M usuarios mensuales");
    }

    #[test]
    fn parse_artist_page_empty_payload() {
        let page = parse_artist_page(&json!({}));
        assert_eq!(page.songs_title, "");
        assert_eq!(page.songs_more_id, None);
        assert!(page.sections.is_empty());
    }

    #[test]
    fn parse_detail_all_attaches_artist_page_without_boilerplate_description() {
        let detail =
            parse_detail_all(&artist_payload(), DetailKind::Artist).expect("artist detail");
        assert_eq!(detail.kind, DetailKind::Artist);
        assert_eq!(detail.title, "Los angeles de Santa Fe");
        assert_eq!(detail.subtitle, "");
        assert_eq!(detail.description, None, "artist microformat boilerplate must not leak");
        assert_eq!(detail.artwork.as_deref(), Some("https://img.local/avatar.jpg"));
        assert_eq!(detail.tracks.len(), 2);
        assert_eq!(detail.tracks[0].id, "vid-1");
        assert_eq!(detail.tracks[0].title, "Con Amor");
        assert_eq!(detail.tracks[0].artist, "Los angeles de Santa Fe");
        assert_eq!(detail.tracks[0].artwork.as_deref(), Some("https://img.local/s1.jpg"));
        let page = detail.artist.expect("artist page attached");
        assert_eq!(page.songs_more_id.as_deref(), Some("VLOLAK5uy_songs"));
        assert_eq!(page.sections.len(), 3);
    }
}
