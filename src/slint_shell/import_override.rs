//! Manual track override logic for Spotify playlist import review rows.
//!
//! Hexagonal invariant: pure in-memory review state mutation without I/O or network calls.

use crate::domain::{LocalTrack, MatchConfidence};
use crate::slint_shell::import_state::ImportReview;

/// Returns `true` if `id` is a valid 11-character YouTube video identifier.
///
/// Trims surrounding whitespace, then verifies that the identifier is exactly 11 characters
/// long and contains only ASCII alphanumeric characters, underscores (`_`), or hyphens (`-`).
pub fn is_valid_video_id(id: &str) -> bool {
    let trimmed = id.trim();
    trimmed.len() == 11
        && trimmed.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl ImportReview {
    /// Overrides a review row's match with an explicit YouTube video ID.
    ///
    /// If the video ID is valid and `row` is within range, replaces the chosen track with a new
    /// high-confidence [`LocalTrack`], sets confidence to [`MatchConfidence::High`], and clears
    /// any unresolved reason. Returns `false` without modifying state if `video_id` is invalid
    /// or `row` is out of bounds.
    pub fn apply_custom_video_id(&mut self, row: usize, video_id: &str) -> bool {
        let trimmed = video_id.trim();
        if !is_valid_video_id(trimmed) {
            return false;
        }
        let Some(target) = self.rows.get_mut(row) else {
            return false;
        };
        target.chosen = Some(LocalTrack::new(
            trimmed,
            &target.source_title,
            &target.source_artist,
            MatchConfidence::High,
        ));
        target.confidence = Some(MatchConfidence::High);
        target.reason = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LocalTrack, MatchConfidence, SpotifyPlaylist, SpotifyTrack};
    use crate::slint_shell::import_worker::TrackResolution;

    fn sample_playlist() -> SpotifyPlaylist {
        SpotifyPlaylist {
            id: "pl1".into(),
            title: "Test Playlist".into(),
            author: "Tester".into(),
            artwork: None,
            tracks: vec![
                SpotifyTrack {
                    title: "Track 1".into(),
                    artist: "Artist 1".into(),
                    album: None,
                    duration_ms: 180_000,
                    isrc: None,
                },
                SpotifyTrack {
                    title: "Track 2".into(),
                    artist: "Artist 2".into(),
                    album: None,
                    duration_ms: 200_000,
                    isrc: None,
                },
            ],
        }
    }

    fn sample_review() -> ImportReview {
        let playlist = sample_playlist();
        let t1 = LocalTrack::new("id111111111", "Track 1", "Artist 1", MatchConfidence::Low);
        let alt1 = LocalTrack::new("alt11111111", "Alt 1", "Artist 1", MatchConfidence::Low);
        let resolutions = vec![
            TrackResolution::Matched { track: t1, alternatives: vec![alt1] },
            TrackResolution::Unresolved {
                title: "Track 2".into(),
                artist: "Artist 2".into(),
                reason: "No match found".into(),
            },
        ];
        ImportReview::from_resolutions(&playlist, &resolutions)
    }

    #[test]
    fn test_apply_custom_video_id_valid_on_matched_row() {
        let mut review = sample_review();
        let initial_alts = review.rows[0].alternatives.clone();
        assert!(!initial_alts.is_empty());

        let ok = review.apply_custom_video_id(0, "dQw4w9WgXcQ");
        assert!(ok);

        let row = &review.rows[0];
        assert_eq!(row.confidence, Some(MatchConfidence::High));
        assert_eq!(row.reason, None);
        let chosen = row.chosen.as_ref().expect("chosen track should be set");
        assert_eq!(chosen.video_id, "dQw4w9WgXcQ");
        assert_eq!(chosen.title, "Track 1");
        assert_eq!(chosen.artist, "Artist 1");
        assert_eq!(chosen.confidence, MatchConfidence::High);
        assert_eq!(row.alternatives, initial_alts);
    }

    #[test]
    fn test_apply_custom_video_id_valid_on_unresolved_row() {
        let mut review = sample_review();
        assert_eq!(review.rows[1].chosen, None);
        assert_eq!(review.rows[1].confidence, None);
        assert!(review.rows[1].reason.is_some());

        let ok = review.apply_custom_video_id(1, "  _abc123-XYZ  ");
        assert!(ok);

        let row = &review.rows[1];
        assert_eq!(row.confidence, Some(MatchConfidence::High));
        assert_eq!(row.reason, None);
        let chosen = row.chosen.as_ref().expect("chosen track should be set");
        assert_eq!(chosen.video_id, "_abc123-XYZ");
        assert_eq!(chosen.title, "Track 2");
        assert_eq!(chosen.artist, "Artist 2");
        assert_eq!(chosen.confidence, MatchConfidence::High);
    }

    #[test]
    fn test_apply_custom_video_id_invalid_lengths_rejected_without_mutation() {
        let mut review = sample_review();
        let original_row = review.rows[0].clone();

        for invalid in ["", "   ", "1234567890", "123456789012"] {
            let ok = review.apply_custom_video_id(0, invalid);
            assert!(!ok, "expected '{invalid}' to be rejected");
            assert_eq!(review.rows[0], original_row);
        }
    }

    #[test]
    fn test_apply_custom_video_id_invalid_characters_rejected_without_mutation() {
        let mut review = sample_review();
        let original_row = review.rows[0].clone();

        for invalid in ["dQw4w9WgXc?", "dQw4w9WgXc@", "dQw4w9WgXc ", "dQw4w9WgXc#"] {
            let ok = review.apply_custom_video_id(0, invalid);
            assert!(!ok, "expected '{invalid}' to be rejected");
            assert_eq!(review.rows[0], original_row);
        }
    }

    #[test]
    fn test_apply_custom_video_id_out_of_range_rejected() {
        let mut review = sample_review();
        let ok = review.apply_custom_video_id(999, "dQw4w9WgXcQ");
        assert!(!ok);
    }

    #[test]
    fn test_is_valid_video_id_table() {
        let cases = [
            ("dQw4w9WgXcQ", true),
            ("  dQw4w9WgXcQ  ", true),
            ("12345678901", true),
            ("___________", true),
            ("-----------", true),
            ("a-b_C-d_E-1", true),
            ("", false),
            (" ", false),
            ("1234567890", false),
            ("123456789012", false),
            ("dQw4w9WgXc!", false),
            ("dQw4w9WgXc$", false),
            ("dQw4w9WgXc*", false),
            ("dQw4w9 WgXcQ", false),
        ];

        for (input, expected) in cases {
            assert_eq!(
                is_valid_video_id(input),
                expected,
                "is_valid_video_id('{input}') should be {expected}"
            );
        }
    }
}
