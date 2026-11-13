//! Playlist header metadata parser: creator, track count, and total duration.
//!
//! Anonymous playlist browse pages nest the header inside the results contents
//! (`musicResponsiveHeaderRenderer` under `twoColumnBrowseResultsRenderer.tabs[0]`), while
//! signed-in editable playlists keep it under
//! `header.musicEditablePlaylistDetailHeaderRenderer`. Both shapes carry the creator in
//! `facepile` (anonymous) or in the last subtitle run (editable), and the aggregate counts in
//! the `secondSubtitle` run list.

use serde_json::Value;

/// Aggregate playlist metadata carried by a detail header node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistMeta {
    /// Playlist creator or channel name.
    pub author: Option<String>,
    /// Total number of tracks the playlist carries, as reported by the server.
    pub track_count: Option<u32>,
    /// Total playback duration in seconds, as reported by the server.
    pub total_duration_secs: Option<u32>,
}

/// Count-unit words the catalog puts in the `secondSubtitle` run list.
const COUNT_WORDS: &[&str] = &["pista", "canci", "titu", "song", "track"];

/// Duration-unit words the catalog puts in the `secondSubtitle` run list.
const TIME_WORDS: &[&str] = &["hora", "hour", "min", "seg", "sec"];

/// Parses playlist header metadata from an optional header node.
pub fn playlist_meta(header: Option<&Value>) -> PlaylistMeta {
    let Some(header) = header else {
        return PlaylistMeta::default();
    };
    PlaylistMeta {
        author: playlist_author(header),
        track_count: playlist_track_count(header),
        total_duration_secs: playlist_total_duration_secs(header),
    }
}

/// Resolves the playlist creator name from a header node.
fn playlist_author(header: &Value) -> Option<String> {
    let facepile = header
        .pointer("/facepile/avatarStackViewModel/text/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if let Some(name) = facepile {
        return Some(name.to_owned());
    }
    // Editable (own) playlist headers list the creator as the last subtitle run; anonymous
    // headers put the year there instead and always carry a facepile.
    let last = header.pointer("/subtitle/runs")?.as_array()?.last()?.get("text")?.as_str()?;
    let trimmed = last.trim();
    if trimmed.is_empty() || is_year(trimmed) { None } else { Some(trimmed.to_owned()) }
}

/// Returns true when the text is a four-digit year.
fn is_year(text: &str) -> bool {
    text.len() == 4 && text.chars().all(|ch| ch.is_ascii_digit())
}

/// Resolves the server-reported track count from the `secondSubtitle` run list.
fn playlist_track_count(header: &Value) -> Option<u32> {
    second_subtitle_runs(header)
        .into_iter()
        .find(|text| contains_any(text, COUNT_WORDS))
        .and_then(|text| leading_number(&text))
}

/// Resolves the server-reported total duration in seconds from the `secondSubtitle` run list.
fn playlist_total_duration_secs(header: &Value) -> Option<u32> {
    second_subtitle_runs(header)
        .into_iter()
        .find(|text| contains_any(text, TIME_WORDS))
        .and_then(|text| duration_secs_from_text(&text))
}

