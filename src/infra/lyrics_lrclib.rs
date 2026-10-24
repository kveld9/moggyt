//! LRCLIB lyrics client and pure LRC parser adapter.

use super::lyrics::{LyricLine, LyricsSource, SyncedLyrics};

/// Default LRCLIB API endpoint for track lyrics lookup.
pub fn lrclib_get_url() -> &'static str {
    "https://lrclib.net/api/get"
}

/// Real LRCLIB HTTP client implementing [`LyricsSource`].
pub struct LrclibClient {
    client: reqwest::blocking::Client,
    base_url: String,
}

impl LrclibClient {
    pub fn new() -> Self {
        Self::with_base_url(lrclib_get_url())
    }

    pub fn with_base_url(base: impl Into<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client, base_url: base.into() }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn fetch_payload(&self, artist: &str, title: &str) -> Option<serde_json::Value> {
        let response = self
            .client
            .get(&self.base_url)
            .query(&[("artist_name", artist), ("track_name", title)])
            .send()
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.json().ok()
    }
}

impl Default for LrclibClient {
    fn default() -> Self {
        Self::new()
    }
}

fn sanitize_query<'a>(artist: &'a str, title: &'a str) -> Option<(&'a str, &'a str)> {
    let artist = artist.trim();
    let title = title.trim();
    if artist.is_empty() || title.is_empty() {
        return None;
    }
    Some((artist, title))
}

fn parse_synced_lyrics(raw_lrc: &str) -> Option<SyncedLyrics> {
    let lines = parse_lrc(raw_lrc);
    if lines.is_empty() {
        return None;
    }
    Some(SyncedLyrics { lines })
}

impl LyricsSource for LrclibClient {
    /// Looks up synced lyrics for the given artist and track title.
    ///
    /// Trims input fields and fails fast if either is blank without issuing a network request.
    /// Uses reqwest's `.query()` mechanism for standard URL percent-encoding.
    /// Network errors or non-2xx status codes soft-fail to `None` because lyrics enrichment
    /// is purely best-effort and must never interrupt audio playback.
    fn lookup(&self, artist: &str, title: &str) -> Option<SyncedLyrics> {
        let (artist, title) = sanitize_query(artist, title)?;
        let payload = self.fetch_payload(artist, title)?;
        let raw_lrc = extract_synced(&payload)?;
        parse_synced_lyrics(raw_lrc)
    }
}

fn parse_centiseconds(frac: &str) -> Option<u64> {
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    frac.parse::<u64>().ok().map(|c| c * 10)
}

fn parse_milliseconds(frac: &str) -> Option<u64> {
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    frac.parse::<u64>().ok()
}

fn parse_fraction_ms(frac: &str) -> Option<u64> {
    match frac.len() {
        0 => Some(0),
        2 => parse_centiseconds(frac),
        3 => parse_milliseconds(frac),
        _ => None,
    }
}

fn parse_seconds_ms(sec_str: &str) -> Option<u64> {
    if sec_str.len() != 2 {
        return None;
    }
    if !sec_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let sec = sec_str.parse::<u64>().ok()?;
    if sec >= 60 {
        return None;
    }
    Some(sec * 1000)
}

fn parse_seconds_and_fraction(sec_part: &str) -> Option<u64> {
    let (sec_str, frac_str) = match sec_part.split_once('.') {
        Some((s, f)) => (s, f),
        None => (sec_part, ""),
    };
    let sec_ms = parse_seconds_ms(sec_str)?;
    let frac_ms = parse_fraction_ms(frac_str)?;
    Some(sec_ms + frac_ms)
}

fn parse_minutes_ms(min_str: &str) -> Option<u64> {
    if min_str.is_empty() {
        return None;
    }
    if !min_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let minutes = min_str.parse::<u64>().ok()?;
    minutes.checked_mul(60_000)
}

fn parse_timestamp_ms(tag: &str) -> Option<u64> {
    let (min_str, sec_str) = tag.split_once(':')?;
    let min_ms = parse_minutes_ms(min_str)?;
    let sec_ms = parse_seconds_and_fraction(sec_str)?;
    min_ms.checked_add(sec_ms)
}

fn extract_line_timestamps(mut line: &str) -> (Vec<u64>, &str) {
    let mut timestamps = Vec::new();
    while let Some(rest) = line.strip_prefix('[') {
        let Some(close_idx) = rest.find(']') else {
            break;
        };
        let tag = &rest[..close_idx];
        let Some(ms) = parse_timestamp_ms(tag) else {
            break;
        };
        timestamps.push(ms);
        line = &rest[close_idx + 1..];
    }
    (timestamps, line)
}

fn append_line_lyrics(line: &str, out: &mut Vec<LyricLine>) {
    let (timestamps, remainder) = extract_line_timestamps(line.trim());
    if timestamps.is_empty() {
        return;
    }
    let text = remainder.trim();
    if text.is_empty() {
        return;
    }
    for ts in timestamps {
        out.push(LyricLine { start_ms: ts, text: text.to_owned() });
    }
}

