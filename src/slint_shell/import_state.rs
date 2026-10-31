//! Review state model for Spotify playlist imports.
//!
//! Hexagonal invariant: pure in-memory view model over import worker resolutions
//! with zero I/O, zero network, zero process spawning, and zero disk writes.

use std::collections::HashSet;

use crate::domain::{
    LocalPlaylist, LocalTrack, MatchConfidence, SpotifyPlaylist, SpotifyTrack, local_playlist_id,
};
use crate::slint_shell::import_worker::TrackResolution;

/// One review row: the Spotify source track and the chosen local match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRow {
    pub index: usize,
    pub source_title: String,
    pub source_artist: String,
    pub chosen: Option<LocalTrack>,
    pub confidence: Option<MatchConfidence>,
    pub alternatives: Vec<LocalTrack>,
    pub chosen_alternative: usize,
    pub reason: Option<String>,
}

/// Tallies shown in the review header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportCounts {
    pub total: usize,
    pub high: usize,
    pub review: usize,
    pub low: usize,
    pub unresolved: usize,
}

/// Review screen state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReview {
    pub playlist_title: String,
    pub playlist_id: String,
    pub rows: Vec<ImportRow>,
    pub selected: usize,
    pub author: String,
    pub artwork: Option<String>,
}

fn map_alternatives(primary: &LocalTrack, candidates: &[LocalTrack]) -> Vec<LocalTrack> {
    let mut seen = HashSet::new();
    seen.insert(primary.video_id.clone());
    let mut alts = vec![primary.clone()];
    for alt in candidates {
        if seen.insert(alt.video_id.clone()) {
            alts.push(alt.clone());
        }
    }
    alts
}

fn resolve_source_meta(
    source_track: Option<&SpotifyTrack>,
    res: &TrackResolution,
) -> (String, String) {
    if let Some(st) = source_track {
        return (st.title.clone(), st.artist.clone());
    }
    match res {
        TrackResolution::Matched { track, .. } => (track.title.clone(), track.artist.clone()),
        TrackResolution::Unresolved { title, artist, .. } => (title.clone(), artist.clone()),
    }
}

fn map_row(
    index: usize,
    source_track: Option<&SpotifyTrack>,
    resolution: &TrackResolution,
) -> ImportRow {
    let (source_title, source_artist) = resolve_source_meta(source_track, resolution);
    match resolution {
        TrackResolution::Matched { track, alternatives } => ImportRow {
            index,
            source_title,
            source_artist,
            chosen: Some(track.clone()),
            confidence: Some(track.confidence),
            alternatives: map_alternatives(track, alternatives),
            chosen_alternative: 0,
            reason: None,
        },
        TrackResolution::Unresolved { reason, .. } => ImportRow {
            index,
            source_title,
            source_artist,
            chosen: None,
            confidence: None,
            alternatives: Vec::new(),
            chosen_alternative: 0,
            reason: Some(reason.clone()),
        },
    }
}

fn tally_row(counts: &mut ImportCounts, row: &ImportRow) {
    if row.chosen.is_none() {
        counts.unresolved += 1;
        return;
    }
    match row.confidence {
        Some(MatchConfidence::High) => counts.high += 1,
        Some(MatchConfidence::Medium) => counts.review += 1,
        Some(MatchConfidence::Low) => counts.low += 1,
        None => counts.unresolved += 1,
    }
}

impl ImportReview {
    /// Builds review state from Spotify playlist metadata and matching worker resolutions.
    pub fn from_resolutions(source: &SpotifyPlaylist, resolutions: &[TrackResolution]) -> Self {
        let rows = resolutions
            .iter()
            .enumerate()
            .map(|(i, res)| map_row(i, source.tracks.get(i), res))
            .collect();

        Self {
            playlist_title: source.title.clone(),
            playlist_id: source.id.clone(),
            rows,
            selected: 0,
            author: source.author.clone(),
            artwork: source.artwork.clone(),
        }
    }

