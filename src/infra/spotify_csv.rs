//! File ingestion adapters for Exportify CSV and Spotify JSON backups.
//!
//! Both adapters are pure and allocation-light.
//!
//! Provenance: Test fixture `exportify_playlist.csv` is a real Exportify
//! 19-column export taken on 2026-10-31, trimmed to three tracks.

use std::path::Path;

use anyhow::Context;
use serde_json::Value;

use crate::domain::{SpotifyPlaylist, SpotifyTrack};

pub const REQUIRED_CSV_COLUMNS: [&str; 4] =
    ["Track Name", "Artist Name(s)", "Album Name", "Track Duration (ms)"];

const ISRC_COLUMN: &str = "ISRC";
const TRACK_URI_COLUMN: &str = "Track URI";

fn consume_newline(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    if chars.peek() == Some(&'\r') {
        chars.next();
    }
    if chars.peek() == Some(&'\n') {
        chars.next();
    }
}

fn consume_separator(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> anyhow::Result<bool> {
    match chars.peek() {
        Some(',') => {
            chars.next();
            Ok(true)
        }
        Some('\r' | '\n') => {
            consume_newline(chars);
            Ok(false)
        }
        None => Ok(false),
        Some(c) => anyhow::bail!("unexpected character '{c}' following CSV field"),
    }
}

fn parse_unquoted_field(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut field = String::new();
    while let Some(&c) = chars.peek() {
        if c == ',' || c == '\r' || c == '\n' {
            break;
        }
        field.push(c);
        chars.next();
    }
    field
}

fn parse_quoted_field(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> anyhow::Result<String> {
    chars.next();
    let mut field = String::new();
    while let Some(c) = chars.next() {
        if c == '"' {
            if chars.peek() == Some(&'"') {
                chars.next();
                field.push('"');
            } else {
                return Ok(field);
            }
        } else {
            field.push(c);
        }
    }
    anyhow::bail!("unterminated quote in CSV field")
}

fn parse_field(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> anyhow::Result<String> {
    if chars.peek() == Some(&'"') {
        parse_quoted_field(chars)
    } else {
        Ok(parse_unquoted_field(chars))
    }
}

fn parse_record(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> anyhow::Result<Vec<String>> {
    let mut fields = Vec::new();
    loop {
        let field = parse_field(chars)?;
        fields.push(field);
        if !consume_separator(chars)? {
            break;
        }
    }
    Ok(fields)
}

/// Parses an RFC 4180 CSV document into records and fields.
pub fn parse_csv_records(input: &str) -> anyhow::Result<Vec<Vec<String>>> {
    let input = input.strip_prefix('\u{FEFF}').unwrap_or(input);
    let mut chars = input.chars().peekable();
    let mut records = Vec::new();
    while chars.peek().is_some() {
        let record = parse_record(&mut chars)?;
        records.push(record);
    }
    Ok(records)
}

struct CsvColIndices {
    track_name: usize,
    artist_name: usize,
    album_name: usize,
    duration_ms: usize,
    isrc: Option<usize>,
}

fn find_column_index(headers: &[String], target: &str) -> Option<usize> {
    headers.iter().position(|h| h.trim().eq_ignore_ascii_case(target))
}

fn validate_and_map_headers(headers: &[String]) -> anyhow::Result<CsvColIndices> {
    let mut missing = Vec::new();
    if find_column_index(headers, TRACK_URI_COLUMN).is_none() {
        missing.push(TRACK_URI_COLUMN);
    }
    let mut indices = [0; REQUIRED_CSV_COLUMNS.len()];
    for (i, &col) in REQUIRED_CSV_COLUMNS.iter().enumerate() {
        match find_column_index(headers, col) {
            Some(idx) => indices[i] = idx,
            None => missing.push(col),
        }
    }
    if !missing.is_empty() {
        anyhow::bail!("CSV header missing required column(s): {}", missing.join(", "));
    }
    let isrc = find_column_index(headers, ISRC_COLUMN);
    Ok(CsvColIndices {
        track_name: indices[0],
        artist_name: indices[1],
        album_name: indices[2],
        duration_ms: indices[3],
        isrc,
    })
}

fn is_row_blank(row: &[String]) -> bool {
    row.iter().all(|cell| cell.trim().is_empty())
}

fn parse_duration_cell(cell: Option<&String>) -> u32 {
    let raw = cell.map(|s| s.trim()).unwrap_or("");
    raw.parse::<u32>().unwrap_or(0)
}

fn parse_optional_cell(cell: Option<&String>) -> Option<String> {
    let trimmed = cell.map(|s| s.trim()).unwrap_or("");
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

fn parse_exportify_row(row: &[String], cols: &CsvColIndices) -> Option<SpotifyTrack> {
    if is_row_blank(row) {
        return None;
    }
    let title = row.get(cols.track_name).map(|s| s.trim()).unwrap_or("");
    let artist = row.get(cols.artist_name).map(|s| s.trim()).unwrap_or("");
    let album = parse_optional_cell(row.get(cols.album_name));
    let duration_ms = parse_duration_cell(row.get(cols.duration_ms));
    let isrc = cols.isrc.and_then(|idx| parse_optional_cell(row.get(idx)));

    Some(SpotifyTrack {
        title: title.to_owned(),
        artist: artist.to_owned(),
        album,
        duration_ms,
        isrc,
    })
}

/// Parses an Exportify CSV export into domain [`SpotifyTrack`] records.
pub fn parse_exportify_csv(input: &str) -> anyhow::Result<Vec<SpotifyTrack>> {
    let records = parse_csv_records(input)?;
    let (header, rows) =
        records.split_first().context("CSV input is empty, expected header row")?;
    let cols = validate_and_map_headers(header)?;
    Ok(rows.iter().filter_map(|row| parse_exportify_row(row, &cols)).collect())
}

fn parse_backup_track(val: &Value) -> SpotifyTrack {
    let title = val
        .get("name")
        .or_else(|| val.get("title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let artist = val
        .get("artist")
        .or_else(|| val.get("artists"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let album = val
        .get("album")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let duration_ms = val.get("duration_ms").and_then(Value::as_u64).unwrap_or(0) as u32;
    let isrc = val
        .get("isrc")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    SpotifyTrack { title, artist, album, duration_ms, isrc }
}

/// Parses a Spotify JSON backup payload into a [`SpotifyPlaylist`].
pub fn parse_backup_json(input: &str) -> anyhow::Result<SpotifyPlaylist> {
    let root: Value = serde_json::from_str(input).context("failed to parse Spotify backup JSON")?;
    let title = root
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .context("missing or blank 'name' in Spotify backup JSON")?;
    let raw_tracks = root
        .get("tracks")
        .and_then(Value::as_array)
        .context("missing or invalid 'tracks' array in Spotify backup JSON")?;

    let id = root.get("id").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_owned();
    let author = root
        .get("author")
        .or_else(|| root.get("owner"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let artwork = root
        .get("artwork")
        .or_else(|| root.get("image"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let tracks = raw_tracks.iter().map(parse_backup_track).collect();

    Ok(SpotifyPlaylist { id, title: title.to_owned(), author, artwork, tracks })
}

fn playlist_from_csv(content: &str, stem: &str) -> anyhow::Result<SpotifyPlaylist> {
    let tracks = parse_exportify_csv(content)?;
    Ok(SpotifyPlaylist {
        id: stem.to_owned(),
        title: stem.to_owned(),
        author: String::new(),
        artwork: None,
        tracks,
    })
}

fn playlist_from_json(content: &str, stem: &str) -> anyhow::Result<SpotifyPlaylist> {
    let mut playlist = parse_backup_json(content)?;
    if playlist.id.is_empty() {
        playlist.id = stem.to_owned();
    }
    Ok(playlist)
}

/// Loads a Spotify playlist from a file, supporting Exportify CSV and Spotify JSON backups.
pub fn load_playlist_file(path: &Path) -> anyhow::Result<SpotifyPlaylist> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read playlist file at {}", path.display()))?;
    let stem = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("import");
    let is_json = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
    if is_json { playlist_from_json(&content, stem) } else { playlist_from_csv(&content, stem) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_CSV: &str = include_str!("../../tests/fixtures/exportify_playlist.csv");

    #[test]
    fn parse_fixture_csv() {
        let tracks = parse_exportify_csv(FIXTURE_CSV).expect("parses fixture CSV");
        assert_eq!(tracks.len(), 3);

        assert_eq!(tracks[0].title, "Loser");
        assert_eq!(tracks[0].artist, "Tame Impala");
        assert_eq!(tracks[0].album.as_deref(), Some("Currents"));
        assert_eq!(tracks[0].duration_ms, 223069);
        assert_eq!(tracks[0].isrc.as_deref(), Some("AUUM71500945"));

        assert_eq!(tracks[1].title, "Nonstop");
        assert_eq!(tracks[1].artist, "Drake");
        assert_eq!(tracks[1].album.as_deref(), Some("Scorpion"));
        assert_eq!(tracks[1].duration_ms, 238613);
        assert_eq!(tracks[1].isrc.as_deref(), Some("USCM51800145"));

        assert_eq!(tracks[2].title, "Never, \"Really\" Gonna");
        assert_eq!(tracks[2].artist, "A, B");
        assert_eq!(tracks[2].album.as_deref(), Some("Demo Album"));
        assert_eq!(tracks[2].duration_ms, 205000);
        assert_eq!(tracks[2].isrc, None);
    }

    #[test]
    fn header_validation_names_missing_columns() {
        let missing_title =
            "Track URI,Artist Name(s),Album Name,Track Duration (ms)\nuri,A,Alb,100";
        let err = parse_exportify_csv(missing_title).expect_err("missing Track Name must error");
        let msg = format!("{err:#}");
        assert!(msg.contains("Track Name"), "error should name missing column: {msg}");

        let empty = "";
        assert!(parse_exportify_csv(empty).is_err());
    }

    #[test]
    fn csv_without_isrc_column_parses_with_none() {
        let missing_isrc =
            "Track URI,Track Name,Artist Name(s),Album Name,Track Duration (ms)\nuri,T,A,Alb,100";
        let tracks = parse_exportify_csv(missing_isrc).expect("missing ISRC column must succeed");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "T");
        assert_eq!(tracks[0].artist, "A");
        assert_eq!(tracks[0].album.as_deref(), Some("Alb"));
        assert_eq!(tracks[0].duration_ms, 100);
        assert_eq!(tracks[0].isrc, None);
    }

    #[test]
    fn bom_prefixed_csv_parses() {
        let bom_csv = concat!(
            "\u{FEFF}Track URI,Track Name,Artist Name(s),Album Name,Track Duration (ms),ISRC\n",
            "uri,T,A,Alb,100,US1234567890"
        );
        let tracks = parse_exportify_csv(bom_csv).expect("BOM-prefixed CSV parses");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "T");
        assert_eq!(tracks[0].isrc.as_deref(), Some("US1234567890"));
    }

    #[test]
    fn blank_lines_and_trailing_newline_produce_no_empty_track() {
        let csv = "Track URI,Track Name,Artist Name(s),Album Name,Track Duration (ms),ISRC\n\
                   \n\
                   sp:1,Song 1,Artist 1,Alb 1,12345,ISRC1\n\
                   \n\
                   sp:2,Song 2,Artist 2,Alb 2,67890,ISRC2\n\
                   \n";
        let tracks = parse_exportify_csv(csv).expect("skips blank lines");
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "Song 1");
        assert_eq!(tracks[1].title, "Song 2");
    }

    #[test]
    fn inline_csv_rfc4180_edge_cases() {
        let csv_with_newline = "\"a\nb\",c\n";
        let records = parse_csv_records(csv_with_newline).expect("parses embedded newline");
        assert_eq!(records, vec![vec!["a\nb", "c"]]);

        let csv_crlf = "a,b\r\nc,d\r\n";
        let records_crlf = parse_csv_records(csv_crlf).expect("parses CRLF line endings");
        assert_eq!(records_crlf, vec![vec!["a", "b"], vec!["c", "d"]]);

        let unterminated = "a,\"unterminated,c";
        assert!(parse_csv_records(unterminated).is_err());
    }

    #[test]
    fn parse_backup_json_full_and_minimal_payloads() {
        let full = r#"{"id":"sp_full","name":"Full List","author":"Curator","artwork":"https://art.test/c.jpg","tracks":[{"name":"Song 1","artist":"Artist 1","album":"Album 1","duration_ms":180000,"isrc":"US1234567890"}]}"#;
        let playlist = parse_backup_json(full).expect("parses full backup");
        assert_eq!(playlist.id, "sp_full");
        assert_eq!(playlist.title, "Full List");
        assert_eq!(playlist.author, "Curator");
        assert_eq!(playlist.artwork.as_deref(), Some("https://art.test/c.jpg"));
        assert_eq!(playlist.tracks.len(), 1);
        assert_eq!(playlist.tracks[0].title, "Song 1");
        assert_eq!(playlist.tracks[0].artist, "Artist 1");
        assert_eq!(playlist.tracks[0].album.as_deref(), Some("Album 1"));
        assert_eq!(playlist.tracks[0].duration_ms, 180000);
        assert_eq!(playlist.tracks[0].isrc.as_deref(), Some("US1234567890"));

        let minimal = r#"{"name":"Minimal List","tracks":[{"name":"Only Name"}]}"#;
        let min_pl = parse_backup_json(minimal).expect("parses minimal backup");
        assert_eq!(min_pl.title, "Minimal List");
        assert_eq!(min_pl.tracks.len(), 1);
        assert_eq!(min_pl.tracks[0].title, "Only Name");
        assert_eq!(min_pl.tracks[0].artist, "");
        assert_eq!(min_pl.tracks[0].album, None);
        assert_eq!(min_pl.tracks[0].duration_ms, 0);
        assert_eq!(min_pl.tracks[0].isrc, None);

        let missing_name = r#"{"tracks":[]}"#;
        let err_name = parse_backup_json(missing_name).expect_err("missing name must fail");
        assert!(format!("{err_name:#}").contains("name"));

        let missing_tracks = r#"{"name":"No Tracks"}"#;
        let err_tracks = parse_backup_json(missing_tracks).expect_err("missing tracks must fail");
        assert!(format!("{err_tracks:#}").contains("tracks"));
    }

    #[test]
    fn parse_csv_records_escaping_rules() {
        assert_eq!(parse_csv_records("").unwrap(), Vec::<Vec<String>>::new());
        assert_eq!(parse_csv_records("\"\"\"\"").unwrap(), vec![vec!["\"".to_string()]]);
        assert_eq!(
            parse_csv_records("\"hello \"\"world\"\"\"").unwrap(),
            vec![vec!["hello \"world\"".to_string()]]
        );
        assert_eq!(
            parse_csv_records("\"one,two\",three").unwrap(),
            vec![vec!["one,two".to_string(), "three".to_string()]]
        );
    }

    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "moggyt-{name}-{}-{}",
                std::process::id(),
                fastrand()
            ));
            let _ = std::fs::remove_dir_all(&path);
            let _ = std::fs::create_dir_all(&path);
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fastrand() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
    }

    #[test]
    fn test_load_playlist_file_csv() {
        let temp = TempDir::new("load-csv");
        let csv_path = temp.path().join("my_summer_mix.csv");
        let csv = concat!(
            "Track URI,Track Name,Artist Name(s),Album Name,Track Duration (ms)\n",
            "sp:1,Song One,Artist One,Album One,180000\n"
        );
        std::fs::write(&csv_path, csv).unwrap();

        let playlist = load_playlist_file(&csv_path).expect("loads CSV playlist");
        assert_eq!(playlist.id, "my_summer_mix");
        assert_eq!(playlist.title, "my_summer_mix");
        assert_eq!(playlist.tracks.len(), 1);
        assert_eq!(playlist.tracks[0].title, "Song One");
    }

    #[test]
    fn test_load_playlist_file_json_with_id() {
        let temp = TempDir::new("load-json-id");
        let json_path = temp.path().join("backup_file.json");
        let json = r#"{"id":"sp_custom_id","name":"Workout Hits","tracks":[{"name":"Track 1"}]}"#;
        std::fs::write(&json_path, json).unwrap();

        let playlist = load_playlist_file(&json_path).expect("loads JSON playlist");
        assert_eq!(playlist.id, "sp_custom_id");
        assert_eq!(playlist.title, "Workout Hits");
        assert_eq!(playlist.tracks.len(), 1);
    }

    #[test]
    fn test_load_playlist_file_json_empty_id() {
        let temp = TempDir::new("load-json-empty-id");
        let json_path = temp.path().join("evening_chill.json");
        let json = r#"{"name":"Evening Chill","tracks":[{"name":"Track 1"}]}"#;
        std::fs::write(&json_path, json).unwrap();

        let playlist = load_playlist_file(&json_path).expect("loads JSON playlist with empty id");
        assert_eq!(playlist.id, "evening_chill");
        assert_eq!(playlist.title, "Evening Chill");
        assert_eq!(playlist.tracks.len(), 1);
    }

    #[test]
    fn test_load_playlist_file_missing_fails() {
        let temp = TempDir::new("load-missing");
        let missing_path = temp.path().join("does_not_exist.csv");
        let result = load_playlist_file(&missing_path);
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(err_msg.contains("failed to read playlist file at"));
    }
}
