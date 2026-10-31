//! Multi-factor match scoring heuristics and title sanitization for Spotify imports.
//!
//! Invariant: this module performs no I/O, network calls, or clocks.

use serde::{Deserialize, Serialize};

use super::local_playlist::{MatchConfidence, SpotifyTrack};

/// Maximum score contribution for the title similarity factor.
pub const WEIGHT_TITLE: f32 = 0.40;

/// Score contribution when the primary artist matches the channel.
pub const WEIGHT_ARTIST_PRIMARY: f32 = 0.30;

/// Score contribution when a featured artist matches the channel.
pub const WEIGHT_ARTIST_SECONDARY: f32 = 0.15;

/// Maximum duration difference in seconds for an exact duration match.
pub const DURATION_EXACT_SECS: u32 = 2;

/// Maximum duration difference in seconds within acceptable tolerance.
pub const DURATION_TOLERANCE_SECS: u32 = 5;

/// Duration difference threshold in seconds above which a match is suspect.
pub const DURATION_SUSPECT_SECS: u32 = 15;

/// Duration difference threshold in seconds indicative of a wrong edit or version.
pub const DURATION_WRONG_EDIT_SECS: u32 = 30;

/// Score bonus awarded for exact duration matches.
pub const DURATION_EXACT_BONUS: f32 = 0.15;

/// Score penalty applied when duration difference is suspect.
pub const DURATION_SUSPECT_PENALTY: f32 = -0.20;

/// Score penalty applied when duration difference indicates a wrong edit.
pub const DURATION_WRONG_EDIT_PENALTY: f32 = -0.40;

/// Score penalty applied when duration difference indicates a music video or extra footage.
pub const DURATION_VIDEO_PENALTY: f32 = -0.60;

/// Score bonus awarded for auto-generated YouTube Music topic channels.
pub const PRISM_TOPIC_BONUS: f32 = 0.15;

/// Score bonus awarded for official Audio Track Video (ATV) items.
pub const PRISM_ATV_BONUS: f32 = 0.15;

/// Score penalty applied for Official Music Video (OMV) items.
pub const PRISM_OMV_PENALTY: f32 = -0.40;

/// Score penalty applied for User Generated Content (UGC) items.
pub const PRISM_UGC_PENALTY: f32 = -0.50;

/// Score penalty applied per music video keyword present in candidate title.
pub const PRISM_KEYWORD_PENALTY: f32 = -0.35;

/// Music video classification reported by InnerTube for a search hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MusicVideoType {
    Atv,
    Omv,
    Ugc,
    Other,
}

/// One YouTube Music search candidate evaluated against a Spotify track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchCandidate {
    pub title: String,
    pub channel: String,
    #[serde(default)]
    pub duration_secs: Option<u32>,
    pub video_type: MusicVideoType,
}

impl MatchCandidate {
    /// Creates a candidate with the given metadata.
    pub fn new(
        title: impl Into<String>,
        channel: impl Into<String>,
        duration_secs: Option<u32>,
        video_type: MusicVideoType,
    ) -> Self {
        Self { title: title.into(), channel: channel.into(), duration_secs, video_type }
    }
}

/// Per-factor score breakdown for one candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchScore {
    pub title: f32,
    pub artist: f32,
    pub duration: f32,
    pub prism: f32,
    pub total: f32,
}

fn normalize_smart_punctuation(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '`' | '\u{00B4}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{00AB}' | '\u{00BB}' => '"',
        '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
        '\u{00A0}' => ' ',
        _ => c,
    }
}