    /// Calculates match tallies across all review rows.
    pub fn counts(&self) -> ImportCounts {
        let mut counts = ImportCounts { total: self.rows.len(), ..Default::default() };
        for row in &self.rows {
            tally_row(&mut counts, row);
        }
        counts
    }

    /// Swaps the chosen track of a row with one of its alternatives.
    pub fn swap(&mut self, row: usize, alternative: usize) -> bool {
        let Some(target_row) = self.rows.get_mut(row) else {
            return false;
        };
        let Some(alt) = target_row.alternatives.get(alternative).cloned() else {
            return false;
        };
        target_row.confidence = Some(alt.confidence);
        target_row.chosen = Some(alt);
        target_row.chosen_alternative = alternative;
        true
    }

    /// Selects an active row by index.
    pub fn select(&mut self, row: usize) -> bool {
        if row >= self.rows.len() {
            return false;
        }
        self.selected = row;
        true
    }

    /// Returns a reference to the currently selected row, if any.
    pub fn selected_row(&self) -> Option<&ImportRow> {
        self.rows.get(self.selected)
    }

    /// Returns `true` if at least one track was successfully matched.
    pub fn has_matches(&self) -> bool {
        self.rows.iter().any(|r| r.chosen.is_some())
    }

    /// Produces a persisted `LocalPlaylist` containing all chosen matches in source order.
    pub fn to_local_playlist(&self) -> LocalPlaylist {
        let mut playlist =
            LocalPlaylist::new(&local_playlist_id(&self.playlist_id), &self.playlist_title)
                .with_author(&self.author);
        if let Some(ref art) = self.artwork {
            playlist = playlist.with_artwork(art);
        }
        playlist.tracks = self.rows.iter().filter_map(|r| r.chosen.clone()).collect();
        playlist
    }
}

/// Returns the i18n translation key for a match confidence tier.
pub fn confidence_label_key(confidence: MatchConfidence) -> &'static str {
    match confidence {
        MatchConfidence::High => "High confidence",
        MatchConfidence::Medium => "Medium confidence",
        MatchConfidence::Low => "Low confidence",
    }
}

