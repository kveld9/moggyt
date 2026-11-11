//! lrc.red lyrics client: keyless search plus Apple-style TTML line timing.
//!
//! Only line timings are consumed here; word-level spans are flattened into the
//! paragraph text so the existing `LyricLine` model stays unchanged.

use serde_json::Value;

use super::lyrics::{LyricLine, LyricWord, LyricsSource, SyncedLyrics};
use super::lyrics_lrclib::{CONFIDENCE_THRESHOLD, LyricsCandidate, match_confidence};

pub const LRC_RED_SEARCH_URL: &str = "https://lrc.red/search.json";
pub const LRC_RED_RELEASE_BASE: &str = "https://lrc.red/s";
pub const LRC_RED_TIMEOUT_SECS: u64 = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct LrcRedHit {
    pub isrc: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub year: Option<u32>,
    pub duration_secs: Option<u64>,
}

pub fn ttml_url(isrc: &str) -> String {
    format!("{LRC_RED_RELEASE_BASE}/{isrc}.ttml")
}

pub fn build_search_query(title: &str, artist: &str) -> String {
    format!("{} {}", title.trim(), artist.trim())
}

fn parse_duration_secs(value: Option<&Value>) -> Option<u64> {
    let sec = value?.as_f64()?;
    if sec.is_finite() && sec >= 0.0 { Some(sec.round() as u64) } else { None }
}

fn parse_hit_entry(entry: &Value) -> Option<LrcRedHit> {
    let isrc = entry.get("isrc")?.as_str()?.trim();
    let title = entry.get("title")?.as_str()?.trim();
    let artist = entry.get("artist")?.as_str()?.trim();
    if isrc.is_empty() || title.is_empty() || artist.is_empty() {
        return None;
    }
    let album = entry
        .get("album")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let year = entry.get("year").and_then(Value::as_u64).and_then(|y| u32::try_from(y).ok());
    let duration_secs = parse_duration_secs(entry.get("duration"));

    Some(LrcRedHit {
        isrc: isrc.to_owned(),
        title: title.to_owned(),
        artist: artist.to_owned(),
        album,
        year,
        duration_secs,
    })
}

pub fn parse_hits(payload: &Value) -> Vec<LrcRedHit> {
    let Some(hits_array) = payload.get("hits").and_then(Value::as_array) else {
        return Vec::new();
    };
    hits_array.iter().filter_map(parse_hit_entry).collect()
}

fn duration_delta(expected: Option<u64>, candidate: Option<u64>) -> u64 {
    match (expected, candidate) {
        (Some(exp), Some(cand)) => exp.abs_diff(cand),
        _ => u64::MAX,
    }
}

fn is_better_candidate(cand_conf: f32, cand_delta: u64, best_conf: f32, best_delta: u64) -> bool {
    const EPSILON: f32 = 1e-5;
    if cand_conf > best_conf + EPSILON {
        return true;
    }
    if (cand_conf - best_conf).abs() <= EPSILON {
        return cand_delta < best_delta;
    }
    false
}

fn candidate_confidence(
    hit: &LrcRedHit,
    artist: &str,
    title: &str,
    duration_secs: Option<u64>,
) -> f32 {
    let candidate =
        LyricsCandidate { artist: &hit.artist, title: &hit.title, duration: hit.duration_secs };
    match_confidence(artist, title, duration_secs, candidate)
}

fn is_blank_query(title: &str, artist: &str) -> bool {
    title.trim().is_empty() || artist.trim().is_empty()
}

fn should_replace(best: Option<(&LrcRedHit, f32, u64)>, conf: f32, delta: u64) -> bool {
    match best {
        None => true,
        Some((_, best_conf, best_delta)) => is_better_candidate(conf, delta, best_conf, best_delta),
    }
}