fn normalize_smart_punctuation_str(s: &str) -> String {
    s.chars().map(normalize_smart_punctuation).collect()
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_after_feat_marker(s: &str) -> Option<&str> {
    const PREFIXES: &[&str] = &["feat.", "feat ", "ft.", "ft ", "featuring "];
    let lower = s.to_ascii_lowercase();
    for &prefix in PREFIXES {
        if lower.starts_with(prefix) {
            return Some(s[prefix.len()..].trim());
        }
    }
    None
}

fn find_bracket_segment(s: &str) -> Option<(usize, usize, &str)> {
    for (start, ch) in s.char_indices() {
        if ch == '(' || ch == '[' {
            let close = if ch == '(' { ')' } else { ']' };
            let Some(end) = s[start..].find(close) else { continue };
            return Some((start, start + end + 1, &s[start + 1..start + end]));
        }
    }
    None
}

fn split_parenthetical_featured(s: &str) -> Option<(String, String)> {
    let (start, end, inner) = find_bracket_segment(s)?;
    let artist = extract_after_feat_marker(inner.trim())?;
    let before = &s[..start];
    let after = &s[end..];
    let title = collapse_whitespace(&format!("{before} {after}"));
    Some((title, artist.to_string()))
}

fn split_dash_featured(s: &str) -> Option<(String, String)> {
    let (before, after) = s.rsplit_once(" - ")?;
    let artist = extract_after_feat_marker(after.trim())?;
    Some((collapse_whitespace(before), artist.to_string()))
}

fn split_inline_featured(s: &str) -> Option<(String, String)> {
    const MARKERS: &[&str] = &[" featuring ", " feat. ", " ft. ", " feat ", " ft "];
    let lower = s.to_ascii_lowercase();
    for &marker in MARKERS {
        if let Some(pos) = lower.find(marker) {
            let before = s[..pos].trim();
            let after = s[pos + marker.len()..].trim();
            if let Some(open_idx) = after.find('(') {
                let artist = after[..open_idx].trim().to_string();
                let trailing = &after[open_idx..];
                let title = collapse_whitespace(&format!("{before} {trailing}"));
                return Some((title, artist));
            }
            return Some((collapse_whitespace(before), after.to_string()));
        }
    }
    None
}

/// Splits a track title into the base title and an optional featured artist clause.
pub fn split_featured(raw: &str) -> (String, Option<String>) {
    let normalized = normalize_smart_punctuation_str(raw);
    if let Some((title, feat)) = split_parenthetical_featured(&normalized) {
        return (title, Some(feat));
    }
    if let Some((title, feat)) = split_dash_featured(&normalized) {
        return (title, Some(feat));
    }
    if let Some((title, feat)) = split_inline_featured(&normalized) {
        return (title, Some(feat));
    }
    (collapse_whitespace(&normalized), None)
}

fn strip_featured_segments(s: &str) -> String {
    let mut current = s.to_string();
    while let (title, Some(_)) = split_featured(&current) {
        current = title;
    }
    current
}

fn is_preserved_qualifier(s: &str) -> bool {
    const PRESERVED_WORDS: &[&str] = &["live", "acoustic", "instrumental", "demo"];
    let lower = s.to_ascii_lowercase();
    if lower.contains("radio edit") {
        return true;
    }
    lower.split(|c: char| !c.is_alphanumeric()).any(|w| PRESERVED_WORDS.contains(&w))
}

fn is_noise_keyword(lower: &str) -> bool {
    const NOISE_PHRASES: &[&str] =
        &["remaster", "deluxe", "expanded", "anniversary", "bonus track", "explicit", "clean"];
    NOISE_PHRASES.iter().any(|&phrase| lower.contains(phrase))
}

fn is_year_mix(lower: &str) -> bool {
    if !lower.contains("mix") {
        return false;
    }
    lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()))
}

fn is_featuring_marker(s: &str) -> bool {
    const PREFIXES: &[&str] = &["feat.", "feat ", "ft.", "ft ", "featuring "];
    let lower = s.to_ascii_lowercase();
    PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
}

fn is_noise_tag(segment: &str) -> bool {
    if is_preserved_qualifier(segment) {
        return false;
    }
    let lower = segment.to_ascii_lowercase();
    is_noise_keyword(&lower)
        || is_year_mix(&lower)
        || lower.trim() == "bonus"
        || is_featuring_marker(&lower)
}

