//! Library browse response parser for playlists, albums, and liked tracks.

use super::catalog_parse_album::{album_credits, apply_credits};
use super::catalog_parse_common::{
    best_thumbnail, browse_id, flex_text, parse_track_count, parse_two_row, two_row_title,
};
use super::catalog_parse_detail::{collect_shelf_tracks, detail_sections};
use crate::domain::{Album, Artist, Playlist, Track};
use serde_json::Value;

/// Renderer keys whose payload wraps a deeper library node or an item array.
const CONTAINER_KEYS: [&str; 5] = [
    "gridRenderer",
    "musicGridRenderer",
    "musicShelfRenderer",
    "itemSectionRenderer",
    "musicCarouselShelfRenderer",
];

fn wrapper_child(node: &Value) -> Option<&Value> {
    CONTAINER_KEYS.iter().find_map(|key| node.get(*key))
}

fn item_array(node: &Value) -> Option<&[Value]> {
    ["/items", "/contents"]
        .iter()
        .find_map(|path| node.pointer(path)?.as_array().map(Vec::as_slice))
}

fn expand_node<'a>(node: &'a Value, pending: &mut Vec<&'a Value>, items: &mut Vec<&'a Value>) {
    if let Some(child) = wrapper_child(node) {
        pending.push(child);
        return;
    }
    let Some(children) = item_array(node) else {
        return;
    };
    let (wrapped, leaves): (Vec<&Value>, Vec<&Value>) =
        children.iter().partition(|child| wrapper_child(child).is_some());
    pending.extend(wrapped);
    items.extend(leaves);
}

/// Flattens a library section into its item renderers.
///
/// The catalog nests library grids as
/// `sectionListRenderer.contents[].itemSectionRenderer.contents[].gridRenderer.items`,
/// while other library endpoints answer with a flat grid or shelf.
fn collect_section_items<'a>(section: &'a Value) -> Vec<&'a Value> {
    let mut items = Vec::new();
    let mut pending: Vec<&'a Value> = vec![section];
    let mut cursor = 0;
    while cursor < pending.len() {
        let node = pending[cursor];
        cursor += 1;
        expand_node(node, &mut pending, &mut items);
    }
    items
}

fn collect_library_items(value: &Value) -> Vec<&Value> {
    let roots: Vec<&Value> =
        detail_sections(value).map_or_else(|| vec![value], |sections| sections.iter().collect());
    roots.into_iter().flat_map(collect_section_items).collect()
}

/// Reads the track count from the subtitle runs of a library playlist card.
///
/// Real cards split the subtitle into runs ("Playlist", owner, "66 songs"), so the
/// count lives in the last run carrying digits instead of the first run.
fn playlist_track_count(renderer: &Value) -> usize {
    let Some(runs) = renderer.pointer("/subtitle/runs").and_then(Value::as_array) else {
        return 0;
    };
    runs.iter()
        .rev()
        .filter_map(|run| run.get("text").and_then(Value::as_str))
        .find(|text| text.chars().any(|c| c.is_ascii_digit()))
        .map(parse_track_count)
        .unwrap_or(0)
}

fn parse_library_playlist_item(item: &Value) -> Option<Playlist> {
    let renderer = item.get("musicTwoRowItemRenderer").unwrap_or(item);
    let title = two_row_title(renderer)?;
    let id = browse_id(renderer)?;
    let count = playlist_track_count(renderer);
    let thumb_url = best_thumbnail(renderer);
    Some(Playlist::new(&id, &title, count, thumb_url))
}

/// Parses saved or owned playlists from catalog API browse response payload.
///
/// The playlists grid opens with the catalog's "create a new playlist" card, which
/// carries no playable playlist and is dropped.
pub fn parse_library_playlists(value: &Value) -> Vec<Playlist> {
    collect_library_items(value)
        .into_iter()
        .skip(1)
        .filter_map(parse_library_playlist_item)
        .collect()
}