/// Returns the i18n translation key for a row review state.
pub fn row_state_key(row: &ImportRow) -> &'static str {
    if row.chosen.is_none() {
        return "No match";
    }
    match row.confidence {
        Some(MatchConfidence::High) => "Matched",
        Some(MatchConfidence::Medium | MatchConfidence::Low) => "Needs review",
        None => "No match",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spotify_playlist() -> SpotifyPlaylist {
        SpotifyPlaylist {
            id: "spotify123".to_string(),
            title: "Road Trip".to_string(),
            author: "Tester".to_string(),
            artwork: Some("https://example.com/cover.jpg".to_string()),
            tracks: vec![
                SpotifyTrack {
                    title: "Song 1".to_string(),
                    artist: "Artist 1".to_string(),
                    album: None,
                    duration_ms: 180000,
                    isrc: None,
                },
                SpotifyTrack {
                    title: "Song 2".to_string(),
                    artist: "Artist 2".to_string(),
                    album: None,
                    duration_ms: 200000,
                    isrc: None,
                },
                SpotifyTrack {
                    title: "Song 3".to_string(),
                    artist: "Artist 3".to_string(),
                    album: None,
                    duration_ms: 220000,
                    isrc: None,
                },
                SpotifyTrack {
                    title: "Song 4".to_string(),
                    artist: "Artist 4".to_string(),
                    album: None,
                    duration_ms: 240000,
                    isrc: None,
                },
            ],
        }
    }

    #[test]
    fn test_from_resolutions_and_counts_with_mixed_resolutions() {
        let playlist = sample_spotify_playlist();
        let track_high = LocalTrack::new("id_h", "Song 1", "Artist 1", MatchConfidence::High);
        let track_med = LocalTrack::new("id_m", "Song 2", "Artist 2", MatchConfidence::Medium);
        let track_low = LocalTrack::new("id_l", "Song 3", "Artist 3", MatchConfidence::Low);

        let resolutions = vec![
            TrackResolution::Matched { track: track_high, alternatives: Vec::new() },
            TrackResolution::Matched { track: track_med, alternatives: Vec::new() },
            TrackResolution::Matched { track: track_low, alternatives: Vec::new() },
            TrackResolution::Unresolved {
                title: "Song 4".to_string(),
                artist: "Artist 4".to_string(),
                reason: "Not found".to_string(),
            },
        ];

        let review = ImportReview::from_resolutions(&playlist, &resolutions);
        assert_eq!(review.playlist_title, "Road Trip");
        assert_eq!(review.playlist_id, "spotify123");
        assert_eq!(review.author, "Tester");
        assert_eq!(review.artwork.as_deref(), Some("https://example.com/cover.jpg"));
        assert_eq!(review.rows.len(), 4);

        let counts = review.counts();
        assert_eq!(counts.total, 4);
        assert_eq!(counts.high, 1);
        assert_eq!(counts.review, 1);
        assert_eq!(counts.low, 1);
        assert_eq!(counts.unresolved, 1);
    }

    #[test]
    fn test_swap_updates_chosen_and_confidence() {
        let playlist = sample_spotify_playlist();
        let primary = LocalTrack::new("id_pri", "Song 1", "Artist 1", MatchConfidence::High);
        let alt = LocalTrack::new("id_alt", "Song 1 Alt", "Artist 1", MatchConfidence::Medium);

        let resolutions =
            vec![TrackResolution::Matched { track: primary, alternatives: vec![alt] }];

        let mut review = ImportReview::from_resolutions(&playlist, &resolutions);
        assert_eq!(review.rows[0].chosen_alternative, 0);
        assert_eq!(review.rows[0].chosen.as_ref().map(|t| t.video_id.as_str()), Some("id_pri"));
        assert_eq!(review.rows[0].confidence, Some(MatchConfidence::High));

        // Valid swap
        assert!(review.swap(0, 1));
        assert_eq!(review.rows[0].chosen_alternative, 1);
        assert_eq!(review.rows[0].chosen.as_ref().map(|t| t.video_id.as_str()), Some("id_alt"));
        assert_eq!(review.rows[0].confidence, Some(MatchConfidence::Medium));

        // Out of bounds row
        assert!(!review.swap(99, 0));

        // Out of bounds alternative
        assert!(!review.swap(0, 99));
    }

    #[test]
    fn test_select_and_selected_row() {
        let playlist = sample_spotify_playlist();
        let track = LocalTrack::new("id_1", "Song 1", "Artist 1", MatchConfidence::High);
        let resolutions = vec![
            TrackResolution::Matched { track: track.clone(), alternatives: Vec::new() },
            TrackResolution::Matched { track, alternatives: Vec::new() },
        ];

        let mut review = ImportReview::from_resolutions(&playlist, &resolutions);
        assert_eq!(review.selected, 0);
        assert_eq!(review.selected_row().map(|r| r.index), Some(0));

        assert!(review.select(1));
        assert_eq!(review.selected, 1);
        assert_eq!(review.selected_row().map(|r| r.index), Some(1));

        // Out of bounds select
        assert!(!review.select(10));
        assert_eq!(review.selected, 1);
    }

    #[test]
    fn test_to_local_playlist_preserves_order_and_omits_unresolved() {
        let playlist = sample_spotify_playlist();
        let track1 = LocalTrack::new("id_1", "Song 1", "Artist 1", MatchConfidence::High);
        let track3 = LocalTrack::new("id_3", "Song 3", "Artist 3", MatchConfidence::Low);

        let resolutions = vec![
            TrackResolution::Matched { track: track1, alternatives: Vec::new() },
            TrackResolution::Unresolved {
                title: "Song 2".to_string(),
                artist: "Artist 2".to_string(),
                reason: "Failed search".to_string(),
            },
            TrackResolution::Matched { track: track3, alternatives: Vec::new() },
        ];

        let review = ImportReview::from_resolutions(&playlist, &resolutions);
        let local = review.to_local_playlist();

        assert_eq!(local.id, "local:spotify123");
        assert_eq!(local.title, "Road Trip");
        assert_eq!(local.author, "Tester");
        assert_eq!(local.artwork.as_deref(), Some("https://example.com/cover.jpg"));
        assert_eq!(local.tracks.len(), 2);
        assert_eq!(local.tracks[0].video_id, "id_1");
        assert_eq!(local.tracks[1].video_id, "id_3");
    }

    #[test]
    fn test_unresolved_row_retains_reason() {
        let playlist = sample_spotify_playlist();
        let resolutions = vec![TrackResolution::Unresolved {
            title: "Unknown Song".to_string(),
            artist: "Unknown Artist".to_string(),
            reason: "No search results returned".to_string(),
        }];

        let review = ImportReview::from_resolutions(&playlist, &resolutions);
        let row = &review.rows[0];
        assert_eq!(row.source_title, "Song 1");
        assert_eq!(row.source_artist, "Artist 1");
        assert!(row.chosen.is_none());
        assert!(row.confidence.is_none());
        assert!(row.alternatives.is_empty());
        assert_eq!(row.reason.as_deref(), Some("No search results returned"));
    }

    #[test]
    fn test_confidence_label_key() {
        assert_eq!(confidence_label_key(MatchConfidence::High), "High confidence");
        assert_eq!(confidence_label_key(MatchConfidence::Medium), "Medium confidence");
        assert_eq!(confidence_label_key(MatchConfidence::Low), "Low confidence");
    }

    #[test]
    fn test_row_state_key() {
        let mut row = ImportRow {
            index: 0,
            source_title: "Title".to_string(),
            source_artist: "Artist".to_string(),
            chosen: None,
            confidence: None,
            alternatives: Vec::new(),
            chosen_alternative: 0,
            reason: None,
        };

        // None -> No match
        assert_eq!(row_state_key(&row), "No match");

        // High -> Matched
        row.chosen = Some(LocalTrack::new("id", "Title", "Artist", MatchConfidence::High));
        row.confidence = Some(MatchConfidence::High);
        assert_eq!(row_state_key(&row), "Matched");

        // Medium -> Needs review
        row.confidence = Some(MatchConfidence::Medium);
        assert_eq!(row_state_key(&row), "Needs review");

        // Low -> Needs review
        row.confidence = Some(MatchConfidence::Low);
        assert_eq!(row_state_key(&row), "Needs review");
    }

    #[test]
    fn test_deduplication_of_alternatives_with_chosen_first() {
        let playlist = sample_spotify_playlist();
        let primary = LocalTrack::new("id_1", "Primary", "Artist", MatchConfidence::High);
        let alt1 = LocalTrack::new("id_2", "Alt 1", "Artist", MatchConfidence::Medium);
        let alt_duplicate_primary =
            LocalTrack::new("id_1", "Duplicate Primary", "Artist", MatchConfidence::Low);
        let alt2 = LocalTrack::new("id_3", "Alt 2", "Artist", MatchConfidence::Low);
        let alt_duplicate_alt1 =
            LocalTrack::new("id_2", "Duplicate Alt 1", "Artist", MatchConfidence::Medium);

        let resolutions = vec![TrackResolution::Matched {
            track: primary,
            alternatives: vec![alt1, alt_duplicate_primary, alt2, alt_duplicate_alt1],
        }];

        let review = ImportReview::from_resolutions(&playlist, &resolutions);
        let row = &review.rows[0];
        assert_eq!(row.alternatives.len(), 3);
        assert_eq!(row.alternatives[0].video_id, "id_1");
        assert_eq!(row.alternatives[1].video_id, "id_2");
        assert_eq!(row.alternatives[2].video_id, "id_3");
    }

    #[test]
    fn test_has_matches() {
        let playlist = sample_spotify_playlist();
        let mut review = ImportReview::from_resolutions(
            &playlist,
            &[TrackResolution::Unresolved {
                title: "T".to_string(),
                artist: "A".to_string(),
                reason: "R".to_string(),
            }],
        );
        assert!(!review.has_matches());

        let matched = LocalTrack::new("id", "T", "A", MatchConfidence::High);
        review.rows[0].chosen = Some(matched);
        assert!(review.has_matches());
    }
}