fn find_next_bracket(s: &str) -> Option<(char, char, usize)> {
    let paren = s.find('(').map(|pos| ('(', ')', pos));
    let bracket = s.find('[').map(|pos| ('[', ']', pos));
    match (paren, bracket) {
        (Some(p), Some(b)) if p.2 < b.2 => Some(p),
        (Some(_), Some(b)) => Some(b),
        (Some(p), None) => Some(p),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn strip_noise_parentheticals(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some((open_char, close_char, start)) = find_next_bracket(remaining) {
        result.push_str(&remaining[..start]);
        let after_open = &remaining[start + open_char.len_utf8()..];
        let Some(end) = after_open.find(close_char) else {
            result.push_str(&remaining[start..]);
            return result;
        };
        let inner = &after_open[..end];
        if !is_noise_tag(inner) {
            result.push(open_char);
            result.push_str(inner);
            result.push(close_char);
        }
        remaining = &after_open[end + close_char.len_utf8()..];
    }
    result.push_str(remaining);
    result
}

fn strip_noise_suffixes(input: &str) -> String {
    let mut current = input.trim();
    while let Some(pos) = current.rfind(" - ") {
        let suffix = current[pos + 3..].trim();
        if !is_noise_tag(suffix) {
            break;
        }
        current = current[..pos].trim();
    }
    current.to_string()
}

/// Sanitizes a track title by stripping noise, version qualifiers, and featuring clauses.
pub fn clean_track_title(raw: &str) -> String {
    let normalized = normalize_smart_punctuation_str(raw);
    let without_feat = strip_featured_segments(&normalized);
    let without_parens = strip_noise_parentheticals(&without_feat);
    let without_suffixes = strip_noise_suffixes(&without_parens);
    collapse_whitespace(&without_suffixes)
}

fn fold_latin_diacritic(c: char) -> Option<&'static str> {
    match c {
        '\u{00E0}'..='\u{00E5}' | '\u{0100}'..='\u{0105}' => Some("a"),
        '\u{00E8}'..='\u{00EB}' | '\u{0112}'..='\u{011B}' => Some("e"),
        '\u{00EC}'..='\u{00EF}' | '\u{0128}'..='\u{0133}' => Some("i"),
        '\u{00F2}'..='\u{00F6}' | '\u{00F8}' | '\u{014C}'..='\u{0153}' => Some("o"),
        '\u{00F9}'..='\u{00FC}' | '\u{0168}'..='\u{0173}' => Some("u"),
        '\u{00F1}' | '\u{0143}'..='\u{0148}' => Some("n"),
        '\u{00E7}' | '\u{0106}'..='\u{010D}' => Some("c"),
        '\u{00FD}' | '\u{00FF}' => Some("y"),
        '\u{00DF}' => Some("ss"),
        '\u{00E6}' => Some("ae"),
        '\u{0160}' | '\u{0161}' => Some("s"),
        '\u{017D}' | '\u{017E}' => Some("z"),
        _ => None,
    }
}

fn append_normalized_char(buf: &mut String, ch: char) {
    if let Some(folded) = fold_latin_diacritic(ch) {
        buf.push_str(folded);
    } else if ch.is_alphanumeric() {
        buf.push(ch);
    } else {
        buf.push(' ');
    }
}

/// Normalizes a track title for fuzzy matching.
///
/// Cleans version and remaster tags via [`clean_track_title`], lowercases, folds
/// common Latin diacritics to ASCII, replaces non-alphanumeric characters with spaces,
/// collapses whitespace, and trims. Non-Latin scripts (e.g. CJK, Cyrillic) pass through
/// unchanged.
pub fn normalize_for_match(raw: &str) -> String {
    let cleaned = clean_track_title(raw);
    let mut normalized = String::with_capacity(cleaned.len());
    for ch in cleaned.to_lowercase().chars() {
        append_normalized_char(&mut normalized, ch);
    }
    collapse_whitespace(&normalized)
}

/// Returns sorted, deduplicated normalized tokens extracted from a string.
pub fn token_set(raw: &str) -> Vec<String> {
    let normalized = normalize_for_match(raw);
    let mut tokens: Vec<String> = normalized.split_whitespace().map(|s| s.to_string()).collect();
    tokens.sort();
    tokens.dedup();
    tokens
}

fn update_levenshtein_row(curr: &mut [usize], prev: &[usize], ca: char, b: &[char]) {
    for (j, &cb) in b.iter().enumerate() {
        let cost = if ca == cb { 0 } else { 1 };
        let ins = curr[j] + 1;
        let del = prev[j + 1] + 1;
        let sub = prev[j] + cost;
        curr[j + 1] = ins.min(del).min(sub);
    }
}

fn compute_levenshtein_distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr: Vec<usize> = vec![0; b.len() + 1];

    for (i, &ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        update_levenshtein_row(&mut curr, &prev, ca, b);
        prev.copy_from_slice(&curr);
    }
    prev[b.len()]
}

