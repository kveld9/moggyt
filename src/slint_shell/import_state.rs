//! Review state model for Spotify playlist imports.
//!
//! Hexagonal invariant: pure in-memory view model over import worker resolutions
//! with zero I/O, zero network, zero process spawning, and zero disk writes.

use std::collections::HashSet;

use crate::domain::{
    Detail, DetailKind, LocalPlaylist, LocalTrack, MatchConfidence, Playlist, SpotifyPlaylist,
    SpotifyTrack, local_playlist_id,
};
use crate::i18n::{Locale, tr};
use crate::slint_shell::import_worker::TrackResolution;
use crate::slint_shell::{ImportAlternativeData, ImportRowData};

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

fn row_matched_text(row: &ImportRow, locale: Locale) -> String {
    if let Some(ref track) = row.chosen {
        return format!("{} - {}", track.title, track.artist);
    }
    if let Some(ref reason) = row.reason {
        return reason.clone();
    }
    tr(locale, "No match").to_string()
}

fn row_source_text(row: &ImportRow) -> String {
    if row.source_artist.is_empty() {
        row.source_title.clone()
    } else {
        format!("{} - {}", row.source_title, row.source_artist)
    }
}

fn row_confidence_text(row: &ImportRow, locale: Locale) -> String {
    row.confidence.map(|c| tr(locale, confidence_label_key(c)).to_string()).unwrap_or_default()
}

fn row_to_slint_data(row: &ImportRow, locale: Locale) -> ImportRowData {
    ImportRowData {
        index: row.index as i32,
        source: row_source_text(row).into(),
        matched: row_matched_text(row, locale).into(),
        state: tr(locale, row_state_key(row)).into(),
        confidence: row_confidence_text(row, locale).into(),
        has_match: row.chosen.is_some(),
    }
}

/// Converts an [`ImportReview`] into Slint row models.
pub fn rows_to_slint(review: &ImportReview, locale: Locale) -> Vec<ImportRowData> {
    review.rows.iter().map(|r| row_to_slint_data(r, locale)).collect()
}

/// Converts an [`ImportRow`]'s alternatives into Slint alternative models.
pub fn alternatives_to_slint(row: &ImportRow) -> Vec<ImportAlternativeData> {
    row.alternatives
        .iter()
        .enumerate()
        .map(|(i, alt)| ImportAlternativeData {
            index: i as i32,
            title: alt.title.as_str().into(),
            artist: alt.artist.as_str().into(),
        })
        .collect()
}

/// Formats review counts with localized prefixes into a summary string.
pub fn counts_text(counts: &ImportCounts, locale: Locale) -> String {
    let matched = tr(locale, "Matched");
    let review = tr(locale, "Needs review");
    let unresolved = tr(locale, "Unresolved");
    let review_count = counts.review + counts.low;
    format!(
        "{matched}: {} / {review}: {} / {unresolved}: {}",
        counts.high, review_count, counts.unresolved
    )
}

/// Maps a stored local playlist into a domain [`Detail`] view model.
pub fn local_detail(playlist: &LocalPlaylist) -> Detail {
    let tracks = playlist.tracks.iter().map(|t| t.to_track(playlist.artwork.clone())).collect();
    let mut detail = Detail::new(DetailKind::Playlist, &playlist.title, &playlist.author, tracks);
    detail.artwork = playlist.artwork.clone();
    detail
}