fn parse_library_album_item(item: &Value) -> Option<Album> {
    let renderer = item.get("musicTwoRowItemRenderer").unwrap_or(item);
    let (title, subtitle, browse_id) = parse_two_row(renderer)?;
    let id = browse_id?;
    let thumb_url = best_thumbnail(renderer);
    let credits = album_credits(renderer);
    Some(apply_credits(Album::new(&id, &title, &subtitle, thumb_url), &credits))
}

/// Parses saved albums from catalog API browse response payload.
pub fn parse_library_albums(value: &Value) -> Vec<Album> {
    collect_library_items(value).into_iter().filter_map(parse_library_album_item).collect()
}

fn parse_library_artist_item(item: &Value) -> Option<Artist> {
    let renderer = item.get("musicResponsiveListItemRenderer").unwrap_or(item);
    let id = renderer.pointer("/navigationEndpoint/browseEndpoint/browseId")?.as_str()?.trim();
    if id.is_empty() {
        return None;
    }
    let name = flex_text(renderer, 0)?;
    let subscribers = flex_text(renderer, 1);
    let artwork = best_thumbnail(renderer);
    Some(Artist::new(id, &name, artwork).with_subscribers(subscribers))
}

/// Parses followed artists from catalog API browse response payload.
pub fn parse_library_artists(value: &Value) -> Vec<Artist> {
    collect_library_items(value).into_iter().filter_map(parse_library_artist_item).collect()
}

/// Parses followed profiles (channels) from catalog API browse response payload.
pub fn parse_library_profiles(value: &Value) -> Vec<Artist> {
    collect_library_items(value).into_iter().filter_map(parse_library_artist_item).collect()
}

/// Parses library podcasts from catalog API browse response payload.
///
/// The podcasts grid opens with the catalog's "Add podcast" card, which carries no
/// playable playlist and is dropped.
pub fn parse_library_podcasts(value: &Value) -> Vec<Playlist> {
    collect_library_items(value)
        .into_iter()
        .skip(1)
        .filter_map(parse_library_playlist_item)
        .collect()
}

/// Parses liked songs from catalog API browse response payload.
pub fn parse_library_songs(value: &Value) -> Vec<Track> {
    let mut tracks = Vec::new();
    let Some(sections) = detail_sections(value) else {
        return tracks;
    };
    for section in sections {
        if let Some(shelf) = section.get("musicShelfRenderer") {
            collect_shelf_tracks(shelf, &mut tracks);
        }
    }
    tracks
}

/// Returns true when the catalog answered with an anonymous library page.
///
/// An expired or revoked session still answers HTTP 200, reports `logged_in=0`, and
/// renders a sign-in card instead of the library, which must not read as an empty
/// library.
pub fn payload_is_logged_out(value: &Value) -> bool {
    let Some(services) =
        value.pointer("/responseContext/serviceTrackingParams").and_then(Value::as_array)
    else {
        return false;
    };
    services
        .iter()
        .filter_map(|service| service.get("params").and_then(Value::as_array))
        .flatten()
        .any(is_logged_out_param)
}

