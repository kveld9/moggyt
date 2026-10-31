//! InnerTube search adapter and candidate ranking for Spotify imports.
//!
//! Maps InnerTube song search results to domain candidate hits, scores candidates
//! using multi-factor heuristics, and ranks alternatives for track review.

use crate::domain::{LocalTrack, MatchCandidate, MatchConfidence, SpotifyTrack, score_candidate};
use crate::i18n::Locale;
use crate::infra::{SearchFailure, SongCandidate, SpotifySongSearch};

use super::import_worker::TrackSearcher;

/// Minimum composite score required to accept a YouTube Music candidate.
pub const MIN_ACCEPT_SCORE: f32 = 0.50;

/// Maximum number of alternative candidates retained per resolved track.
pub const MAX_ALTERNATIVES: usize = 3;

/// One YouTube Music candidate surfaced to the worker, already reduced to domain types.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateHit {
    pub video_id: String,
    pub artwork: Option<String>,
    pub match_candidate: MatchCandidate,
}

impl CandidateHit {
    /// Creates a new candidate hit.
    pub fn new(
        video_id: impl Into<String>,
        artwork: Option<String>,
        match_candidate: MatchCandidate,
    ) -> Self {
        Self { video_id: video_id.into(), artwork, match_candidate }
    }
}

/// Picks the highest scoring candidate above [`MIN_ACCEPT_SCORE`], else None.
pub fn best_candidate<'a>(
    source: &SpotifyTrack,
    hits: &'a [CandidateHit],
) -> Option<(f32, &'a CandidateHit)> {
    hits.iter()
        .map(|hit| (score_candidate(source, &hit.match_candidate).total, hit))
        .filter(|(score, _)| *score >= MIN_ACCEPT_SCORE)
        .max_by(|(s1, _), (s2, _)| s1.partial_cmp(s2).unwrap_or(std::cmp::Ordering::Equal))
}

fn build_local_track(source: &SpotifyTrack, hit: &CandidateHit, score: f32) -> LocalTrack {
    let confidence = MatchConfidence::from_score(score);
    let mut local = LocalTrack::new(&hit.video_id, &source.title, &source.artist, confidence);
    local.album = source.album.clone();
    local.duration_secs = hit
        .match_candidate
        .duration_secs
        .or_else(|| (source.duration_ms > 0).then_some((source.duration_ms + 500) / 1000));
    local
}

/// Returns up to [`MAX_ALTERNATIVES`] candidate matches sorted descending by score.
///
/// Filters out candidates below [`MIN_ACCEPT_SCORE`], deduplicates by `video_id`
/// retaining the highest-scoring occurrence, and converts each to a [`LocalTrack`].
pub fn ranked_alternatives(source: &SpotifyTrack, hits: &[CandidateHit]) -> Vec<LocalTrack> {
    let mut scored: Vec<(f32, &CandidateHit)> = hits
        .iter()
        .map(|hit| (score_candidate(source, &hit.match_candidate).total, hit))
        .filter(|(score, _)| *score >= MIN_ACCEPT_SCORE)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut result = Vec::with_capacity(MAX_ALTERNATIVES.min(scored.len()));
    let mut seen_ids = Vec::with_capacity(MAX_ALTERNATIVES.min(scored.len()));
    for (score, hit) in scored {
        if seen_ids.contains(&hit.video_id.as_str()) {
            continue;
        }
        seen_ids.push(hit.video_id.as_str());
        result.push(build_local_track(source, hit, score));
        if result.len() == MAX_ALTERNATIVES {
            break;
        }
    }
    result
}

/// Adapter binding the InnerTube search client to the [`TrackSearcher`] port.
#[derive(Clone, Debug, Default)]
pub struct LiveTrackSearcher {
    pub(crate) search: SpotifySongSearch,
}

impl LiveTrackSearcher {
    /// Creates a new live searcher using default user agent and settings.
    pub fn new() -> Self {
        Self { search: SpotifySongSearch::new() }
    }
}

fn map_candidate(c: SongCandidate) -> CandidateHit {
    let match_candidate = c.to_match_candidate();
    CandidateHit { video_id: c.video_id, artwork: c.thumb_url, match_candidate }
}

