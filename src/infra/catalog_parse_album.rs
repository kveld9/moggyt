//! Album credits parsed from catalog item metadata runs.

use serde_json::Value;

use super::catalog_parse_common::browse_page_type;
use super::catalog_parse_detail::{
    collect_detail_tracks, fill_missing_album_track_artists, fill_missing_track_artwork,
};
use crate::domain::{Album, Detail};

/// Album credits carried by a catalog item's metadata runs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AlbumCredits {
    /// Credited artist names joined exactly as the payload lists them.
    pub artist: Option<String>,
    /// Browse id of the first credited artist.
    pub artist_id: Option<String>,
    /// Release type label when the item starts with one.
    pub release_type: Option<String>,
}

/// Parses the credited artist, its browse id, and the release type from a catalog item.
///
/// A release type is only accepted when the payload also credits an artist: without an artist run
/// the first run is ordinary subtitle text, not a metadata column with a type label.
pub fn album_credits(renderer: &Value) -> AlbumCredits {
    let Some(runs) = metadata_runs(renderer) else {
        return AlbumCredits::default();
    };
    let Some(artist) = artist_text(runs) else {
        return AlbumCredits::default();
    };
    AlbumCredits {
        artist: Some(artist),
        artist_id: runs.iter().find_map(artist_run_id),
        release_type: release_type_text(runs),
    }
}

/// Returns the album with the parsed artist, artist id, and release type applied.
pub fn apply_credits(album: Album, credits: &AlbumCredits) -> Album {
    let credited = credits.artist.as_deref().map(str::trim).filter(|name| !name.is_empty());
    let artist = credited.unwrap_or(album.artist.as_str());
    Album::new(&album.id, &album.title, artist, album.artwork)
        .with_artist_id(credits.artist_id.clone())
        .with_release_type(credits.release_type.clone())
}

fn metadata_runs(renderer: &Value) -> Option<&[Value]> {
    const RUN_PATHS: [&str; 2] =
        ["/subtitle/runs", "/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs"];
    RUN_PATHS.iter().find_map(|path| renderer.pointer(path)?.as_array().map(Vec::as_slice))
}

fn is_artist_run(run: &Value) -> bool {
    browse_page_type(run) == Some("MUSIC_PAGE_TYPE_ARTIST")
}

fn artist_run_id(run: &Value) -> Option<String> {
    if !is_artist_run(run) {
        return None;
    }
    let id = run.pointer("/navigationEndpoint/browseEndpoint/browseId")?.as_str()?.trim();
    if id.is_empty() { None } else { Some(id.to_owned()) }
}

