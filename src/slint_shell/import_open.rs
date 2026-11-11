//! Derives YouTube Music URLs for import matches and alternative candidates.

pub const YOUTUBE_MUSIC_WATCH_PREFIX: &str = "https://music.youtube.com/watch?v=";

use crate::slint_shell::import_state::ImportRow;

/// Derives a YouTube Music watch URL for a validated video ID.
pub fn youtube_music_url(video_id: &str) -> Option<String> {
    let id = video_id.trim();
    if id.is_empty() || id.len() > 64 {
        return None;
    }
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    Some(format!("{YOUTUBE_MUSIC_WATCH_PREFIX}{id}"))
}

/// Derives a YouTube Music watch URL for the chosen track in an import review row.
pub fn chosen_track_url(row: &ImportRow) -> Option<String> {
    row.chosen.as_ref().and_then(|track| youtube_music_url(&track.video_id))
}

/// Derives a YouTube Music watch URL for a specific candidate in a row's alternatives.
pub fn alternative_track_url(row: &ImportRow, alternative: usize) -> Option<String> {
    row.alternatives.get(alternative).and_then(|track| youtube_music_url(&track.video_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::{LocalTrack, MatchConfidence};

    fn make_test_row(chosen: Option<LocalTrack>, alternatives: Vec<LocalTrack>) -> ImportRow {
        ImportRow {
            index: 0,
            source_title: "Source Title".to_owned(),
            source_artist: "Source Artist".to_owned(),
            chosen,
            confidence: Some(MatchConfidence::High),
            alternatives,
            chosen_alternative: 0,
            reason: None,
            excluded: false,
            source_duration_secs: Some(180),
        }
    }

    #[test]
    fn test_youtube_music_url_valid_and_whitespace_trimmed() {
        let expected = "https://music.youtube.com/watch?v=dQw4w9WgXcQ";
        assert_eq!(youtube_music_url("dQw4w9WgXcQ"), Some(expected.to_owned()));
        assert_eq!(youtube_music_url("  dQw4w9WgXcQ  \t\n"), Some(expected.to_owned()));
    }

    #[test]
    fn test_youtube_music_url_rejected_invalid_inputs() {
        let rejected = ["", " ", "bad id", "a/b", "a?b", "a&b", "a#b", "a%20b", "ni\u{f1}o"];

        for input in rejected {
            assert_eq!(youtube_music_url(input), None, "Expected None for input: {input:?}");
        }

        let long_id = "a".repeat(65);
        assert_eq!(youtube_music_url(&long_id), None, "65-character ID must be rejected");
    }

    #[test]
    fn test_chosen_track_url() {
        let track = LocalTrack::new("dQw4w9WgXcQ", "Title", "Artist", MatchConfidence::High);
        let row_with_chosen = make_test_row(Some(track), Vec::new());
        assert_eq!(
            chosen_track_url(&row_with_chosen),
            Some("https://music.youtube.com/watch?v=dQw4w9WgXcQ".to_owned())
        );

        let row_without_chosen = make_test_row(None, Vec::new());
        assert_eq!(chosen_track_url(&row_without_chosen), None);
    }

    #[test]
    fn test_alternative_track_url() {
        let alt0 = LocalTrack::new("dQw4w9WgXcQ", "Title 1", "Artist 1", MatchConfidence::High);
        let alt1 = LocalTrack::new("alt00000002", "Title 2", "Artist 2", MatchConfidence::Medium);
        let row = make_test_row(None, vec![alt0, alt1]);

        assert_eq!(
            alternative_track_url(&row, 0),
            Some("https://music.youtube.com/watch?v=dQw4w9WgXcQ".to_owned())
        );
        assert_eq!(
            alternative_track_url(&row, 1),
            Some("https://music.youtube.com/watch?v=alt00000002".to_owned())
        );
        assert_eq!(alternative_track_url(&row, 2), None);
        assert_eq!(alternative_track_url(&row, 999), None);
    }
}