/// Computes the Levenshtein edit distance between two strings.
pub fn levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    if a_chars.is_empty() {
        return b_chars.len();
    }
    if b_chars.is_empty() {
        return a_chars.len();
    }
    compute_levenshtein_distance(&a_chars, &b_chars)
}

/// Computes normalized character similarity in `0.0..=1.0` between two strings.
pub fn similarity(a: &str, b: &str) -> f32 {
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    let max_len = a_len.max(b_len);
    if max_len == 0 {
        return 1.0;
    }
    let dist = levenshtein(a, b);
    1.0 - (dist as f32 / max_len as f32)
}

fn token_jaccard_similarity(a: &str, b: &str) -> f32 {
    let tokens_a = token_set(a);
    let tokens_b = token_set(b);
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }
    let intersection = tokens_a.iter().filter(|t| tokens_b.contains(t)).count();
    let union = tokens_a.len() + tokens_b.len() - intersection;
    if union == 0 {
        return 0.0;
    }
    intersection as f32 / union as f32
}

/// Computes combined fuzzy title similarity in `0.0..=1.0`.
pub fn title_similarity(a: &str, b: &str) -> f32 {
    if a.trim().is_empty() || b.trim().is_empty() {
        return 0.0;
    }
    let norm_a = normalize_for_match(a);
    let norm_b = normalize_for_match(b);
    if norm_a.is_empty() || norm_b.is_empty() {
        return 0.0;
    }
    let str_sim = similarity(&norm_a, &norm_b);
    let jaccard = token_jaccard_similarity(a, b);
    ((str_sim + jaccard) / 2.0).clamp(0.0, 1.0)
}

fn score_duration_delta(delta: u32) -> f32 {
    if delta <= DURATION_EXACT_SECS {
        DURATION_EXACT_BONUS
    } else if delta <= DURATION_TOLERANCE_SECS {
        0.0
    } else if delta <= DURATION_SUSPECT_SECS {
        DURATION_SUSPECT_PENALTY
    } else if delta <= DURATION_WRONG_EDIT_SECS {
        DURATION_WRONG_EDIT_PENALTY
    } else {
        DURATION_VIDEO_PENALTY
    }
}

/// Scores the duration delta between candidate and source track.
pub fn duration_score(candidate_secs: Option<u32>, source_secs: Option<u32>) -> f32 {
    let (Some(cand), Some(src)) = (candidate_secs, source_secs) else {
        return 0.0;
    };
    let delta = cand.abs_diff(src);
    score_duration_delta(delta)
}