fn is_logged_out_param(param: &Value) -> bool {
    param.get("key").and_then(Value::as_str) == Some("logged_in")
        && param.get("value").and_then(Value::as_str) == Some("0")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALBUM_ONE_ARTWORK: &str = "https://lh3.example.test/album1=s544";
    const ALBUM_TWO_ARTWORK: &str = "https://lh3.example.test/album2=s544";
    const FAV_ARTWORK: &str = "https://lh3.example.test/fav=s544";

    #[test]
    fn parse_library_playlists_golden_fixture_and_determinism() {
        let raw = include_str!("../../tests/fixtures/catalog_library_playlists.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let first = parse_library_playlists(&payload);
        let second = parse_library_playlists(&payload);
        assert_eq!(first, second, "parsing must be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(
            first[0],
            Playlist::new("VL_fav_mix", "Favorites Mix", 25, Some(FAV_ARTWORK.to_owned()))
        );
        assert_eq!(first[1], Playlist::new("VL_workout", "Workout Beats", 10, None));
    }

    #[test]
    fn parse_library_playlists_skips_the_only_create_card() {
        let payload = serde_json::json!({ "sectionListRenderer": { "contents": [{
            "itemSectionRenderer": { "contents": [{ "gridRenderer": { "items": [
                { "musicTwoRowItemRenderer": { "title": { "runs": [{ "text": "New playlist" }] } } }
            ] } }] }
        }] } });
        assert!(parse_library_playlists(&payload).is_empty());
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
        assert_eq!(
            first[0],
            Album::new(
                "MPREb_saved_1",
                "Saved Album 1",
                "Album Artist 1",
                Some(ALBUM_ONE_ARTWORK.to_owned())
            )
            .with_artist_id(Some("UC_artist_1".to_owned()))
            .with_release_type(Some("Album".to_owned()))
        );
        assert_eq!(
            first[1],
            Album::new(
                "MPREb_saved_2",
                "Saved Album 2",
                "Album Artist 2",
                Some(ALBUM_TWO_ARTWORK.to_owned())
            )
            .with_artist_id(Some("UC_artist_2".to_owned()))
            .with_release_type(Some("Album".to_owned()))
        );
    }

    #[test]
    fn parse_library_albums_tolerates_empty_and_malformed() {
        assert!(parse_library_albums(&serde_json::json!({})).is_empty());
        assert!(parse_library_albums(&serde_json::json!({"contents": null})).is_empty());
    }

    #[test]
    fn parse_library_albums_reads_artist_id() {
        let payload = serde_json::json!({ "sectionListRenderer": { "contents": [{ "gridRenderer": { "items": [{
            "musicTwoRowItemRenderer": {
                "title": { "runs": [{ "text": "Saved Album" }] },
                "subtitle": { "runs": [
                    { "text": "Album" },
                    { "text": " \u{2022} " },
                    { "text": "Album Artist", "navigationEndpoint": { "browseEndpoint": {
                        "browseId": "UC_saved_artist",
                        "browseEndpointContextSupportedConfigs": {
                            "browseEndpointContextMusicConfig": {
                                "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                            }
                        }
                    } } }
                ] },
                "navigationEndpoint": { "browseEndpoint": { "browseId": "MPREb_saved" } }
            }
        }] } }] } });

        let albums = parse_library_albums(&payload);
        assert_eq!(albums.len(), 1);
        assert_eq!(
            albums[0],
            Album::new("MPREb_saved", "Saved Album", "Album Artist", None)
                .with_artist_id(Some("UC_saved_artist".to_owned()))
                .with_release_type(Some("Album".to_owned()))
        );
    }

    #[test]
    fn parse_library_artists_reads_the_shelf_items() {
        let payload = serde_json::json!({ "sectionListRenderer": { "contents": [{
            "musicShelfRenderer": { "contents": [{
                "musicResponsiveListItemRenderer": {
                    "thumbnail": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [
                        { "url": "https://yt3.example.test/artist=w120", "width": 120, "height": 120 }
                    ] } } },
                    "flexColumns": [
                        { "musicResponsiveListItemFlexColumnRenderer": { "text": {
                            "runs": [{ "text": "Followed Artist" }]
                        } } },
                        { "musicResponsiveListItemFlexColumnRenderer": { "text": {
                            "runs": [{ "text": "1.49 K subscribers" }]
                        } } }
                    ],
                    "navigationEndpoint": { "browseEndpoint": {
                        "browseId": "UC_followed",
                        "browseEndpointContextSupportedConfigs": {
                            "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_ARTIST" }
                        }
                    } }
                }
            }] }
        }] } });

        let artists = parse_library_artists(&payload);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].id, "UC_followed");
        assert_eq!(artists[0].name, "Followed Artist");
        assert_eq!(artists[0].subscribers.as_deref(), Some("1.49 K subscribers"));
        assert!(artists[0].artwork.is_some());
    }

    #[test]
    fn parse_library_profiles_reads_the_channel_shelf() {
        let payload = serde_json::json!({ "sectionListRenderer": { "contents": [{
            "musicShelfRenderer": { "contents": [{
                "musicResponsiveListItemRenderer": {
                    "thumbnail": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": [
                        { "url": "https://yt3.example.test/channel=w120", "width": 120, "height": 120 }
                    ] } } },
                    "flexColumns": [
                        { "musicResponsiveListItemFlexColumnRenderer": { "text": {
                            "runs": [{ "text": "Followed Profile" }]
                        } } },
                        { "musicResponsiveListItemFlexColumnRenderer": { "text": {
                            "runs": [{ "text": "149 K subscribers" }]
                        } } }
                    ],
                    "navigationEndpoint": { "browseEndpoint": {
                        "browseId": "UC_profile",
                        "browseEndpointContextSupportedConfigs": {
                            "browseEndpointContextMusicConfig": { "pageType": "MUSIC_PAGE_TYPE_USER_CHANNEL" }
                        }
                    } }
                }
            }] }
        }] } });
        let profiles = parse_library_profiles(&payload);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "UC_profile");
        assert_eq!(profiles[0].name, "Followed Profile");
        assert_eq!(profiles[0].subscribers.as_deref(), Some("149 K subscribers"));
    }

    #[test]
    fn parse_library_podcasts_skips_the_add_card() {
        let payload = serde_json::json!({ "sectionListRenderer": { "contents": [{
            "gridRenderer": { "items": [
                { "musicTwoRowItemRenderer": { "title": { "runs": [{ "text": "Add podcast" }] } } },
                { "musicTwoRowItemRenderer": {
                    "title": { "runs": [{ "text": "Saved Podcast", "navigationEndpoint": {
                        "browseEndpoint": { "browseId": "MPSPPLD_saved" }
                    } }] },
                    "subtitle": { "runs": [{ "text": "Podcast" }] }
                } }
            ] }
        }] } });
        let podcasts = parse_library_podcasts(&payload);
        assert_eq!(podcasts.len(), 1);
        assert_eq!(podcasts[0].id, "MPSPPLD_saved");
        assert_eq!(podcasts[0].title, "Saved Podcast");
    }

    #[test]
    fn parse_library_songs_reads_the_liked_shelf() {
        let payload = serde_json::json!({ "sectionListRenderer": { "contents": [{
            "musicShelfRenderer": { "contents": [{
                "musicResponsiveListItemRenderer": {
                    "playlistItemData": { "videoId": "song-1" },
                    "flexColumns": [
                        { "musicResponsiveListItemFlexColumnRenderer": { "text": {
                            "runs": [{ "text": "Liked Song" }]
                        } } },
                        { "musicResponsiveListItemFlexColumnRenderer": { "text": {
                            "runs": [{ "text": "Liked Artist" }]
                        } } }
                    ],
                    "fixedColumns": [{ "musicResponsiveListItemFixedColumnRenderer": {
                        "text": { "runs": [{ "text": "3:20" }] }
                    } }]
                }
            }] }
        }] } });

        let songs = parse_library_songs(&payload);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].id, "song-1");
        assert_eq!(songs[0].title, "Liked Song");
        assert_eq!(songs[0].artist, "Liked Artist");
        assert_eq!(songs[0].duration_secs, Some(200));
    }

    #[test]
    fn payload_is_logged_out_reads_the_anonymous_library_page() {
        let raw = include_str!("../../tests/fixtures/catalog_library_logged_out.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        assert!(payload_is_logged_out(&payload));
        assert!(parse_library_playlists(&payload).is_empty());
        assert!(parse_library_albums(&payload).is_empty());
    }

    #[test]
    fn payload_is_logged_out_ignores_authenticated_payloads() {
        let raw = include_str!("../../tests/fixtures/catalog_library_playlists.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        assert!(!payload_is_logged_out(&payload));
        assert!(!payload_is_logged_out(&serde_json::json!({})));
    }
}
