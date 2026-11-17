//! Playlist row column extraction: album and date-added cells.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedDate {
    pub label: String,
    pub epoch_secs: i64,
}

const ALBUM_PAGE_TYPE_PATH: &str = concat!(
    "/navigationEndpoint/browseEndpoint/",
    "browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType",
);

fn column_runs(col: &Value) -> Option<&[Value]> {
    col.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs")
        .or_else(|| col.pointer("/text/runs"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
}

fn joined_run_text(runs: &[Value]) -> Option<String> {
    let mut text = String::new();
    for run in runs {
        if let Some(t) = run.get("text").and_then(Value::as_str) {
            text.push_str(t);
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

fn is_album_run(run: &Value) -> bool {
    run.pointer(ALBUM_PAGE_TYPE_PATH).and_then(Value::as_str) == Some("MUSIC_PAGE_TYPE_ALBUM")
}

fn is_album_column(runs: &[Value]) -> bool {
    runs.iter().any(is_album_run)
}

/// Album cell of a playlist row, resolved from the column that navigates to an album page.
pub fn row_album(renderer: &Value) -> Option<String> {
    let cols = renderer.get("flexColumns")?.as_array()?;
    for col in cols {
        let runs = column_runs(col)?;
        if is_album_column(runs) {
            return joined_run_text(runs);
        }
    }
    None
}

fn has_navigation_endpoint(runs: &[Value]) -> bool {
    runs.iter().any(|run| run.get("navigationEndpoint").is_some())
}

fn display_label(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.get(..6).is_some_and(|prefix| prefix.eq_ignore_ascii_case("added ")) {
        trimmed.get(6..).map(str::trim).unwrap_or("").to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn fold_char(ch: char) -> char {
    match ch {
        '\u{00E1}' | '\u{00E0}' | '\u{00E4}' | '\u{00E2}' => 'a',
        '\u{00C1}' | '\u{00C0}' | '\u{00C4}' | '\u{00C2}' => 'a',
        '\u{00E9}' | '\u{00E8}' | '\u{00EB}' | '\u{00EA}' => 'e',
        '\u{00C9}' | '\u{00C8}' | '\u{00CB}' | '\u{00CA}' => 'e',
        '\u{00ED}' | '\u{00EC}' | '\u{00EF}' | '\u{00EE}' => 'i',
        '\u{00CD}' | '\u{00CC}' | '\u{00CF}' | '\u{00CE}' => 'i',
        '\u{00F3}' | '\u{00F2}' | '\u{00F6}' | '\u{00F4}' => 'o',
        '\u{00D3}' | '\u{00D2}' | '\u{00D6}' | '\u{00D4}' => 'o',
        '\u{00FA}' | '\u{00F9}' | '\u{00FC}' | '\u{00FB}' => 'u',
        '\u{00DA}' | '\u{00D9}' | '\u{00DC}' | '\u{00DB}' => 'u',
        '\u{00F1}' | '\u{00D1}' => 'n',
        other => other,
    }
}

fn normalize_text(text: &str) -> String {
    text.to_lowercase().chars().map(fold_char).collect()
}

fn tokenize(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect()
}

fn unit_seconds(word: &str) -> Option<i64> {
    match word {
        "minute" | "minutes" | "minuto" | "minutos" => Some(60),
        "hour" | "hours" | "hora" | "horas" => Some(3600),
        "day" | "days" | "dia" | "dias" => Some(86_400),
        "week" | "weeks" | "semana" | "semanas" => Some(604_800),
        "month" | "months" | "mes" | "meses" => Some(2_592_000),
        "year" | "years" | "ano" | "anos" => Some(31_536_000),
        _ => None,
    }
}

fn parse_relative_keyword(tokens: &[&str], now_secs: i64) -> Option<i64> {
    for &token in tokens {
        if token == "today" || token == "hoy" {
            return Some(now_secs);
        }
        if token == "yesterday" || token == "ayer" {
            return Some(now_secs - 86_400);
        }
    }
    None
}

fn match_ago_at(tokens: &[&str], i: usize, now_secs: i64) -> Option<i64> {
    if i < 2 || tokens[i] != "ago" {
        return None;
    }
    let unit_secs = unit_seconds(tokens[i - 1])?;
    let n = tokens[i - 2].parse::<i64>().ok()?;
    if n <= 0 {
        return None;
    }
    Some(now_secs - n * unit_secs)
}

fn parse_english_relative(tokens: &[&str], now_secs: i64) -> Option<i64> {
    (0..tokens.len()).find_map(|i| match_ago_at(tokens, i, now_secs))
}

fn parse_spanish_count(token: &str) -> Option<i64> {
    if token == "un" || token == "una" {
        return Some(1);
    }
    let n = token.parse::<i64>().ok()?;
    if n > 0 { Some(n) } else { None }
}

fn match_hace_at(tokens: &[&str], i: usize, now_secs: i64) -> Option<i64> {
    if tokens[i] != "hace" || i + 2 >= tokens.len() {
        return None;
    }
    let n = parse_spanish_count(tokens[i + 1])?;
    let unit_secs = unit_seconds(tokens[i + 2])?;
    Some(now_secs - n * unit_secs)
}

fn parse_spanish_relative(tokens: &[&str], now_secs: i64) -> Option<i64> {
    (0..tokens.len()).find_map(|i| match_hace_at(tokens, i, now_secs))
}

fn parse_relative(tokens: &[&str], now_secs: i64) -> Option<i64> {
    parse_relative_keyword(tokens, now_secs)
        .or_else(|| parse_english_relative(tokens, now_secs))
        .or_else(|| parse_spanish_relative(tokens, now_secs))
}

fn civil_days_to_epoch(year: i32, month: u32, day: u32) -> Option<i64> {
    if year < 1970 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let m_prime = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = (era as i64) * 146_097 + (doe as i64) - 719_468;
    Some(days * 86_400)
}

fn match_iso_word(word: &str) -> Option<i64> {
    let clean = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let mut parts = clean.split('-');
    let y_str = parts.next()?;
    let m_str = parts.next()?;
    let d_str = parts.next()?;
    if parts.next().is_some()
        || y_str.len() != 4
        || !(1..=2).contains(&m_str.len())
        || !(1..=2).contains(&d_str.len())
    {
        return None;
    }
    let y = y_str.parse::<i32>().ok()?;
    let m = m_str.parse::<u32>().ok()?;
    let d = d_str.parse::<u32>().ok()?;
    civil_days_to_epoch(y, m, d)
}

fn parse_iso_date(text: &str) -> Option<i64> {
    text.split_whitespace().find_map(match_iso_word)
}

fn match_slash_word(word: &str) -> Option<i64> {
    let clean = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let mut parts = clean.split('/');
    let d_str = parts.next()?;
    let m_str = parts.next()?;
    let y_str = parts.next()?;
    if parts.next().is_some()
        || y_str.len() != 4
        || !(1..=2).contains(&d_str.len())
        || !(1..=2).contains(&m_str.len())
    {
        return None;
    }
    let d = d_str.parse::<u32>().ok()?;
    let m = m_str.parse::<u32>().ok()?;
    let y = y_str.parse::<i32>().ok()?;
    civil_days_to_epoch(y, m, d)
}

fn parse_slash_date(text: &str) -> Option<i64> {
    if !text.contains('/') {
        return None;
    }
    text.split_whitespace().find_map(match_slash_word)
}

fn month_from_prefix(prefix: &str) -> Option<u32> {
    match prefix {
        "ene" | "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "abr" | "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "ago" | "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dic" | "dec" => Some(12),
        _ => None,
    }
}

fn parse_month_token(token: &str) -> Option<u32> {
    if token.len() < 3 {
        return None;
    }
    month_from_prefix(&token[..3])
}

fn parse_day_token(token: &str) -> Option<u32> {
    if !(1..=2).contains(&token.len()) {
        return None;
    }
    let d = token.parse::<u32>().ok()?;
    if (1..=31).contains(&d) { Some(d) } else { None }
}

fn parse_year_token(token: &str) -> Option<i32> {
    if token.len() != 4 {
        return None;
    }
    let y = token.parse::<i32>().ok()?;
    if y >= 1970 { Some(y) } else { None }
}

fn match_month_name_at(tokens: &[&str], i: usize) -> Option<i64> {
    if i + 2 >= tokens.len() {
        return None;
    }
    if let (Some(m), Some(d), Some(y)) = (
        parse_month_token(tokens[i]),
        parse_day_token(tokens[i + 1]),
        parse_year_token(tokens[i + 2]),
    ) {
        return civil_days_to_epoch(y, m, d);
    }
    if let (Some(d), Some(m), Some(y)) = (
        parse_day_token(tokens[i]),
        parse_month_token(tokens[i + 1]),
        parse_year_token(tokens[i + 2]),
    ) {
        return civil_days_to_epoch(y, m, d);
    }
    None
}

fn parse_month_name_date(tokens: &[&str]) -> Option<i64> {
    let clean: Vec<&str> = tokens.iter().copied().filter(|&t| t != "de" && t != "del").collect();
    (0..clean.len()).find_map(|i| match_month_name_at(&clean, i))
}

fn parse_date(text: &str, now_secs: i64) -> Option<i64> {
    let normalized = normalize_text(text);
    let tokens = tokenize(&normalized);
    if tokens.is_empty() {
        return None;
    }
    if let Some(epoch) = parse_relative(&tokens, now_secs) {
        return Some(epoch);
    }
    if let Some(epoch) = parse_iso_date(&normalized) {
        return Some(epoch);
    }
    if let Some(epoch) = parse_slash_date(&normalized) {
        return Some(epoch);
    }
    parse_month_name_date(&tokens)
}

fn date_column(col: &Value, now_secs: i64) -> Option<AddedDate> {
    let runs = column_runs(col)?;
    if has_navigation_endpoint(runs) {
        return None;
    }
    let raw_text = joined_run_text(runs)?;
    let epoch_secs = parse_date(&raw_text, now_secs)?;
    Some(AddedDate { label: display_label(&raw_text), epoch_secs })
}

/// Date-added cell of a playlist row with its resolved epoch timestamp.
pub fn row_added(renderer: &Value, now_secs: i64) -> Option<AddedDate> {
    renderer.get("flexColumns")?.as_array()?.iter().find_map(|col| date_column(col, now_secs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn row_album_extracts_album_from_flex_columns() {
        let renderer = json!({
            "flexColumns": [
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [{ "text": "Song Title" }]
                        }
                    }
                },
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [
                                {
                                    "text": "Artist Name",
                                    "navigationEndpoint": {
                                        "browseEndpoint": {
                                            "browseEndpointContextSupportedConfigs": {
                                                "browseEndpointContextMusicConfig": {
                                                    "pageType": "MUSIC_PAGE_TYPE_ARTIST"
                                                }
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                },
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [
                                {
                                    "text": "Primer Amor",
                                    "navigationEndpoint": {
                                        "browseEndpoint": {
                                            "browseEndpointContextSupportedConfigs": {
                                                "browseEndpointContextMusicConfig": {
                                                    "pageType": "MUSIC_PAGE_TYPE_ALBUM"
                                                }
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            ]
        });
        assert_eq!(row_album(&renderer), Some("Primer Amor".to_string()));
    }

    #[test]
    fn row_album_ignores_text_only_views_column() {
        let renderer = json!({
            "flexColumns": [
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [{ "text": "15 M reproducciones" }]
                        }
                    }
                }
            ]
        });
        assert_eq!(row_album(&renderer), None);
    }

    #[test]
    fn row_album_ignores_album_text_without_album_page_type() {
        let renderer = json!({
            "flexColumns": [
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [{ "text": "Primer Amor" }]
                        }
                    }
                }
            ]
        });
        assert_eq!(row_album(&renderer), None);
    }

    #[test]
    fn row_added_extracts_label_and_epoch_with_added_prefix() {
        let now = 1_800_000_000;
        let renderer = json!({
            "flexColumns": [
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [{ "text": "Title" }]
                        }
                    }
                },
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [{ "text": "Artist" }]
                        }
                    }
                },
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [{ "text": "Album" }]
                        }
                    }
                },
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [{ "text": "Added 1 week ago" }]
                        }
                    }
                }
            ]
        });
        assert_eq!(
            row_added(&renderer, now),
            Some(AddedDate { label: "1 week ago".to_string(), epoch_secs: now - 604_800 })
        );
    }

    #[test]
    fn row_added_ignores_column_with_navigation_endpoint() {
        let now = 1_800_000_000;
        let renderer = json!({
            "flexColumns": [
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": {
                            "runs": [
                                {
                                    "text": "1 week ago",
                                    "navigationEndpoint": {
                                        "browseEndpoint": {
                                            "browseId": "some_id"
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            ]
        });
        assert_eq!(row_added(&renderer, now), None);
    }

    #[test]
    fn relative_english_date_parsing() {
        let now = 1_800_000_000;
        assert_eq!(parse_date("3 weeks ago", now), Some(now - 3 * 604_800));
        assert_eq!(parse_date("1 day ago", now), Some(now - 86_400));
        assert_eq!(parse_date("2 hours ago", now), Some(now - 2 * 3600));
        assert_eq!(parse_date("today", now), Some(now));
        assert_eq!(parse_date("yesterday", now), Some(now - 86_400));
    }

    #[test]
    fn relative_spanish_date_parsing() {
        let now = 1_800_000_000;
        assert_eq!(parse_date("hace 3 semanas", now), Some(now - 3 * 604_800));
        assert_eq!(parse_date("hace 1 día", now), Some(now - 86_400));
        assert_eq!(parse_date("hoy", now), Some(now));
        assert_eq!(parse_date("ayer", now), Some(now - 86_400));
        assert_eq!(parse_date("Añadido hace 2 días", now), Some(now - 2 * 86_400));
    }

    #[test]
    fn absolute_date_formats_produce_matching_epoch() {
        let now = 1_800_000_000;
        let d1 = parse_date("Oct 16, 2026", now).expect("month day year");
        let d2 = parse_date("16 oct 2026", now).expect("day month year");
        let d3 = parse_date("2026-10-16", now).expect("iso date");
        let d4 = parse_date("16/10/2026", now).expect("slash date");

        assert_eq!(d1, d2);
        assert_eq!(d2, d3);
        assert_eq!(d3, d4);
        assert!(d3 > 1_700_000_000);
        assert!(d3 < 1_900_000_000);
    }

    #[test]
    fn non_dates_return_none() {
        let now = 1_800_000_000;
        assert_eq!(parse_date("15 M reproducciones", now), None);
        assert_eq!(parse_date("", now), None);
        assert_eq!(parse_date("Added", now), None);
    }
}
