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

/// Weight assigned to title similarity in confidence scoring.
pub const WEIGHT_TITLE: f32 = 0.5;

/// Weight assigned to artist similarity in confidence scoring.
pub const WEIGHT_ARTIST: f32 = 0.3;

/// Weight assigned to duration agreement in confidence scoring.
pub const WEIGHT_DURATION: f32 = 0.2;

/// Partial credit assigned when one normalized string contains the other as a substring.
pub const PARTIAL_MATCH_SCORE: f32 = 0.7;

/// Allowed absolute difference in seconds for two durations to be considered matching.
pub const DURATION_TOLERANCE_SECS: u64 = 5;

/// Neutral duration score when duration is missing from either requested track or candidate payload.
pub const DURATION_NEUTRAL_SCORE: f32 = 0.5;

/// Minimum confidence required to accept a lyrics payload (0.0 to 1.0).
///
/// Threshold is set to 0.75. With weights 0.5 title, 0.3 artist, 0.2 duration:
/// - An exact title (0.50) and matching duration (0.20) without artist match (0.0) yields
///   0.70 < 0.75, rejecting false positives from another artist with the same song title.
/// - An exact title (0.50) and exact artist (0.30) yields at least 0.80, safely passing
///   regardless of duration mismatch (e.g. outro silence).
/// - Exact title (0.50) with partial artist (0.21) passes with matching duration (0.91)
///   or neutral duration (0.81).
pub const CONFIDENCE_THRESHOLD: f32 = 0.75;

/// Trims leading and trailing whitespace, converts to lowercase, and collapses
/// sequences of interior whitespace characters into a single space.
pub fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Computes text similarity between expected metadata and a candidate payload field.
///
/// Returns:
/// - `1.0` for an exact match after normalization.
/// - `0.7` (`PARTIAL_MATCH_SCORE`) for partial credit when either normalized string contains the other
///   (e.g., handles "Creep" matching "Creep (Remastered)" or featured artist variants).
/// - `0.0` if either string is blank or neither contains the other.
pub fn text_similarity(expected: &str, candidate: &str) -> f32 {
    let norm_expected = normalize_text(expected);
    let norm_candidate = normalize_text(candidate);

    if norm_expected.is_empty() || norm_candidate.is_empty() {
        return 0.0;
    }
    if norm_expected == norm_candidate {
        return 1.0;
    }
    if norm_expected.contains(&norm_candidate) || norm_candidate.contains(&norm_expected) {
        return PARTIAL_MATCH_SCORE;
    }
    0.0
}

/// Evaluates agreement between expected track duration and candidate payload duration.
///
/// Returns:
/// - `0.5` (`DURATION_NEUTRAL_SCORE`) if either requested or candidate duration is `None`,
///   ensuring absent duration data neither inflates nor vetoes lyrics matching.
/// - `1.0` if both durations are present and their absolute difference is <= 5 seconds
///   (`DURATION_TOLERANCE_SECS`).
/// - `0.0` for differences greater than 5 seconds.
pub fn duration_similarity(expected: Option<u64>, candidate: Option<u64>) -> f32 {
    let (Some(exp), Some(cand)) = (expected, candidate) else {
        return DURATION_NEUTRAL_SCORE;
    };
    if exp.abs_diff(cand) <= DURATION_TOLERANCE_SECS { 1.0 } else { 0.0 }
}

/// Candidate track metadata extracted from a lyrics payload for confidence scoring.
pub struct LyricsCandidate<'a> {
    pub artist: &'a str,
    pub title: &'a str,
    pub duration: Option<u64>,
}

/// Computes the overall weighted confidence score for candidate lyrics against track metadata.
pub fn match_confidence(
    expected_artist: &str,
    expected_title: &str,
    expected_duration: Option<u64>,
    candidate: LyricsCandidate<'_>,
) -> f32 {
    let title_score = text_similarity(expected_title, candidate.title);
    let artist_score = text_similarity(expected_artist, candidate.artist);
    let duration_score = duration_similarity(expected_duration, candidate.duration);

    WEIGHT_TITLE * title_score + WEIGHT_ARTIST * artist_score + WEIGHT_DURATION * duration_score
}

fn extract_payload_duration(payload: &serde_json::Value) -> Option<u64> {
    let sec = payload.get("duration")?.as_f64()?;
    if sec >= 0.0 { Some(sec.round() as u64) } else { None }
}

