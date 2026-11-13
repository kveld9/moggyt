//! Album credits parsed from catalog item metadata runs.

use serde_json::Value;

use super::catalog_parse_common::browse_page_type;
use crate::domain::Album;

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
pub fn album_credits(renderer: &Value) -> AlbumCredits {
    let Some(runs) = metadata_runs(renderer) else {
        return AlbumCredits::default();
    };
    AlbumCredits {
        artist: artist_text(runs),
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
    let head = runs.first()?.get("text")?.as_str()?.trim();
    if is_type_label(head) { Some(head.to_owned()) } else { None }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