/// Merges local playlists before catalog playlists, sorting local ones by title.
pub fn merge_local_library(catalog: &[Playlist], local: &[LocalPlaylist]) -> Vec<Playlist> {
    let mut local_playlists: Vec<Playlist> = local
        .iter()
        .map(|p| Playlist::new(&p.id, &p.title, p.tracks.len(), p.artwork.clone()))
        .collect();
    local_playlists.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| a.id.cmp(&b.id)));
    local_playlists.extend(catalog.iter().cloned());
    local_playlists
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

    #[test]
    fn test_rows_to_slint_mapping() {
        let playlist = sample_spotify_playlist();
        let track = LocalTrack::new("vid_1", "Song 1", "Artist 1", MatchConfidence::High);
        let resolutions = vec![
            TrackResolution::Matched { track, alternatives: Vec::new() },
            TrackResolution::Unresolved {
                title: "Song 2".to_string(),
                artist: "Artist 2".to_string(),
                reason: "not found in catalog".to_string(),
            },
        ];
        let review = ImportReview::from_resolutions(&playlist, &resolutions);
        let rows = rows_to_slint(&review, Locale::English);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].index, 0);
        assert_eq!(rows[0].source, "Song 1 - Artist 1");
        assert_eq!(rows[0].matched, "Song 1 - Artist 1");
        assert_eq!(rows[0].state, "Matched");
        assert_eq!(rows[0].confidence, "High confidence");
        assert!(rows[0].has_match);

        assert_eq!(rows[1].index, 1);
        assert_eq!(rows[1].source, "Song 2 - Artist 2");
        assert_eq!(rows[1].matched, "not found in catalog");
        assert_eq!(rows[1].state, "No match");
        assert_eq!(rows[1].confidence, "");
        assert!(!rows[1].has_match);
    }

    #[test]
    fn test_alternatives_to_slint_mapping() {
        let alt1 = LocalTrack::new("v1", "Alt 1", "Artist 1", MatchConfidence::High);
        let alt2 = LocalTrack::new("v2", "Alt 2", "Artist 2", MatchConfidence::Medium);
        let row = ImportRow {
            index: 0,
            source_title: "Title".to_string(),
            source_artist: "Artist".to_string(),
            chosen: Some(alt1.clone()),
            confidence: Some(MatchConfidence::High),
            alternatives: vec![alt1, alt2],
            chosen_alternative: 0,
            reason: None,
        };
        let slint_alts = alternatives_to_slint(&row);
        assert_eq!(slint_alts.len(), 2);
        assert_eq!(slint_alts[0].index, 0);
        assert_eq!(slint_alts[0].title, "Alt 1");
        assert_eq!(slint_alts[0].artist, "Artist 1");
        assert_eq!(slint_alts[1].index, 1);
        assert_eq!(slint_alts[1].title, "Alt 2");
        assert_eq!(slint_alts[1].artist, "Artist 2");
    }

    #[test]
    fn test_counts_text_formatting_english_and_spanish() {
        let counts = ImportCounts { total: 10, high: 6, review: 2, low: 1, unresolved: 1 };
        let en = counts_text(&counts, Locale::English);
        assert_eq!(en, "Matched: 6 / Needs review: 3 / Unresolved: 1");

        let es = counts_text(&counts, Locale::Spanish);
        assert_eq!(es, "Coincidencias: 6 / Requieren revision: 3 / Sin resolver: 1");
    }

    #[test]
    fn test_local_detail_maps_tracks_author_and_artwork() {
        let mut local = LocalPlaylist::new("local:123", "Road Tunes").with_author("Traveler");
        local.artwork = Some("https://example.com/art.jpg".to_string());
        local.tracks.push(LocalTrack::new("v1", "Track 1", "Artist 1", MatchConfidence::High));

        let detail = local_detail(&local);
        assert_eq!(detail.kind, DetailKind::Playlist);
        assert_eq!(detail.title, "Road Tunes");
        assert_eq!(detail.subtitle, "Traveler");
        assert_eq!(detail.artwork.as_deref(), Some("https://example.com/art.jpg"));
        assert_eq!(detail.tracks.len(), 1);
        assert_eq!(detail.tracks[0].id, "v1");
        assert_eq!(detail.tracks[0].title, "Track 1");
        assert_eq!(detail.tracks[0].artwork.as_deref(), Some("https://example.com/art.jpg"));
    }

    #[test]
    fn test_merge_local_library_prepends_sorted_local_before_catalog() {
        let catalog = vec![
            Playlist::new("cat_1", "Pop Hits", 10, None),
            Playlist::new("cat_2", "Rock Hits", 20, None),
        ];
        let local =
            vec![LocalPlaylist::new("local:z", "Zeta"), LocalPlaylist::new("local:a", "Alpha")];

        let merged = merge_local_library(&catalog, &local);
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[0].id, "local:a");
        assert_eq!(merged[0].title, "Alpha");
        assert_eq!(merged[1].id, "local:z");
        assert_eq!(merged[1].title, "Zeta");
        assert_eq!(merged[2].id, "cat_1");
        assert_eq!(merged[3].id, "cat_2");
    }
}