fn artist_text(runs: &[Value]) -> Option<String> {
    let first = runs.iter().position(is_artist_run)?;
    let last = runs.iter().rposition(is_artist_run)?;
    let joined: String = runs[first..=last]
        .iter()
        .filter_map(|run| run.get("text").and_then(Value::as_str))
        .collect();
    let trimmed = joined.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

fn release_type_text(runs: &[Value]) -> Option<String> {
    let head = runs.first()?;
    if is_artist_run(head) {
        return None;
    }
    let text = head.get("text")?.as_str()?.trim();
    if is_type_label(text) { Some(text.to_owned()) } else { None }
}

/// Item-type labels the catalog prepends to a release or metadata run list.
const TYPE_LABELS: &[&str] = &[
    "album",
    "\u{e1}lbum",
    "artist",
    "artista",
    "cancion",
    "canci\u{f3}n",
    "ep",
    "episode",
    "episodio",
    "lista de reproduccion",
    "lista de reproducci\u{f3}n",
    "perfil",
    "playlist",
    "podcast",
    "p\u{f3}dcast",
    "profile",
    "sencillo",
    "single",
    "song",
    "video",
    "v\u{ed}deo",
];

/// Returns true when `raw` is a catalog item-type label.
pub(crate) fn is_type_label(raw: &str) -> bool {
    let lowered = raw.trim().to_lowercase();
    TYPE_LABELS.contains(&lowered.as_str())
}

/// Playlist id prefix carried by an album's canonical audio playlist.
const ALBUM_AUDIO_PLAYLIST_PREFIX: &str = "OLAK5uy_";

/// Returns the bare `OLAK5uy_...` audio playlist id carried by an album browse payload.
///
/// Precedence: header play button, first track row overlay, microformat canonical URL. Only album
/// audio playlist ids are accepted; radio and other container ids are ignored.
pub(super) fn album_audio_playlist_id(payload: &Value) -> Option<String> {
    [header_playlist_id(payload), row_playlist_id(payload), canonical_playlist_id(payload)]
        .into_iter()
        .flatten()
        .find_map(accept_album_playlist_id)
}

/// Returns true when any album row is an official or user-uploaded music video.
pub(super) fn has_music_video_rows(detail: &Detail) -> bool {
    detail.tracks.iter().any(|track| track.is_music_video())
}

/// Returns the album with its rows replaced by the tracks of its audio playlist payload.
///
/// Keeps the parsed album unchanged when the playlist payload carries no tracks, so a failed or
/// empty audio playlist degrades to the catalog rows instead of an empty album.
pub(super) fn replace_album_tracks_with_audio(
    mut detail: Detail,
    audio: &Value,
    now_secs: i64,
) -> Detail {
    let tracks = collect_detail_tracks(audio, now_secs);
    if tracks.is_empty() {
        return detail;
    }
    detail.tracks = tracks;
    let artists = detail.subtitle.clone();
    fill_missing_track_artwork(fill_missing_album_track_artists(detail, Some(artists.as_str())))
}

fn accept_album_playlist_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let rest = trimmed.strip_prefix(ALBUM_AUDIO_PLAYLIST_PREFIX)?;
    if rest.is_empty() { None } else { Some(trimmed.to_owned()) }
}

fn header_playlist_id(payload: &Value) -> Option<&str> {
    let buttons = payload
        .pointer(
            "/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicResponsiveHeaderRenderer/buttons",
        )?
        .as_array()?;
    buttons.iter().find_map(|button| {
        button
            .pointer(
                "/musicPlayButtonRenderer/playNavigationEndpoint/watchPlaylistEndpoint/playlistId",
            )?
            .as_str()
    })
}

fn row_playlist_id(payload: &Value) -> Option<&str> {
    let rows = payload
        .pointer(
            "/contents/twoColumnBrowseResultsRenderer/secondaryContents/sectionListRenderer/contents/0/musicShelfRenderer/contents",
        )?
        .as_array()?;
    rows.iter().find_map(|row| {
        row.pointer(
            "/musicResponsiveListItemRenderer/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/playlistId",
        )?
        .as_str()
    })
}