fn sort_and_dedup_lyrics(lines: &mut Vec<LyricLine>) {
    lines.sort_by(|a, b| a.start_ms.cmp(&b.start_ms).then_with(|| a.text.cmp(&b.text)));
    lines.dedup();
}

/// Parses raw LRC lyrics into ordered, deduplicated [`LyricLine`] items.
pub fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        append_line_lyrics(raw_line, &mut lines);
    }
    sort_and_dedup_lyrics(&mut lines);
    lines
}

/// Extracts synced lyrics text from an LRCLIB response payload.
///
/// Returns `None` if the track is marked as instrumental (per LRCLIB convention,
/// instrumental tracks have no vocal lyrics), if the `syncedLyrics` field is null or missing,
/// or if the content is purely blank whitespace.
fn extract_synced(payload: &serde_json::Value) -> Option<&str> {
    if payload.get("instrumental").and_then(serde_json::Value::as_bool).unwrap_or(false) {
        return None;
    }
    let text = payload.get("syncedLyrics").and_then(serde_json::Value::as_str)?;
    if text.trim().is_empty() {
        return None;
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lrc_centiseconds_and_bare_seconds_math() {
        let lrc = "[00:12.34]Line 1\n[00:03]Intro";
        let lines = parse_lrc(lrc);
        assert_eq!(
            lines,
            vec![
                LyricLine { start_ms: 3000, text: "Intro".to_owned() },
                LyricLine { start_ms: 12340, text: "Line 1".to_owned() },
            ]
        );
    }

    #[test]
    fn parse_lrc_milliseconds_and_multi_tag_lines() {
        let lrc = "[01:02.345]Chorus\n[00:10.00][00:20.00]Duplicated chorus";
        let lines = parse_lrc(lrc);
        assert_eq!(
            lines,
            vec![
                LyricLine { start_ms: 10000, text: "Duplicated chorus".to_owned() },
                LyricLine { start_ms: 20000, text: "Duplicated chorus".to_owned() },
                LyricLine { start_ms: 62345, text: "Chorus".to_owned() },
            ]
        );
    }

    #[test]
    fn parse_lrc_filters_invalid_and_sorts_and_deduplicates() {
        let lrc = "\
[ab:cd]Invalid tag
Just plain text without tag
[00:01.00]   
[00:05.00]Later line
[00:02.00]Earlier line
[00:05.00]Later line";
        let lines = parse_lrc(lrc);
        assert_eq!(
            lines,
            vec![
                LyricLine { start_ms: 2000, text: "Earlier line".to_owned() },
                LyricLine { start_ms: 5000, text: "Later line".to_owned() },
            ]
        );
        assert!(parse_lrc("").is_empty());
        assert!(parse_lrc("   \n\t  \n").is_empty());
    }

    #[test]
    fn extract_synced_handles_valid_null_blank_and_instrumental() {
        let valid = serde_json::json!({
            "instrumental": false,
            "syncedLyrics": "[00:10.00]Valid lyrics"
        });
        assert_eq!(extract_synced(&valid), Some("[00:10.00]Valid lyrics"));

        let instrumental = serde_json::json!({
            "instrumental": true,
            "syncedLyrics": "[00:10.00]Should be ignored"
        });
        assert_eq!(extract_synced(&instrumental), None);

        let null_lyrics = serde_json::json!({
            "instrumental": false,
            "syncedLyrics": null
        });
        assert_eq!(extract_synced(&null_lyrics), None);

        let blank_lyrics = serde_json::json!({
            "instrumental": false,
            "syncedLyrics": "   \n  "
        });
        assert_eq!(extract_synced(&blank_lyrics), None);

        let missing_key = serde_json::json!({});
        assert_eq!(extract_synced(&missing_key), None);
    }

    #[test]
    fn client_constructors_and_base_url_seam() {
        let default_client = LrclibClient::new();
        assert_eq!(default_client.base_url(), lrclib_get_url());

        let custom_client = LrclibClient::with_base_url("http://127.0.0.1:8080/api/get");
        assert_eq!(custom_client.base_url(), "http://127.0.0.1:8080/api/get");
    }

    #[test]
    fn lookup_blank_artist_or_title_returns_none_without_network() {
        let client = LrclibClient::with_base_url("http://127.0.0.1:0/unreachable");
        for (artist, title) in [
            ("", "Track Title"),
            ("Artist Name", ""),
            ("   ", "Track Title"),
            ("Artist Name", "\t\n  "),
            ("", ""),
        ] {
            assert_eq!(client.lookup(artist, title), None);
        }
    }

    #[test]
    fn lookup_connection_failure_soft_fails_to_none() {
        let client = LrclibClient::with_base_url("http://127.0.0.1:1/invalid");
        assert_eq!(client.lookup("Radiohead", "Creep"), None);
    }

    #[test]
    fn parse_synced_lyrics_empty_returns_none() {
        assert_eq!(parse_synced_lyrics(""), None);
        assert_eq!(parse_synced_lyrics("[ab:cd]Invalid"), None);
        let valid = parse_synced_lyrics("[00:01.00]Valid line").expect("valid");
        assert_eq!(valid.lines.len(), 1);
    }
}