/// Evaluates whether an LRCLIB response payload meets the minimum confidence threshold.
///
/// Extracts `trackName`, `artistName`, and `duration` from the candidate payload and compares
/// them against the expected track metadata using [`match_confidence`].
pub fn payload_matches(
    payload: &serde_json::Value,
    artist: &str,
    title: &str,
    duration_secs: Option<u64>,
) -> bool {
    let candidate_artist =
        payload.get("artistName").and_then(serde_json::Value::as_str).unwrap_or("");
    let candidate_title =
        payload.get("trackName").and_then(serde_json::Value::as_str).unwrap_or("");
    let candidate_duration = extract_payload_duration(payload);

    let candidate = LyricsCandidate {
        artist: candidate_artist,
        title: candidate_title,
        duration: candidate_duration,
    };
    let confidence = match_confidence(artist, title, duration_secs, candidate);
    confidence >= CONFIDENCE_THRESHOLD
}

impl LyricsSource for LrclibClient {
    /// Looks up synced lyrics for the given artist, track title, and optional duration.
    ///
    /// Trims input fields and fails fast if either is blank without issuing a network request.
    /// Uses reqwest's `.query()` mechanism for standard URL percent-encoding.
    /// Checks that the response payload passes [`payload_matches`] confidence scoring
    /// before extracting lyrics to avoid attaching wrong-track lyrics.
    /// Network errors or non-2xx status codes soft-fail to `None` because lyrics enrichment
    /// is purely best-effort and must never interrupt audio playback.
    fn lookup(
        &self,
        artist: &str,
        title: &str,
        duration_secs: Option<u64>,
    ) -> Option<SyncedLyrics> {
        let (artist, title) = sanitize_query(artist, title)?;
        let payload = self.fetch_payload(artist, title)?;
        if !payload_matches(&payload, artist, title, duration_secs) {
            return None;
        }
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
            assert_eq!(client.lookup(artist, title, None), None);
            assert_eq!(client.lookup(artist, title, Some(180)), None);
        }
    }

    #[test]
    fn lookup_connection_failure_soft_fails_to_none() {
        let client = LrclibClient::with_base_url("http://127.0.0.1:1/invalid");
        assert_eq!(client.lookup("Radiohead", "Creep", Some(236)), None);
    }

    #[test]
    fn parse_synced_lyrics_empty_returns_none() {
        assert_eq!(parse_synced_lyrics(""), None);
        assert_eq!(parse_synced_lyrics("[ab:cd]Invalid"), None);
        let valid = parse_synced_lyrics("[00:01.00]Valid line").expect("valid");
        assert_eq!(valid.lines.len(), 1);
    }

    #[test]
    fn normalize_text_trims_lowercases_and_collapses_whitespace() {
        assert_eq!(normalize_text("  Radiohead  "), "radiohead");
        assert_eq!(normalize_text("Creep   (Acoustic) "), "creep (acoustic)");
        assert_eq!(normalize_text("RADIOHEAD"), "radiohead");
        assert_eq!(normalize_text("\t\n  Karma \t Police \n"), "karma police");
        assert_eq!(normalize_text(""), "");
        assert_eq!(normalize_text("   \t  \n  "), "");
    }

    #[test]
    fn text_similarity_scoring_table() {
        // Exact after normalize scores 1.0
        assert_eq!(text_similarity("Creep", "Creep"), 1.0);
        assert_eq!(text_similarity("Radiohead", "RADIOHEAD"), 1.0);
        assert_eq!(text_similarity("Karma  Police", "  karma   police  "), 1.0);

        // Containment in either direction scores PARTIAL_MATCH_SCORE (0.7)
        assert_eq!(text_similarity("Creep", "Creep (Acoustic)"), PARTIAL_MATCH_SCORE);
        assert_eq!(text_similarity("Creep (Remastered)", "Creep"), PARTIAL_MATCH_SCORE);
        assert_eq!(text_similarity("Radiohead", "Radiohead feat. Thom Yorke"), PARTIAL_MATCH_SCORE);

        // Mismatches score 0.0
        assert_eq!(text_similarity("Creep", "Karma Police"), 0.0);
        assert_eq!(text_similarity("Radiohead", "Coldplay"), 0.0);

        // Blank input on either side scores 0.0
        assert_eq!(text_similarity("", "Creep"), 0.0);
        assert_eq!(text_similarity("Radiohead", ""), 0.0);
        assert_eq!(text_similarity("   ", "   "), 0.0);
    }

    #[test]
    fn duration_similarity_tolerance_boundaries() {
        // Exact match -> 1.0
        assert_eq!(duration_similarity(Some(200), Some(200)), 1.0);

        // Boundaries at <= 5s -> 1.0
        assert_eq!(duration_similarity(Some(200), Some(205)), 1.0);
        assert_eq!(duration_similarity(Some(200), Some(195)), 1.0);

        // Boundaries at > 5s -> 0.0
        assert_eq!(duration_similarity(Some(200), Some(206)), 0.0);
        assert_eq!(duration_similarity(Some(200), Some(194)), 0.0);
        assert_eq!(duration_similarity(Some(200), Some(400)), 0.0);

        // Missing either or both sides -> neutral 0.5 (never vetoes)
        assert_eq!(duration_similarity(None, Some(200)), DURATION_NEUTRAL_SCORE);
        assert_eq!(duration_similarity(Some(200), None), DURATION_NEUTRAL_SCORE);
        assert_eq!(duration_similarity(None, None), DURATION_NEUTRAL_SCORE);
    }

    #[test]
    fn match_confidence_weighted_matrix() {
        // Exact match across all fields: 0.5*1.0 + 0.3*1.0 + 0.2*1.0 = 1.0
        let full = match_confidence(
            "Radiohead",
            "Creep",
            Some(236),
            LyricsCandidate { artist: "Radiohead", title: "Creep", duration: Some(236) },
        );
        assert!((full - 1.0).abs() < f32::EPSILON);

        // Exact metadata with neutral duration: 0.5*1.0 + 0.3*1.0 + 0.2*0.5 = 0.90
        let neutral_dur = match_confidence(
            "Radiohead",
            "Creep",
            None,
            LyricsCandidate { artist: "Radiohead", title: "Creep", duration: Some(236) },
        );
        assert!((neutral_dur - 0.90).abs() < 1e-6);

        // Exact metadata with mismatched duration: 0.5*1.0 + 0.3*1.0 + 0.2*0.0 = 0.80
        let dur_mismatch = match_confidence(
            "Radiohead",
            "Creep",
            Some(100),
            LyricsCandidate { artist: "Radiohead", title: "Creep", duration: Some(236) },
        );
        assert!((dur_mismatch - 0.80).abs() < 1e-6);

        // Same title and matching duration, but completely wrong artist: 0.5*1.0 + 0.3*0.0 + 0.2*1.0 = 0.70
        let wrong_artist = match_confidence(
            "Radiohead",
            "Creep",
            Some(236),
            LyricsCandidate { artist: "TLC", title: "Creep", duration: Some(236) },
        );
        assert!((wrong_artist - 0.70).abs() < 1e-6);
        assert!(wrong_artist < CONFIDENCE_THRESHOLD);

        // Wrong title, exact artist, matching duration: 0.5*0.0 + 0.3*1.0 + 0.2*1.0 = 0.50
        let wrong_title = match_confidence(
            "Radiohead",
            "Creep",
            Some(236),
            LyricsCandidate { artist: "Radiohead", title: "Karma Police", duration: Some(236) },
        );
        assert!((wrong_title - 0.50).abs() < 1e-6);
        assert!(wrong_title < CONFIDENCE_THRESHOLD);
    }

    #[test]
    fn payload_matches_threshold_gate_accepts_confident_and_rejects_wrong() {
        let confident_payload = serde_json::json!({
            "trackName": "Creep",
            "artistName": "Radiohead",
            "duration": 236.0,
            "instrumental": false,
            "syncedLyrics": "[00:10.00]Line"
        });
        assert!(payload_matches(&confident_payload, "Radiohead", "Creep", Some(236)));
        assert!(payload_matches(&confident_payload, "Radiohead", "Creep", None));

        // Substring / partial title passes with matching duration: 0.5*0.7 + 0.3*1.0 + 0.2*1.0 = 0.85 >= 0.75
        let partial_title_payload = serde_json::json!({
            "trackName": "Creep (Acoustic)",
            "artistName": "Radiohead",
            "duration": 238
        });
        assert!(payload_matches(&partial_title_payload, "Radiohead", "Creep", Some(236)));

        // Wrong artist is rejected (0.70 < 0.75)
        let wrong_artist_payload = serde_json::json!({
            "trackName": "Creep",
            "artistName": "TLC",
            "duration": 236
        });
        assert!(!payload_matches(&wrong_artist_payload, "Radiohead", "Creep", Some(236)));

        // Wrong title is rejected (0.50 < 0.75)
        let wrong_title_payload = serde_json::json!({
            "trackName": "Karma Police",
            "artistName": "Radiohead",
            "duration": 236
        });
        assert!(!payload_matches(&wrong_title_payload, "Radiohead", "Creep", Some(236)));

        // Missing fields in payload default to 0.0 score and reject
        let empty_payload = serde_json::json!({});
        assert!(!payload_matches(&empty_payload, "Radiohead", "Creep", Some(236)));
    }
}
