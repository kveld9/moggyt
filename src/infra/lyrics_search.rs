//! Ranking of catalog candidates against an lrc.red lyrics-search hit.

use super::lyrics_lrcred::LrcRedHit;
use crate::domain::{Track, artist_score, duration_score, title_similarity};

/// Minimum combined score for a catalog track to be shown as a lyrics match.
pub const LYRICS_MATCH_THRESHOLD: f32 = 0.6;

/// Maximum number of lrc.red hits resolved against the catalog per search.
pub const LYRICS_HIT_LIMIT: usize = 3;

/// Maximum number of catalog tracks returned per lrc.red hit.
pub const LYRICS_RESULT_LIMIT: usize = 10;

const WEIGHT_TITLE: f32 = 0.6;
const WEIGHT_ARTIST: f32 = 0.25;
const WEIGHT_DURATION: f32 = 0.15;

/// Scores how well `track` matches the release described by `hit`.
pub fn lyrics_match_score(hit: &LrcRedHit, track: &Track) -> f32 {
    let title = title_similarity(&hit.title, &track.title);
    let artist = artist_score(&hit.artist, &track.artist, None);
    let duration = duration_score(track.duration_secs, hit.duration_secs.map(|secs| secs as u32));
    (title * WEIGHT_TITLE + artist * WEIGHT_ARTIST + duration * WEIGHT_DURATION).clamp(0.0, 1.0)
}

/// Ranks and filters catalog `candidates` for one lyrics-search hit, best first.
pub fn rank_lyrics_candidates(hit: &LrcRedHit, candidates: &[Track]) -> Vec<Track> {
    let mut scored: Vec<(f32, &Track)> = candidates
        .iter()
        .map(|track| (lyrics_match_score(hit, track), track))
        .filter(|(score, _)| *score >= LYRICS_MATCH_THRESHOLD)
        .collect();
    scored.sort_by(|left, right| right.0.total_cmp(&left.0));
    scored.into_iter().take(LYRICS_RESULT_LIMIT).map(|(_, track)| track.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit() -> LrcRedHit {
        LrcRedHit {
            isrc: "GBAYE0000001".to_string(),
            title: "Bohemian Rhapsody".to_string(),
            artist: "Queen".to_string(),
            album: None,
            year: Some(1975),
            duration_secs: Some(354),
        }
    }

    #[test]
    fn test_exact_match_ranks_first_and_passes_threshold() {
        let exact = Track::new("v1", "Bohemian Rhapsody", "Queen").with_duration(354);
        let cover =
            Track::new("v2", "Bohemian Rhapsody (Piano Cover)", "Some Pianist").with_duration(200);
        let ranked = rank_lyrics_candidates(&hit(), &[cover.clone(), exact.clone()]);
        assert_eq!(ranked.first().map(|track| track.id.as_str()), Some("v1"));
        assert!(
            !ranked.iter().any(|track| track.id == cover.id),
            "cover must score below threshold"
        );
    }

    #[test]
    fn test_unrelated_candidates_are_dropped() {
        let ranked = rank_lyrics_candidates(
            &hit(),
            &[Track::new("x", "Completely Different Song", "Another Artist").with_duration(90)],
        );
        assert!(ranked.is_empty());
    }

    #[test]
    fn test_empty_candidates_yield_empty_ranking() {
        assert!(rank_lyrics_candidates(&hit(), &[]).is_empty());
    }
}