pub fn select_hit<'a>(
    hits: &'a [LrcRedHit],
    title: &str,
    artist: &str,
    duration_secs: Option<u64>,
) -> Option<&'a LrcRedHit> {
    if is_blank_query(title, artist) {
        return None;
    }
    let (artist, title) = (artist.trim(), title.trim());
    let mut best: Option<(&'a LrcRedHit, f32, u64)> = None;
    for hit in hits {
        let conf = candidate_confidence(hit, artist, title, duration_secs);
        if conf < CONFIDENCE_THRESHOLD {
            continue;
        }
        let delta = duration_delta(duration_secs, hit.duration_secs);
        if should_replace(best, conf, delta) {
            best = Some((hit, conf, delta));
        }
    }
    best.map(|(hit, _, _)| hit)
}

fn parse_clock_part(part: &str) -> Option<f64> {
    let value = part.trim().parse::<f64>().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Some(value)
}

fn parse_timestamp_ms(raw: &str) -> Option<u64> {
    let trimmed = raw.trim().strip_suffix('s').unwrap_or(raw.trim());
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut total_secs = 0.0_f64;
    for part in parts {
        total_secs = total_secs * 60.0 + parse_clock_part(part)?;
    }
    Some((total_secs * 1000.0).round() as u64)
}

fn extract_attribute_value_ms(after: &str) -> Option<u64> {
    let after_eq = after.trim_start().strip_prefix('=')?.trim_start();
    let quote = after_eq.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let val_str = &after_eq[1..];
    let end_quote = val_str.find(quote)?;
    parse_timestamp_ms(&val_str[..end_quote])
}

fn parse_begin_ms(tag: &str) -> Option<u64> {
    parse_attribute_ms(tag, "begin")
}

fn parse_end_ms(tag: &str) -> Option<u64> {
    parse_attribute_ms(tag, "end")
}

fn parse_attribute_ms(tag: &str, name: &str) -> Option<u64> {
    let mut remainder = tag;
    while let Some(pos) = remainder.find(name) {
        let before = &remainder[..pos];
        let after = &remainder[pos + name.len()..];
        if before.ends_with(char::is_whitespace) {
            if let Some(ms) = extract_attribute_value_ms(after) {
                return Some(ms);
            }
        }
        remainder = after;
    }
    None
}

fn is_p_tag_boundary(c: Option<char>) -> bool {
    match c {
        Some(ch) => ch.is_ascii_whitespace() || ch == '>' || ch == '/',
        None => false,
    }
}

fn find_p_start(s: &str) -> Option<usize> {
    let mut offset = 0;
    while let Some(pos) = s[offset..].find("<p") {
        let abs_pos = offset + pos;
        let next_char = s[abs_pos + 2..].chars().next();
        if is_p_tag_boundary(next_char) {
            return Some(abs_pos);
        }
        offset = abs_pos + 2;
    }
    None
}

fn extract_paragraph(input: &str) -> Option<(&str, &str, &str)> {
    let p_start = find_p_start(input)?;
    let after_p = &input[p_start..];
    let open_tag_end = after_p.find('>')?;
    let open_tag = &after_p[..open_tag_end];
    let after_open = &after_p[open_tag_end + 1..];
    let close_idx = after_open.find("</p>")?;
    let body = &after_open[..close_idx];
    let rest = &after_open[close_idx + 4..];
    Some((open_tag, body, rest))
}

fn strip_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for c in input.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn decode_numeric_entity(token: &str) -> Option<char> {
    let digits = token.strip_prefix('#')?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let codepoint = digits.parse::<u32>().ok()?;
    char::from_u32(codepoint)
}