impl TrackSearcher for LiveTrackSearcher {
    fn search(&self, query: &str) -> Result<Vec<CandidateHit>, SearchFailure> {
        let raw = self.search.search(query, Locale::English)?;
        Ok(raw.into_iter().map(map_candidate).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::MusicVideoType;

    fn test_track(title: &str, artist: &str) -> SpotifyTrack {
        SpotifyTrack {
            title: title.into(),
            artist: artist.into(),
            album: Some("Test Album".into()),
            duration_ms: 180_000,
            isrc: None,
        }
    }

    fn test_hit(
        id: &str,
        title: &str,
        channel: &str,
        duration_secs: Option<u32>,
        video_type: MusicVideoType,
    ) -> CandidateHit {
        CandidateHit {
            video_id: id.to_string(),
            artwork: Some(format!("https://example.com/{id}.jpg")),
            match_candidate: MatchCandidate::new(title, channel, duration_secs, video_type),
        }
    }

    #[test]
    fn test_live_track_searcher_construct() {
        let searcher = LiveTrackSearcher::new();
        assert!(format!("{searcher:?}").contains("LiveTrackSearcher"));
        let default_searcher = LiveTrackSearcher::default();
        assert!(format!("{default_searcher:?}").contains("LiveTrackSearcher"));
    }

    #[test]
    fn test_best_candidate_picks_highest_and_enforces_threshold() {
        let t = test_track("Heroes", "David Bowie");
        assert!(best_candidate(&t, &[]).is_none());
        assert!(
            best_candidate(
                &t,
                &[test_hit("low", "Unrelated", "Topic", Some(180), MusicVideoType::Ugc)]
            )
            .is_none()
        );
        let pool = [
            test_hit("good", "Heroes", "Topic", Some(180), MusicVideoType::Atv),
            test_hit("better", "Heroes", "David Bowie", Some(180), MusicVideoType::Atv),
        ];
        let (score, hit) = best_candidate(&t, &pool).expect("finds candidate");
        assert!(score >= MIN_ACCEPT_SCORE);
        assert_eq!(hit.video_id, "better");
    }

    #[test]
    fn test_ranked_alternatives_ordering_and_cap_of_three() {
        let t = test_track("Heroes", "David Bowie");
        let pool = [
            test_hit("v1", "Heroes", "David Bowie", Some(180), MusicVideoType::Atv),
            test_hit("v2", "Heroes", "David Bowie - Topic", Some(180), MusicVideoType::Atv),
            test_hit("v3", "Heroes", "David Bowie", Some(182), MusicVideoType::Other),
            test_hit("v4", "Heroes", "David Bowie", Some(185), MusicVideoType::Other),
            test_hit("v5", "Heroes", "David Bowie", Some(188), MusicVideoType::Other),
        ];
        let alts = ranked_alternatives(&t, &pool);
        assert_eq!(alts.len(), MAX_ALTERNATIVES);
        assert_eq!(alts.len(), 3);
        assert_eq!(alts[0].video_id, "v1");
        assert_eq!(alts[1].video_id, "v2");
        assert_eq!(alts[2].video_id, "v3");
        assert_eq!(alts[0].confidence, MatchConfidence::High);
    }

    #[test]
    fn test_ranked_alternatives_dedupe_by_video_id() {
        let t = test_track("Heroes", "David Bowie");
        let pool = [
            test_hit("v1", "Heroes", "David Bowie", Some(180), MusicVideoType::Atv),
            test_hit("v1", "Heroes", "Topic", Some(180), MusicVideoType::Ugc),
            test_hit("v2", "Heroes", "David Bowie - Topic", Some(180), MusicVideoType::Atv),
        ];
        let alts = ranked_alternatives(&t, &pool);
        assert_eq!(alts.len(), 2);
        assert_eq!(alts[0].video_id, "v1");
        assert_eq!(alts[1].video_id, "v2");
        assert_eq!(alts[0].confidence, MatchConfidence::High);
    }

    #[test]
    fn test_ranked_alternatives_filters_below_min_accept_score() {
        let t = test_track("Heroes", "David Bowie");
        let pool = [
            test_hit("v_low1", "Completely Unrelated", "Nobody", Some(60), MusicVideoType::Ugc),
            test_hit("v_low2", "Something Else", "Random Channel", Some(300), MusicVideoType::Omv),
        ];
        let alts = ranked_alternatives(&t, &pool);
        assert!(alts.is_empty());
    }

    #[test]
    fn test_ranked_alternatives_copies_source_metadata_and_duration() {
        let mut t = test_track("Heroes", "David Bowie");
        t.album = Some("Heroes Album".into());
        t.duration_ms = 180_000;
        let pool = [
            test_hit("v1", "Heroes", "David Bowie", Some(181), MusicVideoType::Atv),
            test_hit("v2", "Heroes", "David Bowie - Topic", None, MusicVideoType::Atv),
        ];
        let alts = ranked_alternatives(&t, &pool);
        assert_eq!(alts.len(), 2);
        assert_eq!(alts[0].title, "Heroes");
        assert_eq!(alts[0].artist, "David Bowie");
        assert_eq!(alts[0].album.as_deref(), Some("Heroes Album"));
        assert_eq!(alts[0].duration_secs, Some(181));
        assert_eq!(alts[1].duration_secs, Some(180));
    }
}