/// Collects the trimmed text of every `secondSubtitle` run.
fn second_subtitle_runs(header: &Value) -> Vec<String> {
    let Some(runs) = header.pointer("/secondSubtitle/runs").and_then(Value::as_array) else {
        return Vec::new();
    };
    runs.iter()
        .filter_map(|run| run.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Returns true when the lowered text contains any of the words.
fn contains_any(text: &str, words: &[&str]) -> bool {
    let lowered = text.to_lowercase();
    words.iter().any(|word| lowered.contains(word))
}

/// Converts a localized duration phrase ("3 hours, 47 minutes") into total seconds.
fn duration_secs_from_text(text: &str) -> Option<u32> {
    let mut parts = DurationParts::default();
    let mut pending: Option<u32> = None;
    for token in duration_tokens(text) {
        if let Some(value) = pending.take() {
            parts.add(&token, value);
            continue;
        }
        pending = leading_number(&token);
    }
    parts.total()
}

/// Splits a duration phrase into lowercased alphanumeric tokens.
fn duration_tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric() && ch != '.' && ch != ',')
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Accumulated hours, minutes, and seconds of a duration phrase.
#[derive(Default)]
struct DurationParts {
    hours: u32,
    minutes: u32,
    seconds: u32,
}

impl DurationParts {
    /// Assigns the amount that precedes a unit word, ignoring unknown units.
    fn add(&mut self, unit: &str, value: u32) {
        if unit.starts_with("hora") || unit.starts_with("hour") {
            self.hours = value;
        } else if unit.starts_with("min") {
            self.minutes = value;
        } else if unit.starts_with("seg") || unit.starts_with("sec") {
            self.seconds = value;
        }
    }

    /// Returns the accumulated total in seconds, or `None` when nothing added up.
    fn total(&self) -> Option<u32> {
        let total = self
            .hours
            .saturating_mul(3600)
            .saturating_add(self.minutes.saturating_mul(60))
            .saturating_add(self.seconds);
        if total == 0 { None } else { Some(total) }
    }
}

/// Parses a leading integer, tolerating thousands separators ("1.234 tracks" -> 1234).
fn leading_number(text: &str) -> Option<u32> {
    let digits: String = text
        .trim()
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ','))
        .filter(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DetailKind;
    use crate::infra::catalog_parse_detail::parse_detail_all;

    fn subtitle_header(runs: &[&str]) -> Value {
        let runs: Vec<Value> =
            runs.iter().map(|text| serde_json::json!({ "text": text })).collect();
        serde_json::json!({ "secondSubtitle": { "runs": runs } })
    }

    #[test]
    fn community_playlist_fixture_yields_header_meta() {
        let raw = include_str!("../../tests/fixtures/catalog_playlist_community_detail.json");
        let payload: Value = serde_json::from_str(raw).expect("valid fixture json");
        let detail = parse_detail_all(&payload, DetailKind::Playlist).expect("parsed detail");
        assert_eq!(detail.kind, DetailKind::Playlist);
        assert_eq!(detail.title, "Community Playlist Title");
        assert_eq!(detail.subtitle, "Community Creator");
        assert_eq!(detail.description.as_deref(), Some("Community Playlist Description"));
        assert_eq!(detail.artwork.as_deref(), Some("https://img.local/community_header.jpg"));
        assert_eq!(detail.track_count, Some(64));
        assert_eq!(detail.total_duration_secs, Some(13620));
        assert_eq!(detail.tracks.len(), 2);
        assert_eq!(detail.tracks[0].title, "Community Track 1");
        assert_eq!(detail.tracks[0].duration_secs, Some(200));
        assert_eq!(detail.tracks[1].title, "Community Track 2");
        assert_eq!(
            detail.tracks[1].artwork.as_deref(),
            Some("https://img.local/community_header.jpg"),
            "track without its own artwork inherits the container artwork"
        );
    }

    #[test]
    fn duration_secs_from_text_handles_spanish_and_english() {
        for (input, expected) in [
            ("3 horas y 47 minutos", Some(13620)),
            ("1 hora y 59 minutos", Some(7140)),
            ("32 horas", Some(115200)),
            ("3 hours, 47 minutes", Some(13620)),
            ("45 minutes", Some(2700)),
            ("2 horas y 5 minutos", Some(7500)),
            ("no duration", None),
            ("", None),
        ] {
            assert_eq!(duration_secs_from_text(input), expected, "failed for: {input}");
        }
    }

    #[test]
    fn track_count_ignores_view_counts_and_reads_thousands() {
        let spanish =
            subtitle_header(&["274 K visualizaciones", "64 pistas", "3 horas y 47 minutos"]);
        assert_eq!(playlist_track_count(&spanish), Some(64));
        let english = subtitle_header(&["1.2M views", "1 track", "4 minutes"]);
        assert_eq!(playlist_track_count(&english), Some(1));
        let thousands = subtitle_header(&["1.234 tracks"]);
        assert_eq!(playlist_track_count(&thousands), Some(1234));
        let views_only = subtitle_header(&["274 K views"]);
        assert_eq!(playlist_track_count(&views_only), None);
    }

    #[test]
    fn author_prefers_facepile_and_skips_the_year_fallback() {
        let facepile = serde_json::json!({
            "facepile": { "avatarStackViewModel": { "text": { "content": "Community Creator" } } },
            "subtitle": { "runs": [{ "text": "Playlist" }, { "text": " \u{2022} " }, { "text": "2026" }] }
        });
        assert_eq!(playlist_author(&facepile).as_deref(), Some("Community Creator"));

        let editable = serde_json::json!({
            "subtitle": {
                "runs": [{ "text": "Playlist" }, { "text": " \u{2022} " }, { "text": "Test Creator" }]
            }
        });
        assert_eq!(playlist_author(&editable).as_deref(), Some("Test Creator"));

        let year_only = serde_json::json!({
            "subtitle": { "runs": [{ "text": "Playlist" }, { "text": "2026" }] }
        });
        assert_eq!(playlist_author(&year_only), None);
        assert_eq!(playlist_meta(None), PlaylistMeta::default());
    }

    #[test]
    fn leading_number_samples() {
        for (input, expected) in [
            ("64 tracks", Some(64)),
            ("1.234 tracks", Some(1234)),
            ("0 pistas", Some(0)),
            ("tracks", None),
            ("  12  ", Some(12)),
        ] {
            assert_eq!(leading_number(input), expected, "failed for: {input}");
        }
    }
}