fn canonical_playlist_id(payload: &Value) -> Option<&str> {
    let url = payload.pointer("/microformat/microformatDataRenderer/urlCanonical")?.as_str()?;
    let (_, rest) = url.split_once("list=")?;
    Some(rest.split(['&', '#']).next().unwrap_or(rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DetailKind, MusicVideoType, Track, apply_detail_policy};
    use serde_json::json;

    fn audio_row(video_id: &str, title: &str, duration: &str) -> Value {
        json!({
            "musicResponsiveListItemRenderer": {
                "playlistItemData": { "videoId": video_id },
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{
                            "text": title,
                            "navigationEndpoint": { "watchEndpoint": {
                                "videoId": video_id,
                                "watchEndpointMusicSupportedConfigs": {
                                    "watchEndpointMusicConfig": {
                                        "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"
                                    }
                                }
                            } }
                        }] }
                    } },
                    { "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": "Bad Bunny" }] }
                    } }
                ],
                "fixedColumns": [
                    { "musicResponsiveListItemFixedColumnRenderer": {
                        "text": { "runs": [{ "text": duration }] }
                    } }
                ],
                "overlay": { "musicItemThumbnailOverlayRenderer": { "content": {
                    "musicPlayButtonRenderer": { "playNavigationEndpoint": { "watchEndpoint": {
                        "videoId": video_id,
                        "watchEndpointMusicSupportedConfigs": {
                            "watchEndpointMusicConfig": { "musicVideoType": "MUSIC_VIDEO_TYPE_ATV" }
                        }
                    } } }
                } } }
            }
        })
    }

    fn audio_payload(rows: Vec<Value>) -> Value {
        json!({ "contents": { "twoColumnBrowseResultsRenderer": { "secondaryContents": {
            "sectionListRenderer": { "contents": [
                { "musicPlaylistShelfRenderer": { "contents": rows } }
            ] }
        } } } })
    }

    fn release_card() -> Value {
        serde_json::json!({
            "subtitle": { "runs": [
                { "text": "Single" },
                { "text": " \u{2022} " },
                { "text": "Toosii", "navigationEndpoint": { "browseEndpoint": {
                    "browseId": "UC_toosii",
                    "browseEndpointContextSupportedConfigs": {
                        "browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                        }
                    }
                } } }
            ] }
        })
    }

    #[test]
    fn album_credits_reads_release_cards() {
        let credits = album_credits(&release_card());
        assert_eq!(credits.artist.as_deref(), Some("Toosii"));
        assert_eq!(credits.artist_id.as_deref(), Some("UC_toosii"));
        assert_eq!(credits.release_type.as_deref(), Some("Single"));
    }

    #[test]
    fn album_credits_joins_multiple_artists() {
        let renderer = serde_json::json!({
            "subtitle": { "runs": [
                { "text": "Single" },
                { "text": " \u{2022} " },
                { "text": "Kungs", "navigationEndpoint": { "browseEndpoint": {
                    "browseId": "UC_kungs",
                    "browseEndpointContextSupportedConfigs": {
                        "browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                        }
                    }
                } } },
                { "text": ", " },
                { "text": "PNAU", "navigationEndpoint": { "browseEndpoint": {
                    "browseId": "UC_pnau",
                    "browseEndpointContextSupportedConfigs": {
                        "browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                        }
                    }
                } } },
                { "text": " & " },
                { "text": "Crusy", "navigationEndpoint": { "browseEndpoint": {
                    "browseId": "UC_crusy",
                    "browseEndpointContextSupportedConfigs": {
                        "browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                        }
                    }
                } } }
            ] }
        });

        let credits = album_credits(&renderer);
        assert_eq!(credits.artist.as_deref(), Some("Kungs, PNAU & Crusy"));
        assert_eq!(credits.artist_id.as_deref(), Some("UC_kungs"));
        assert_eq!(credits.release_type.as_deref(), Some("Single"));
    }

    #[test]
    fn album_credits_reads_responsive_album_rows() {
        let renderer = serde_json::json!({
            "flexColumns": [
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Un Verano Sin Ti" }
                ] } } },
                { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [
                    { "text": "Album" },
                    { "text": " \u{2022} " },
                    { "text": "Bad Bunny", "navigationEndpoint": { "browseEndpoint": {
                        "browseId": "UC_bunny",
                        "browseEndpointContextSupportedConfigs": {
                            "browseEndpointContextMusicConfig": {
                                "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                            }
                        }
                    } } },
                    { "text": " \u{2022} " },
                    { "text": "2022" }
                ] } } }
            ]
        });

        let credits = album_credits(&renderer);
        assert_eq!(credits.artist.as_deref(), Some("Bad Bunny"));
        assert_eq!(credits.artist_id.as_deref(), Some("UC_bunny"));
        assert_eq!(credits.release_type.as_deref(), Some("Album"));
    }

    #[test]
    fn album_credits_are_absent_for_plain_subtitles() {
        let plain = serde_json::json!({ "subtitle": { "runs": [{ "text": "Album Artist 1" }] } });
        assert_eq!(album_credits(&plain), AlbumCredits::default());

        let no_runs = serde_json::json!({ "subtitle": {} });
        assert_eq!(album_credits(&no_runs), AlbumCredits::default());
    }

    #[test]
    fn album_credits_ignore_blank_artist_ids() {
        let renderer = serde_json::json!({
            "subtitle": { "runs": [
                { "text": "Single" },
                { "text": " \u{2022} " },
                { "text": "Nobody", "navigationEndpoint": { "browseEndpoint": {
                    "browseId": "   ",
                    "browseEndpointContextSupportedConfigs": {
                        "browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                        }
                    }
                } } }
            ] }
        });

        let credits = album_credits(&renderer);
        assert_eq!(credits.artist.as_deref(), Some("Nobody"));
        assert_eq!(credits.artist_id, None);
        assert_eq!(credits.release_type.as_deref(), Some("Single"));
    }

    #[test]
    fn apply_credits_keeps_the_subtitle_when_no_artist_is_credited() {
        let album = Album::new("a1", "Album", "Album Artist 1", None);
        let plain = apply_credits(album.clone(), &AlbumCredits::default());
        assert_eq!(plain, album);
    }

    #[test]
    fn apply_credits_replaces_the_type_with_the_credited_artist() {
        let album = Album::new("a1", "Call Me", "Single", None);
        let credited = apply_credits(album, &album_credits(&release_card()));
        assert_eq!(
            credited,
            Album::new("a1", "Call Me", "Toosii", None)
                .with_artist_id(Some("UC_toosii".to_owned()))
                .with_release_type(Some("Single".to_owned()))
        );
    }

    #[test]
    fn album_credits_without_artist_runs_ignore_the_type_label() {
        let plain = serde_json::json!({ "subtitle": { "runs": [{ "text": "Single" }] } });
        assert_eq!(album_credits(&plain), AlbumCredits::default());

        let album =
            apply_credits(Album::new("a1", "Call Me", "Single", None), &album_credits(&plain));
        assert_eq!(album.artist, "Single");
        assert_eq!(album.release_type, None);
        assert_eq!(album.caption(), "Single");
    }

    #[test]
    fn album_credits_ignore_a_head_run_that_is_an_artist_link() {
        let renderer = serde_json::json!({
            "subtitle": { "runs": [
                { "text": "Single", "navigationEndpoint": { "browseEndpoint": {
                    "browseId": "UC_single_band",
                    "browseEndpointContextSupportedConfigs": {
                        "browseEndpointContextMusicConfig": {
                            "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                        }
                    }
                } } }
            ] }
        });

        let credits = album_credits(&renderer);
        assert_eq!(credits.artist.as_deref(), Some("Single"));
        assert_eq!(credits.artist_id.as_deref(), Some("UC_single_band"));
        assert_eq!(credits.release_type, None);
    }

    #[test]
    fn album_audio_playlist_id_reads_the_header_play_button() {
        let payload = json!({
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicResponsiveHeaderRenderer": {
                                            "buttons": [{
                                                "musicPlayButtonRenderer": {
                                                    "playNavigationEndpoint": {
                                                        "watchPlaylistEndpoint": {
                                                            "playlistId": "OLAK5uy_header"
                                                        }
                                                    }
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        });
        assert_eq!(album_audio_playlist_id(&payload), Some("OLAK5uy_header".to_owned()));
    }

    #[test]
    fn album_audio_playlist_id_falls_back_to_the_first_row_overlay() {
        let payload = json!({
            "contents": {
                "twoColumnBrowseResultsRenderer": {
                    "secondaryContents": {
                        "sectionListRenderer": {
                            "contents": [{
                                "musicShelfRenderer": {
                                    "contents": [{
                                        "musicResponsiveListItemRenderer": {
                                            "overlay": {
                                                "musicItemThumbnailOverlayRenderer": {
                                                    "content": {
                                                        "musicPlayButtonRenderer": {
                                                            "playNavigationEndpoint": {
                                                                "watchEndpoint": {
                                                                    "playlistId": "OLAK5uy_row"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }]
                                }
                            }]
                        }
                    }
                }
            }
        });
        assert_eq!(album_audio_playlist_id(&payload), Some("OLAK5uy_row".to_owned()));
    }

    #[test]
    fn album_audio_playlist_id_falls_back_to_the_microformat_canonical_url() {
        let payload = json!({
            "microformat": {
                "microformatDataRenderer": {
                    "urlCanonical": "https://music.youtube.com/playlist?list=OLAK5uy_canonical&feature=applinks"
                }
            }
        });
        assert_eq!(album_audio_playlist_id(&payload), Some("OLAK5uy_canonical".to_owned()));
    }

    #[test]
    fn album_audio_playlist_id_rejects_foreign_playlist_ids() {
        for rejected in ["RDAMPLOLAK5uy_x", "VLOLAK5uy_x", "OLAK5uy_", "   "] {
            let payload = json!({
                "microformat": {
                    "microformatDataRenderer": {
                        "urlCanonical": format!("https://music.youtube.com/playlist?list={rejected}")
                    }
                }
            });
            assert_eq!(album_audio_playlist_id(&payload), None);
        }
        assert_eq!(album_audio_playlist_id(&json!({})), None);
    }

    #[test]
    fn has_music_video_rows_flags_only_omv_and_ugc() {
        let atv_track = Track::new("t1", "Track 1", "Artist").with_video_type(MusicVideoType::Atv);
        let unknown_track = Track::new("t2", "Track 2", "Artist");
        let mut detail =
            Detail::new(DetailKind::Album, "Album", "Artist", vec![atv_track, unknown_track]);
        assert!(!has_music_video_rows(&detail));

        let omv_track = Track::new("t3", "Track 3", "Artist").with_video_type(MusicVideoType::Omv);
        detail.tracks.push(omv_track);
        assert!(has_music_video_rows(&detail));

        let ugc_track = Track::new("t4", "Track 4", "Artist").with_video_type(MusicVideoType::Ugc);
        let ugc_detail = Detail::new(DetailKind::Album, "Album", "Artist", vec![ugc_track]);
        assert!(has_music_video_rows(&ugc_detail));
    }

    #[test]
    fn replace_album_tracks_with_audio_swaps_rows_and_backfills_metadata() {
        let omv_track = Track::new("video-1", "NI BIEN NI MAL", "Bad Bunny")
            .with_duration(306)
            .with_video_type(MusicVideoType::Omv);
        let detail = Detail::new(DetailKind::Album, "X 100PRE", "Bad Bunny", vec![omv_track])
            .with_artwork("https://img.local/album.jpg");
        let audio = audio_payload(vec![audio_row("atv-1", "NI BIEN NI MAL", "3:57")]);

        let updated = replace_album_tracks_with_audio(detail, &audio, 0);
        assert_eq!(updated.tracks.len(), 1);
        let track = &updated.tracks[0];
        assert_eq!(track.id, "atv-1");
        assert_eq!(track.duration_secs, Some(237));
        assert_eq!(track.video_type, Some(MusicVideoType::Atv));
        assert_eq!(track.artist, "Bad Bunny");
        assert_eq!(track.artwork.as_deref(), Some("https://img.local/album.jpg"));
    }

    #[test]
    fn replace_album_tracks_with_audio_keeps_rows_when_the_playlist_has_no_tracks() {
        let omv_track = Track::new("video-1", "NI BIEN NI MAL", "Bad Bunny")
            .with_duration(306)
            .with_video_type(MusicVideoType::Omv);
        let detail = Detail::new(DetailKind::Album, "X 100PRE", "Bad Bunny", vec![omv_track])
            .with_artwork("https://img.local/album.jpg");
        let result = replace_album_tracks_with_audio(detail.clone(), &json!({}), 0);
        assert_eq!(result, detail);
    }

    #[test]
    fn album_audio_tracks_survive_the_songs_only_policy() {
        let omv_track = Track::new("video-1", "NI BIEN NI MAL", "Bad Bunny")
            .with_duration(306)
            .with_video_type(MusicVideoType::Omv);
        let detail = Detail::new(DetailKind::Album, "X 100PRE", "Bad Bunny", vec![omv_track]);
        let audio = audio_payload(vec![
            audio_row("atv-1", "NI BIEN NI MAL", "3:57"),
            audio_row("atv-2", "Ser Bichote", "3:14"),
        ]);
        let detail = replace_album_tracks_with_audio(detail, &audio, 0);
        let filtered = apply_detail_policy(detail, true);
        assert_eq!(filtered.tracks.len(), 2);
    }
}