fn decode_entity_token(token: &str) -> Option<char> {
    match token {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => decode_numeric_entity(token),
    }
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut remainder = text;
    while let Some(amp_pos) = remainder.find('&') {
        out.push_str(&remainder[..amp_pos]);
        let after_amp = &remainder[amp_pos + 1..];
        match after_amp.find(';') {
            Some(semi_pos) if semi_pos <= 10 => {
                let token = &after_amp[..semi_pos];
                if let Some(ch) = decode_entity_token(token) {
                    out.push(ch);
                } else {
                    out.push('&');
                    out.push_str(token);
                    out.push(';');
                }
                remainder = &after_amp[semi_pos + 1..];
            }
            _ => {
                out.push('&');
                remainder = after_amp;
            }
        }
    }
    out.push_str(remainder);
    out
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn parse_ttml_lines(ttml: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    let mut cursor = ttml;
    while let Some((open_tag, body, rest)) = extract_paragraph(cursor) {
        cursor = rest;
        let Some(start_ms) = parse_begin_ms(open_tag) else {
            continue;
        };
        let stripped = strip_tags(body);
        let decoded = decode_entities(&stripped);
        let text = collapse_whitespace(&decoded);
        if text.is_empty() {
            continue;
        }
        lines.push(LyricLine::with_words(start_ms, text, parse_word_spans(body)));
    }
    lines
}

/// Parses `<span begin end>text</span>` runs inside a TTML paragraph body.
fn parse_word_spans(body: &str) -> Vec<LyricWord> {
    let mut words = Vec::new();
    let mut cursor = body;
    while let Some(start) = cursor.find("<span") {
        let after = &cursor[start + 5..];
        let Some(open_end) = after.find('>') else {
            break;
        };
        let open_tag = &after[..open_end];
        let rest = &after[open_end + 1..];
        let Some(close) = rest.find("</span>") else {
            break;
        };
        push_word(open_tag, &rest[..close], &mut words);
        cursor = &rest[close + 7..];
    }
    words
}

fn push_word(open_tag: &str, raw: &str, words: &mut Vec<LyricWord>) {
    let text = collapse_whitespace(&decode_entities(&strip_tags(raw)));
    if text.is_empty() {
        return;
    }
    let (Some(start_ms), Some(end_ms)) = (parse_begin_ms(open_tag), parse_end_ms(open_tag)) else {
        return;
    };
    if end_ms > start_ms {
        words.push(LyricWord { start_ms, end_ms, text });
    }
}

fn is_valid_isrc(isrc: &str) -> bool {
    !isrc.is_empty() && isrc.chars().all(|c| c.is_ascii_alphanumeric())
}

pub struct LrcRedClient {
    client: reqwest::blocking::Client,
    search_url: String,
    release_base: String,
}

impl LrcRedClient {
    pub fn new() -> Self {
        Self::with_endpoints(LRC_RED_SEARCH_URL, LRC_RED_RELEASE_BASE)
    }

    pub fn with_endpoints(search_url: impl Into<String>, release_base: impl Into<String>) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(LRC_RED_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self { client, search_url: search_url.into(), release_base: release_base.into() }
    }

    pub fn search_url(&self) -> &str {
        &self.search_url
    }

    pub fn release_base(&self) -> &str {
        &self.release_base
    }

    fn fetch_search_payload(&self, title: &str, artist: &str) -> Option<Value> {
        let query = build_search_query(title, artist);
        let response = self.client.get(&self.search_url).query(&[("q", query)]).send().ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.json().ok()
    }

    fn fetch_ttml(&self, isrc: &str) -> Option<String> {
        if !is_valid_isrc(isrc) {
            return None;
        }
        let url = format!("{}/{isrc}.ttml", self.release_base);
        let resp = self.client.get(&url).send().ok();
        match resp {
            Some(r) if r.status().is_success() => r.text().ok(),
            _ => {
                tracing::debug!(isrc = %isrc, "lrc.red ttml fetch failed");
                None
            }
        }
    }
}

impl Default for LrcRedClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LyricsSource for LrcRedClient {
    fn lookup(
        &self,
        artist: &str,
        title: &str,
        duration_secs: Option<u64>,
    ) -> Option<SyncedLyrics> {
        let artist = artist.trim();
        let title = title.trim();
        if artist.is_empty() || title.is_empty() {
            return None;
        }
        let payload = self.fetch_search_payload(title, artist)?;
        let hits = parse_hits(&payload);
        let hit = select_hit(&hits, title, artist, duration_secs)?;
        let raw_ttml = self.fetch_ttml(&hit.isrc)?;
        let lines = parse_ttml_lines(&raw_ttml);
        if lines.is_empty() {
            return None;
        }
        Some(SyncedLyrics { lines })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_SEARCH_JSON: &str = include_str!("../../tests/fixtures/lrcred_search.json");
    const FIXTURE_BOHEMIAN_TTML: &str = include_str!("../../tests/fixtures/lrcred_bohemian.ttml");

    #[test]
    fn parse_hits_reads_fixture_fields() {
        let value: Value = serde_json::from_str(FIXTURE_SEARCH_JSON).expect("valid fixture json");
        let hits = parse_hits(&value);
        assert_eq!(hits.len(), 4);
        assert_eq!(
            hits[0],
            LrcRedHit {
                isrc: "GBUM71029604".to_owned(),
                title: "Bohemian Rhapsody (Remastered 2011)".to_owned(),
                artist: "Queen".to_owned(),
                album: Some("A Night at the Opera".to_owned()),
                year: Some(1975),
                duration_secs: Some(357),
            }
        );
    }

    #[test]
    fn parse_hits_skips_entries_without_isrc_or_metadata() {
        let payload = serde_json::json!({
            "hits": [
                { "isrc": "", "title": "Song", "artist": "Artist" },
                { "isrc": "   ", "title": "Song", "artist": "Artist" },
                { "title": "Song", "artist": "Artist" },
                { "isrc": "ABC12345", "title": "", "artist": "Artist" },
                { "isrc": "ABC12345", "title": "Song", "artist": "" },
                { "isrc": "VALID12345", "title": "Song", "artist": "Artist" }
            ]
        });
        let hits = parse_hits(&payload);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].isrc, "VALID12345");
        assert_eq!(hits[0].album, None);
        assert_eq!(hits[0].year, None);
        assert_eq!(hits[0].duration_secs, None);

        let empty = serde_json::json!({});
        assert!(parse_hits(&empty).is_empty());
    }

    #[test]
    fn select_hit_prefers_matching_duration_over_live_edit() {
        let value: Value = serde_json::from_str(FIXTURE_SEARCH_JSON).expect("valid fixture json");
        let hits = parse_hits(&value);
        let selected = select_hit(&hits, "Bohemian Rhapsody", "Queen", Some(356));
        assert_eq!(selected.map(|h| h.isrc.as_str()), Some("GBUM71029604"));
    }

    #[test]
    fn select_hit_returns_none_for_unrelated_track() {
        let value: Value = serde_json::from_str(FIXTURE_SEARCH_JSON).expect("valid fixture json");
        let hits = parse_hits(&value);
        let selected = select_hit(&hits, "Creep", "Radiohead", Some(236));
        assert_eq!(selected, None);
    }

    #[test]
    fn select_hit_returns_none_on_blank_inputs() {
        let value: Value = serde_json::from_str(FIXTURE_SEARCH_JSON).expect("valid fixture json");
        let hits = parse_hits(&value);
        assert_eq!(select_hit(&hits, "", "Queen", Some(356)), None);
        assert_eq!(select_hit(&hits, "   ", "Queen", Some(356)), None);
        assert_eq!(select_hit(&hits, "Bohemian Rhapsody", "", Some(356)), None);
        assert_eq!(select_hit(&hits, "Bohemian Rhapsody", "\t\n ", Some(356)), None);
        assert_eq!(select_hit(&[], "Bohemian Rhapsody", "Queen", Some(356)), None);
    }

    #[test]
    fn parse_ttml_fixture_yields_one_line_per_paragraph() {
        let lines = parse_ttml_lines(FIXTURE_BOHEMIAN_TTML);
        assert_eq!(lines.len(), 6);
        let expected_ms = [0, 3725, 7126, 10087, 14607, 61636];
        let expected_text = [
            "Is this the real life?",
            "Is this just fantasy?",
            "Caught in a landslide",
            "No escape from reality",
            "Open your eyes",
            "Put a gun against his head",
        ];
        for (i, line) in lines.iter().enumerate() {
            assert_eq!(line.start_ms, expected_ms[i]);
            assert_eq!(line.text, expected_text[i]);
        }
    }

    #[test]
    fn parse_ttml_parses_minute_clock_timestamps() {
        let xml = r#"<p begin="1:01.636" end="1:04.234"><span begin="1:01.636" end="1:01.900">Put</span> <span begin="1:01.900" end="1:04.234">a gun</span></p>"#;
        let lines = parse_ttml_lines(xml);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].start_ms, 61636);
        assert_eq!(lines[0].text, "Put a gun");
        assert_eq!(lines[0].words.len(), 2);
        assert_eq!(lines[0].words[0].text, "Put");
        assert_eq!(lines[0].words[0].end_ms, 61900);
        assert_eq!(lines[0].words[1].text, "a gun");
    }

    #[test]
    fn parse_ttml_parses_hour_clock_timestamps() {
        let xml = r#"<p begin="1:02:03.500" end="1:02:04.000">Late</p>"#;
        let lines = parse_ttml_lines(xml);
        assert_eq!(lines, vec![LyricLine::new(3723500, "Late".to_owned())]);
    }

    #[test]
    fn parse_ttml_concatenates_word_spans_without_inserting_spaces() {
        let lines = parse_ttml_lines(FIXTURE_BOHEMIAN_TTML);
        assert!(lines[1].text.contains("fantasy?"));
        assert!(lines[3].text.contains("reality"));
    }

    #[test]
    fn parse_ttml_decodes_entities() {
        let xml = r#"<p begin="1.5" end="2.0">Tom &amp; Jerry &lt;3 &#65;</p>"#;
        let lines = parse_ttml_lines(xml);
        assert_eq!(lines, vec![LyricLine::new(1500, "Tom & Jerry <3 A".to_owned())]);
    }

    #[test]
    fn parse_ttml_skips_empty_lines() {
        let xml = r#"<p begin="1.0"></p><p begin="2.0">   <span>   </span> </p>"#;
        let lines = parse_ttml_lines(xml);
        assert!(lines.is_empty());
    }

    #[test]
    fn parse_ttml_ignores_unterminated_paragraph() {
        let xml = r#"<p begin="1.0">Unclosed text"#;
        let lines = parse_ttml_lines(xml);
        assert!(lines.is_empty());
    }

    #[test]
    fn ttml_url_and_query_builders() {
        assert_eq!(ttml_url("USRC17607839"), "https://lrc.red/s/USRC17607839.ttml");
        assert_eq!(
            build_search_query("  Bohemian Rhapsody ", " Queen "),
            "Bohemian Rhapsody Queen"
        );
    }

    #[test]
    fn lookup_rejects_blank_metadata_without_network() {
        let client = LrcRedClient::new();
        assert_eq!(client.lookup("", "x", None), None);
        assert_eq!(client.lookup("Artist", "  ", None), None);
        assert_eq!(client.lookup("   ", "   ", None), None);
    }

    #[test]
    fn client_constructors_and_endpoints() {
        let default_client = LrcRedClient::default();
        assert_eq!(default_client.search_url(), LRC_RED_SEARCH_URL);
        assert_eq!(default_client.release_base(), LRC_RED_RELEASE_BASE);

        let custom = LrcRedClient::with_endpoints("http://search", "http://release");
        assert_eq!(custom.search_url(), "http://search");
        assert_eq!(custom.release_base(), "http://release");
    }

    #[test]
    fn test_parse_ttml_extracts_word_timings_from_fixture() {
        let lines = parse_ttml_lines(FIXTURE_BOHEMIAN_TTML);
        let first = &lines[0];
        assert_eq!(first.text, "Is this the real life?");
        assert_eq!(first.words.len(), 5);
        assert_eq!(first.words[0].text, "Is");
        assert_eq!(first.words[0].start_ms, 0);
        assert_eq!(first.words[0].end_ms, 458);
        assert_eq!(first.words[4].text, "life?");
        assert_eq!(first.words[4].end_ms, 2906);
    }

    #[test]
    fn test_parse_ttml_splits_syllable_spans() {
        let lines = parse_ttml_lines(FIXTURE_BOHEMIAN_TTML);
        let syllables: Vec<&str> = lines[1].words.iter().map(|word| word.text.as_str()).collect();
        assert_eq!(syllables, vec!["Is", "this", "just", "fan", "ta", "sy?"]);
    }

    #[test]
    fn test_parse_ttml_paragraph_without_spans_has_no_words() {
        let lines = parse_ttml_lines(r#"<p begin="1.0" end="2.0">Plain line</p>"#);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].words.is_empty());
    }
}