fn strip_topic_suffix(channel: &str) -> String {
    let trimmed = channel.trim();
    if let Some(stripped) = trimmed.strip_suffix("- Topic") {
        stripped.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_primary_artist_match(source_artist: &str, channel_tokens: &[String]) -> bool {
    let source_tokens = token_set(source_artist);
    !source_tokens.is_empty() && source_tokens.iter().all(|t| channel_tokens.contains(t))
}

fn is_featured_artist_match(featured: Option<&str>, channel_tokens: &[String]) -> bool {
    let Some(feat) = featured else {
        return false;
    };
    let feat_tokens = token_set(feat);
    !feat_tokens.is_empty() && feat_tokens.iter().any(|t| channel_tokens.contains(t))
}

/// Scores candidate channel against source artist and optional featured artist.
pub fn artist_score(source_artist: &str, candidate_channel: &str, featured: Option<&str>) -> f32 {
    let channel_clean = strip_topic_suffix(candidate_channel);
    let channel_tokens = token_set(&channel_clean);
    if channel_tokens.is_empty() {
        return 0.0;
    }
    if is_primary_artist_match(source_artist, &channel_tokens) {
        return WEIGHT_ARTIST_PRIMARY;
    }
    if is_featured_artist_match(featured, &channel_tokens) {
        return WEIGHT_ARTIST_SECONDARY;
    }
    0.0
}

fn topic_channel_bonus(channel: &str) -> f32 {
    if channel.trim().ends_with("- Topic") { PRISM_TOPIC_BONUS } else { 0.0 }
}

fn video_type_adjustment(video_type: MusicVideoType) -> f32 {
    match video_type {
        MusicVideoType::Atv => PRISM_ATV_BONUS,
        MusicVideoType::Omv => PRISM_OMV_PENALTY,
        MusicVideoType::Ugc => PRISM_UGC_PENALTY,
        MusicVideoType::Other => 0.0,
    }
}

fn has_keyword(norm_text: &str, kw: &str) -> bool {
    if kw.contains(' ') {
        norm_text.contains(kw)
    } else {
        norm_text.split_whitespace().any(|word| word == kw)
    }
}

fn candidate_keyword_penalty(source_title: &str, candidate_title: &str) -> f32 {
    const PENALTY_KEYWORDS: &[&str] = &["official video", "music video", "cover", "reaction"];
    let norm_source = normalize_for_match(source_title);
    let norm_candidate = normalize_for_match(candidate_title);
    let mut penalty = 0.0;
    for &kw in PENALTY_KEYWORDS {
        if has_keyword(&norm_candidate, kw) && !has_keyword(&norm_source, kw) {
            penalty += PRISM_KEYWORD_PENALTY;
        }
    }
    penalty
}

/// Evaluates YouTube Music metadata (channel, video type, keywords) for video penalties.
pub fn prism_video_penalty(source_title: &str, candidate: &MatchCandidate) -> f32 {
    let channel_bonus = topic_channel_bonus(&candidate.channel);
    let video_type_delta = video_type_adjustment(candidate.video_type);
    let keyword_penalties = candidate_keyword_penalty(source_title, &candidate.title);
    let total = channel_bonus + video_type_delta + keyword_penalties;
    total.clamp(-0.50, 0.15)
}

/// Computes multi-factor match score breakdown for a candidate against a source track.
pub fn score_candidate(source: &SpotifyTrack, candidate: &MatchCandidate) -> MatchScore {
    let title_sim = title_similarity(&source.title, &candidate.title);
    let title = WEIGHT_TITLE * title_sim;

    let (_, featured) = split_featured(&source.title);
    let artist = artist_score(&source.artist, &candidate.channel, featured.as_deref());

    let source_secs =
        if source.duration_ms == 0 { None } else { Some((source.duration_ms + 500) / 1000) };
    let duration = duration_score(candidate.duration_secs, source_secs);

    let prism = prism_video_penalty(&source.title, candidate);
    let total = (title + artist + duration + prism).clamp(0.0, 1.0);

    MatchScore { title, artist, duration, prism, total }
}

/// Derives categorical confidence for a candidate against a source track.
pub fn confidence_for(source: &SpotifyTrack, candidate: &MatchCandidate) -> MatchConfidence {
    MatchConfidence::from_score(score_candidate(source, candidate).total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_track_title_matrix() {
        let cases = [
            ("Song (2011 Remaster)", "Song"),
            ("Song - Remastered 2020", "Song"),
            ("Song [Remaster]", "Song"),
            ("Song - 2015 Mix", "Song"),
            ("Song (Deluxe Edition)", "Song"),
            ("Song [Bonus Track]", "Song"),
            ("Song (10th Anniversary)", "Song"),
            ("Song (feat. Drake)", "Song"),
            ("Song (ft. Future)", "Song"),
            ("Song featuring Someone", "Song"),
            ("Song (Live)", "Song (Live)"),
            ("Song (Acoustic)", "Song (Acoustic)"),
            ("Song - Instrumental", "Song - Instrumental"),
            ("Song (Radio Edit)", "Song (Radio Edit)"),
        ];

        for (raw, expected) in cases {
            let cleaned = clean_track_title(raw);
            assert_eq!(cleaned, expected, "failed cleaning for: {raw}");
            assert_eq!(clean_track_title(&cleaned), cleaned, "idempotency failed for: {raw}");
        }
    }

    #[test]
    fn clean_track_title_smart_punctuation() {
        let raw = "\u{201C}Song\u{201D} \u{2018}Special\u{2019} \u{2013} Remastered 2020";
        assert_eq!(clean_track_title(raw), "\"Song\" 'Special'");

        let em_dash = "Song \u{2014} 2011 Remaster";
        assert_eq!(clean_track_title(em_dash), "Song");
    }

    #[test]
    fn normalize_diacritics_and_cjk() {
        assert_eq!(normalize_for_match("Canci\u{00F3}n"), "cancion");
        assert_eq!(normalize_for_match("\u{00C9}xito"), "exito");
        assert_eq!(normalize_for_match("\u{00FC}ber"), "uber");

        let cjk = "\u{591C}\u{306B}\u{99C4}\u{3051}\u{308B}";
        assert_eq!(normalize_for_match(cjk), cjk);
    }

    #[test]
    fn split_featured_cases() {
        let cases = [
            ("Song (feat. Drake)", ("Song", Some("Drake"))),
            ("Song (ft. Future)", ("Song", Some("Future"))),
            ("Song featuring Someone", ("Song", Some("Someone"))),
            ("Song", ("Song", None)),
            ("Song (Live)", ("Song (Live)", None)),
        ];
        for (raw, (expected_title, expected_feat)) in cases {
            let (title, feat) = split_featured(raw);
            assert_eq!(title, expected_title, "failed title for: {raw}");
            assert_eq!(feat.as_deref(), expected_feat, "failed feat for: {raw}");
        }
    }

    #[test]
    fn similarity_and_token_invariance() {
        assert_eq!(similarity("", ""), 1.0);
        assert_eq!(similarity("hello", "hello"), 1.0);
        assert_eq!(similarity("abc", "xyz"), 0.0);

        assert_eq!(title_similarity("", ""), 0.0);
        assert_eq!(title_similarity("Song", ""), 0.0);
        assert_eq!(title_similarity("", "Song"), 0.0);
        assert_eq!(title_similarity("Bohemian Rhapsody", "Bohemian Rhapsody"), 1.0);
        assert_eq!(title_similarity("abc", "xyz"), 0.0);

        let t1 = token_set("World Hello");
        let t2 = token_set("Hello World");
        assert_eq!(t1, t2);

        let sim_order1 = title_similarity("Hello World", "World Hello");
        assert!(sim_order1 > 0.60, "expected high similarity for reordered tokens: {sim_order1}");
    }

    #[test]
    fn duration_score_boundaries() {
        let cases = [
            (Some(200), DURATION_EXACT_BONUS),
            (Some(202), DURATION_EXACT_BONUS),
            (Some(203), 0.0),
            (Some(205), 0.0),
            (Some(206), DURATION_SUSPECT_PENALTY),
            (Some(215), DURATION_SUSPECT_PENALTY),
            (Some(216), DURATION_WRONG_EDIT_PENALTY),
            (Some(230), DURATION_WRONG_EDIT_PENALTY),
            (Some(231), DURATION_VIDEO_PENALTY),
            (None, 0.0),
        ];
        for (cand, expected) in cases {
            assert_eq!(duration_score(cand, Some(200)), expected);
        }
        assert_eq!(duration_score(Some(200), None), 0.0);
        assert_eq!(duration_score(None, None), 0.0);
    }

    #[test]
    fn artist_score_variations() {
        assert_eq!(artist_score("Queen", "Queen", None), WEIGHT_ARTIST_PRIMARY);
        assert_eq!(artist_score("Queen", "Queen - Topic", None), WEIGHT_ARTIST_PRIMARY);
        assert_eq!(
            artist_score("Simon & Garfunkel", "Simon & Garfunkel", None),
            WEIGHT_ARTIST_PRIMARY
        );
        assert_eq!(artist_score("Future", "Drake", Some("Drake")), WEIGHT_ARTIST_SECONDARY);
        assert_eq!(artist_score("Queen", "The Beatles", None), 0.0);
    }

    #[test]
    fn prism_penalties_and_bonuses() {
        let cases = [
            (
                MatchCandidate::new("Song", "Queen - Topic", Some(200), MusicVideoType::Other),
                PRISM_TOPIC_BONUS,
            ),
            (MatchCandidate::new("Song", "Queen", Some(200), MusicVideoType::Atv), PRISM_ATV_BONUS),
            (
                MatchCandidate::new("Song", "Queen", Some(200), MusicVideoType::Omv),
                PRISM_OMV_PENALTY,
            ),
            (
                MatchCandidate::new("Song", "Queen", Some(200), MusicVideoType::Ugc),
                PRISM_UGC_PENALTY,
            ),
            (
                MatchCandidate::new(
                    "Song (Official Video)",
                    "Queen",
                    Some(200),
                    MusicVideoType::Other,
                ),
                PRISM_KEYWORD_PENALTY,
            ),
            (MatchCandidate::new("Song", "Queen - Topic", Some(200), MusicVideoType::Atv), 0.15),
            (
                MatchCandidate::new(
                    "Song (Official Video) (Music Video) (Reaction)",
                    "Random",
                    Some(200),
                    MusicVideoType::Ugc,
                ),
                -0.50,
            ),
        ];
        for (cand, expected) in cases {
            assert_eq!(prism_video_penalty("Song", &cand), expected);
        }
        let cand_cover =
            MatchCandidate::new("Song (Cover)", "Queen", Some(200), MusicVideoType::Other);
        assert_eq!(prism_video_penalty("Song (Cover)", &cand_cover), 0.0);
    }

    #[test]
    fn score_candidate_and_confidence() {
        let source = SpotifyTrack {
            title: "Bohemian Rhapsody".to_string(),
            artist: "Queen".to_string(),
            album: None,
            duration_ms: 354_000,
            isrc: None,
        };

        let high = MatchCandidate::new(
            "Bohemian Rhapsody",
            "Queen - Topic",
            Some(354),
            MusicVideoType::Atv,
        );
        assert!(score_candidate(&source, &high).total >= 0.85);
        assert_eq!(confidence_for(&source, &high), MatchConfidence::High);

        let medium =
            MatchCandidate::new("Bohemian Rhapsody", "Queen", Some(350), MusicVideoType::Other);
        let med_score = score_candidate(&source, &medium).total;
        assert!((0.60..0.85).contains(&med_score));
        assert_eq!(confidence_for(&source, &medium), MatchConfidence::Medium);

        let low = MatchCandidate::new("Different", "Other", Some(120), MusicVideoType::Other);
        assert!(score_candidate(&source, &low).total < 0.60);
        assert_eq!(confidence_for(&source, &low), MatchConfidence::Low);

        let omv = MatchCandidate::new("Bohemian Rhapsody", "Queen", Some(354), MusicVideoType::Omv);
        assert!(score_candidate(&source, &omv).total < 0.85);
        assert_ne!(confidence_for(&source, &omv), MatchConfidence::High);
    }

    #[test]
    fn serde_music_video_type() {
        assert_eq!(serde_json::to_string(&MusicVideoType::Atv).unwrap(), "\"ATV\"");
        assert_eq!(serde_json::to_string(&MusicVideoType::Omv).unwrap(), "\"OMV\"");
        assert_eq!(serde_json::to_string(&MusicVideoType::Ugc).unwrap(), "\"UGC\"");
        assert_eq!(serde_json::to_string(&MusicVideoType::Other).unwrap(), "\"OTHER\"");

        let de: MusicVideoType = serde_json::from_str("\"ATV\"").unwrap();
        assert_eq!(de, MusicVideoType::Atv);
    }
}
