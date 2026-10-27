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

const ALLOWED_HOSTS: &[&str] = &["music.youtube.com", "www.youtube.com", "youtube.com", "youtu.be"];

const ALLOWED_PREFIXES: &[&str] = &["PL", "VL", "OLAK", "RD"];

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
}
