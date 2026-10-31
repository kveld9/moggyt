//! Playlist URL extraction and identifier validation.
//!
//! Validates public YouTube and YouTube Music playlist URLs and extracts the
//! canonical playlist browse identifier. Only HTTP and HTTPS schemes are supported.
//! Scheme and host matching is case-insensitive.
//!
//! # Allowed Hosts
//! - `music.youtube.com`: YouTube Music web client.
//! - `www.youtube.com`: YouTube main web client.
//! - `youtube.com`: YouTube apex domain.
//! - `youtu.be`: YouTube short-link domain (only valid when containing a `list` parameter).
//!
//! # Playlist Identifier Prefixes
//! Well-known public playlist identifier shapes:
//! - `PL`: Standard YouTube public and user-created playlists.
//! - `VL`: YouTube Music Virtual List container wrapper.
//! - `OLAK`: YouTube Music official album release playlists.
//! - `RD`: YouTube Music radio mixes and automix continuation playlists.

use serde::{Deserialize, Serialize};

const ALLOWED_HOSTS: &[&str] = &["music.youtube.com", "www.youtube.com", "youtube.com", "youtu.be"];

const ALLOWED_PREFIXES: &[&str] = &["PL", "VL", "OLAK", "RD"];

const SPOTIFY_HOST: &str = "open.spotify.com";
const SPOTIFY_ID_LEN: usize = 22;

fn split_scheme(raw: &str) -> Option<(&str, &str)> {
    if raw.len() >= 8 && raw[..8].eq_ignore_ascii_case("https://") {
        return Some(("https", &raw[8..]));
    }
    if raw.len() >= 7 && raw[..7].eq_ignore_ascii_case("http://") {
        return Some(("http", &raw[7..]));
    }
    None
}

fn split_host_and_rest(after_scheme: &str) -> (&str, &str) {
    match after_scheme.find(['/', '?']) {
        Some(idx) => (&after_scheme[..idx], &after_scheme[idx..]),
        None => (after_scheme, ""),
    }
}

fn strip_port(host: &str) -> &str {
    match host.find(':') {
        Some(idx) => &host[..idx],
        None => host,
    }
}

fn is_allowed_host(host: &str) -> bool {
    let clean = strip_port(host);
    ALLOWED_HOSTS.iter().any(|allowed| clean.eq_ignore_ascii_case(allowed))
}

fn extract_query_string(rest: &str) -> Option<&str> {
    let query_start = rest.find('?')? + 1;
    let query_part = &rest[query_start..];
    let end = query_part.find('#').unwrap_or(query_part.len());
    Some(&query_part[..end])
}

fn extract_query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key && !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

fn has_valid_id_chars(id: &str) -> bool {
    id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn has_allowed_prefix(id: &str) -> bool {
    ALLOWED_PREFIXES.iter().any(|prefix| id.starts_with(prefix) && id.len() > prefix.len())
}

fn is_valid_playlist_id(id: &str) -> bool {
    !id.is_empty() && has_allowed_prefix(id) && has_valid_id_chars(id)
}

fn resolve_raw_playlist_param(url: &str) -> Option<&str> {
    let (_scheme, after_scheme) = split_scheme(url)?;
    let (host, rest) = split_host_and_rest(after_scheme);
    if !is_allowed_host(host) {
        return None;
    }
    let query = extract_query_string(rest)?;
    extract_query_param(query, "list")
}

/// Extracts and validates a playlist identifier from a YouTube or YouTube Music URL.
///
/// Accepts standard playlist URLs and watch URLs carrying a `list` query parameter.
/// Returns `None` if the scheme is not HTTP/HTTPS, the host is not allowlisted,
/// the `list` parameter is missing or empty, or the identifier does not start
/// with an approved prefix (`PL`, `VL`, `OLAK`, `RD`).
pub fn extract_playlist_id(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let id = resolve_raw_playlist_param(trimmed)?;
    if !is_valid_playlist_id(id) {
        return None;
    }
    Some(id.to_owned())
}

/// Spotify resource kinds accepted by the importer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpotifyRef {
    Playlist(String),
    Album(String),
}

fn is_spotify_host(host: &str) -> bool {
    let clean = strip_port(host);
    clean.eq_ignore_ascii_case(SPOTIFY_HOST)
}

fn is_valid_spotify_id(id: &str) -> bool {
    id.len() == SPOTIFY_ID_LEN && id.chars().all(|c| c.is_ascii_alphanumeric())
}

fn parse_spotify_kind(kind: &str, id: &str) -> Option<SpotifyRef> {
    if !is_valid_spotify_id(id) {
        return None;
    }
    if kind.eq_ignore_ascii_case("playlist") {
        return Some(SpotifyRef::Playlist(id.to_owned()));
    }
    if kind.eq_ignore_ascii_case("album") {
        return Some(SpotifyRef::Album(id.to_owned()));
    }
    None
}

fn strip_query_and_fragment(raw: &str) -> &str {
    let without_frag = match raw.find('#') {
        Some(idx) => &raw[..idx],
        None => raw,
    };
    match without_frag.find('?') {
        Some(idx) => &without_frag[..idx],
        None => without_frag,
    }
}

fn is_intl_segment(segment: &str) -> bool {
    segment.len() > 5
        && segment[..5].eq_ignore_ascii_case("intl-")
        && segment[5..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_user_playlist_segments(segments: &[&str]) -> bool {
    segments[0].eq_ignore_ascii_case("user")
        && !segments[1].is_empty()
        && segments[2].eq_ignore_ascii_case("playlist")
}

fn parse_spotify_segments(slice: &[&str]) -> Option<SpotifyRef> {
    if slice.len() == 2 {
        return parse_spotify_kind(slice[0], slice[1]);
    }
    if slice.len() == 3 && slice[0].eq_ignore_ascii_case("embed") {
        return parse_spotify_kind(slice[1], slice[2]);
    }
    if slice.len() == 4 && is_user_playlist_segments(slice) {
        return parse_spotify_kind("playlist", slice[3]);
    }
    None
}

fn resolve_path_slice<'a>(segments: &'a [&'a str]) -> &'a [&'a str] {
    if let Some((first, rest)) = segments.split_first() {
        if is_intl_segment(first) {
            return rest;
        }
    }
    segments
}

fn parse_spotify_url(after_scheme: &str) -> Option<SpotifyRef> {
    let (host, rest) = split_host_and_rest(after_scheme);
    if !is_spotify_host(host) {
        return None;
    }
    let path = strip_query_and_fragment(rest);
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let slice = resolve_path_slice(&segments);
    parse_spotify_segments(slice)
}

fn is_spotify_uri_scheme(raw: &str) -> bool {
    raw.len() >= 8 && raw[..8].eq_ignore_ascii_case("spotify:")
}

fn parse_spotify_uri(after_scheme: &str) -> Option<SpotifyRef> {
    let clean = strip_query_and_fragment(after_scheme).trim_end_matches('/');
    let parts: Vec<&str> = clean.split(':').collect();
    if parts.len() == 2 {
        return parse_spotify_kind(parts[0], parts[1]);
    }
    if parts.len() == 4 && is_user_playlist_segments(&parts) {
        return parse_spotify_kind("playlist", parts[3]);
    }
    None
}

/// Extracts a Spotify playlist or album id from a user-supplied link or URI.
///
/// Accepts both web URLs (`open.spotify.com`) and Spotify URIs (`spotify:...`).
/// Spotify identifiers are expected to be 22-character base62 (ASCII alphanumeric) strings.
/// Returns [`None`] if the link is malformed, uses an unsupported host or scheme,
/// points to an unhandled resource kind (such as a track or artist), or has an invalid identifier.
pub fn parse_spotify_ref(raw: &str) -> Option<SpotifyRef> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_spotify_uri_scheme(trimmed) {
        return parse_spotify_uri(&trimmed[8..]);
    }
    let (_scheme, after_scheme) = split_scheme(trimmed)?;
    parse_spotify_url(after_scheme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_url_shapes_table() {
        let cases = [
            ("https://music.youtube.com/playlist?list=PL12345", "PL12345"),
            (
                "https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=PL4fGSI1pDJn6jXS_PECE3h6PBbcU42xy",
                "PL4fGSI1pDJn6jXS_PECE3h6PBbcU42xy",
            ),
            ("https://youtube.com/playlist?list=OLAK5uy_test", "OLAK5uy_test"),
            ("https://www.youtube.com/playlist?list=RDCLAK5uy", "RDCLAK5uy"),
            ("https://youtu.be/dQw4w9WgXcQ?list=VLPL123", "VLPL123"),
            ("https://www.youtube.com/watch?v=abc&index=3&list=PL98765&t=40", "PL98765"),
            ("  https://music.youtube.com/playlist?list=PL12345 \n\t ", "PL12345"),
            ("HTTPS://MUSIC.YOUTUBE.COM/playlist?list=PL12345", "PL12345"),
            ("Http://Www.YouTube.Com/watch?v=1&list=PL12345", "PL12345"),
            ("http://music.youtube.com/playlist?list=PL12345", "PL12345"),
            ("https://music.youtube.com/playlist?list=PL12345#overview", "PL12345"),
            ("https://music.youtube.com:443/playlist?list=PL12345", "PL12345"),
            ("https://music.youtube.com/playlist?list=OLAK5uy_k-123_abc", "OLAK5uy_k-123_abc"),
        ];

        for (url, expected_id) in cases {
            assert_eq!(
                extract_playlist_id(url),
                Some(expected_id.to_string()),
                "failed to parse expected id for: {url}"
            );
        }
    }

    #[test]
    fn rejected_url_shapes_table() {
        let cases = [
            "",
            "   ",
            "\t\n",
            "https://spotify.com/playlist?list=PL12345",
            "https://evil-music.youtube.com/playlist?list=PL12345",
            "https://youtube.com.attacker.com/playlist?list=PL12345",
            "https://music.youtube.com/playlist",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ",
            "https://music.youtube.com/playlist?other=value",
            "https://music.youtube.com/playlist?list=",
            "https://music.youtube.com/playlist?list=&other=val",
            "ftp://music.youtube.com/playlist?list=PL12345",
            "file:///music.youtube.com/playlist?list=PL12345",
            "javascript:alert(1)?list=PL12345",
            "music.youtube.com/playlist?list=PL12345",
            "//music.youtube.com/playlist?list=PL12345",
            "https://music.youtube.com/playlist?list=ALBUM123",
            "https://music.youtube.com/playlist?list=UNKNOWN123",
            "https://music.youtube.com/playlist?list=12345",
            "https://music.youtube.com/playlist?list=PL",
            "https://music.youtube.com/playlist?list=VL",
            "https://music.youtube.com/playlist?list=OLAK",
            "https://music.youtube.com/playlist?list=RD",
            "https://music.youtube.com/playlist?list=PL12345<script>",
            "https://music.youtube.com/playlist?list=PL123 45",
            "https://music.youtube.com/playlist?list=PL123$45",
        ];

        for url in cases {
            assert_eq!(extract_playlist_id(url), None, "expected rejection for: {url}");
        }
    }

    #[test]
    fn spotify_accepted_shapes_table() {
        let valid_id = "37i9dQZF1DXcBWIGoYBM5M";
        let cases = [
            (
                "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com/album/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Album(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com/intl-es/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com/intl-en/album/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Album(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com/intl-pt-BR/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com/user/spotify_user/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com/embed/album/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Album(valid_id.to_string()),
            ),
            (
                "http://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "http://open.spotify.com/album/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Album(valid_id.to_string()),
            ),
            (
                "http://open.spotify.com/intl-es/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "http://open.spotify.com/user/spotify_user/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "http://open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "http://open.spotify.com/embed/album/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Album(valid_id.to_string()),
            ),
            ("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M", SpotifyRef::Playlist(valid_id.to_string())),
            ("spotify:album:37i9dQZF1DXcBWIGoYBM5M", SpotifyRef::Album(valid_id.to_string())),
            (
                "spotify:user:spotify_user:playlist:37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "  https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M \n\t ",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "HTTPS://OPEN.SPOTIFY.COM/PLAYLIST/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com:443/playlist/37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=e8c07e21a8684a0c",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M#section",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M/",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M/",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M?si=123",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M#heading",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
            (
                "SPOTIFY:USER:ALICE:PLAYLIST:37i9dQZF1DXcBWIGoYBM5M",
                SpotifyRef::Playlist(valid_id.to_string()),
            ),
        ];

        for (raw, expected) in cases {
            assert_eq!(
                parse_spotify_ref(raw),
                Some(expected),
                "failed to parse expected ref for: {raw}"
            );
        }
    }

    #[test]
    fn spotify_rejected_shapes_table() {
        let cases = [
            "",
            "   ",
            "\t\n",
            "spotify:",
            "spotify:   ",
            "spotify:track:37i9dQZF1DXcBWIGoYBM5M",
            "spotify:artist:37i9dQZF1DXcBWIGoYBM5M",
            "spotify:show:37i9dQZF1DXcBWIGoYBM5M",
            "spotify:episode:37i9dQZF1DXcBWIGoYBM5M",
            "https://spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            "https://evil-open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            "https://open.spotify.com.attacker.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            "ftp://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            "file:///open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            "javascript:alert(1)",
            "https://open.spotify.com/track/37i9dQZF1DXcBWIGoYBM5M",
            "https://open.spotify.com/artist/37i9dQZF1DXcBWIGoYBM5M",
            "https://open.spotify.com/show/37i9dQZF1DXcBWIGoYBM5M",
            "https://open.spotify.com/episode/37i9dQZF1DXcBWIGoYBM5M",
            "https://open.spotify.com/playlist/",
            "https://open.spotify.com/playlist",
            "https://open.spotify.com/",
            "https://open.spotify.com/intl-es",
            "https://open.spotify.com/user/alice",
            "https://open.spotify.com/user/alice/playlist",
            "https://open.spotify.com/user//playlist/37i9dQZF1DXcBWIGoYBM5M",
            "spotify:user::playlist:37i9dQZF1DXcBWIGoYBM5M",
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5",
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M1",
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5_",
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5-",
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5$",
            "spotify:playlist:37i9dQZF1DXcBWIGoYBM5_",
            "spotify:playlist:",
            "spotify:playlist:short",
            "spotify:playlist:too_long_identifier_value_1234",
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M/extra",
            "https://open.spotify.com/user/alice/album/37i9dQZF1DXcBWIGoYBM5M",
        ];

        for raw in cases {
            assert_eq!(parse_spotify_ref(raw), None, "expected rejection for: {raw}");
        }
    }
}
