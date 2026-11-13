//! Ranked top-result selection for catalog search.
//!
//! Invariant: this module performs no I/O, network calls, or clocks.

use super::library::SearchResult;
use super::matching::{normalize_for_match, title_similarity};

/// Minimum score an item needs to be promoted to the top-result card.
pub const MIN_TOP_RESULT_SCORE: f32 = 0.62;

/// Bonus applied when the normalized entity name equals the normalized query.
const EXACT_MATCH_BONUS: f32 = 0.25;

/// Bonus applied when the entity name starts with the normalized query tokens.
const PREFIX_MATCH_BONUS: f32 = 0.10;

/// Bonus applied when every normalized query token appears in the entity name.
const TOKEN_CONTAINMENT_BONUS: f32 = 0.08;

/// Score penalty applied to music videos so a comparably matching song wins the card.
const VIDEO_TOP_RESULT_PENALTY: f32 = 0.15;

/// Returns the display name used to match a search hit against the query.
pub fn entity_name(item: &SearchResult) -> &str {
    match item {
        SearchResult::Track(track) => &track.title,
        SearchResult::Album(album) => &album.title,
        SearchResult::Artist(artist) => &artist.name,
        SearchResult::Playlist(playlist) => &playlist.title,
    }
}

fn type_prior(item: &SearchResult) -> f32 {
    match item {
        SearchResult::Artist(_) => 0.06,
        SearchResult::Album(_) => 0.05,
        SearchResult::Playlist(_) => 0.02,
        SearchResult::Track(track) if track.is_music_video() => -VIDEO_TOP_RESULT_PENALTY,
        SearchResult::Track(_) => 0.0,
    }
}

fn containment_bonus(normalized_query: &str, normalized_name: &str) -> f32 {
    if normalized_query.is_empty() || normalized_name.is_empty() {
        return 0.0;
    }
    if normalized_name == normalized_query {
        return EXACT_MATCH_BONUS;
    }
    let query_tokens: Vec<&str> = normalized_query.split_whitespace().collect();
    let name_tokens: Vec<&str> = normalized_name.split_whitespace().collect();
    if name_tokens.starts_with(&query_tokens) {
        return PREFIX_MATCH_BONUS;
    }
    if query_tokens.iter().all(|token| name_tokens.contains(token)) {
        return TOKEN_CONTAINMENT_BONUS;
    }
    0.0
}

fn raw_score(query: &str, item: &SearchResult) -> f32 {
    let name = entity_name(item);
    let normalized_query = normalize_for_match(query);
    let normalized_name = normalize_for_match(name);
    title_similarity(query, name)
        + containment_bonus(&normalized_query, &normalized_name)
        + type_prior(item)
}

/// Scores how well `item` matches `query` for top-result promotion, clamped to `0.0..=1.0`.
pub fn score_top_result(query: &str, item: &SearchResult) -> f32 {
    raw_score(query, item).clamp(0.0, 1.0)
}

/// Picks the single best search hit for `query`, or `None` when no hit clears
/// [`MIN_TOP_RESULT_SCORE`]. Ties keep the first occurrence in `items`.
pub fn best_match(query: &str, items: &[SearchResult]) -> Option<SearchResult> {
    let mut best: Option<(f32, &SearchResult)> = None;
    for item in items {
        let score = raw_score(query, item);
        let is_better = match best {
            Some((best_score, _)) => score > best_score,
            None => score >= MIN_TOP_RESULT_SCORE,
        };
        if is_better {
            best = Some((score, item));
        }
    }
    best.map(|(_, item)| item.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, Artist, MusicVideoType, Playlist, Track};

    fn track(title: &str, artist: &str) -> SearchResult {
        SearchResult::Track(Track::new("t-1", title, artist))
    }

    fn artist(name: &str) -> SearchResult {
        SearchResult::Artist(Artist::new("ar-1", name, None))
    }

    fn album(title: &str, artist_name: &str) -> SearchResult {
        SearchResult::Album(Album::new("a-1", title, artist_name, None))
    }

    fn playlist(title: &str) -> SearchResult {
        SearchResult::Playlist(Playlist::new("p-1", title, 10, None))
    }

    #[test]
    fn exact_track_match_is_promoted() {
        let items = vec![track("Celoso", "Santa Fe Klan"), track("Starboy", "The Weeknd")];
        let best = best_match("Celoso", &items).expect("exact match");
        assert_eq!(entity_name(&best), "Celoso");
    }

    #[test]
    fn entity_name_covers_every_variant() {
        assert_eq!(entity_name(&track("T", "A")), "T");
        assert_eq!(entity_name(&album("AL", "A")), "AL");
        assert_eq!(entity_name(&artist("AR")), "AR");
        assert_eq!(entity_name(&playlist("PL")), "PL");
    }

    #[test]
    fn artist_wins_over_track_with_equal_name() {
        let items = vec![track("Midnight", "Someone"), artist("Midnight")];
        let best = best_match("midnight", &items).expect("match");
        assert_eq!(entity_name(&best), "Midnight");
        assert!(matches!(best, SearchResult::Artist(_)));
    }

    #[test]
    fn token_containment_matches_artist_with_leading_article() {
        let items = vec![
            track("Celoso", "Santa Fe Klan"),
            track("Corazon Herido", "Los angeles de Santa Fe"),
            artist("Los Angeles de Santa Fe"),
        ];
        let best = best_match("angeles de santa fe", &items).expect("contained tokens");
        assert!(matches!(&best, SearchResult::Artist(_)));
        assert_eq!(entity_name(&best), "Los Angeles de Santa Fe");
    }

    #[test]
    fn prefix_match_is_promoted() {
        let items = vec![track("Midnight City", "M83")];
        let best = best_match("midnight", &items).expect("prefix match");
        assert_eq!(entity_name(&best), "Midnight City");
    }

    #[test]
    fn unrelated_hit_stays_below_threshold() {
        let items = vec![track("Starboy", "The Weeknd")];
        assert_eq!(best_match("midnight", &items), None);
        assert_eq!(best_match("santa fe", &items), None);
    }

    #[test]
    fn blank_query_and_empty_items_return_none() {
        assert_eq!(best_match("", &[] as &[SearchResult]), None);
        assert_eq!(best_match("   ", &[artist("Los Angeles")]), None);
    }

    #[test]
    fn diacritics_are_folded_before_matching() {
        let items = vec![track("Cancion del Amor", "Los Angeles")];
        let best = best_match("canción del amor", &items).expect("folded match");
        assert_eq!(entity_name(&best), "Cancion del Amor");
    }

    #[test]
    fn exact_entity_scores_clamp_to_one() {
        let score = score_top_result("Celoso", &track("Celoso", "Santa Fe Klan"));
        assert_eq!(score, 1.0);
    }

    #[test]
    fn album_match_beats_loose_track() {
        let items = vec![track("Random", "Someone"), album("Random Access Memories", "Daft Punk")];
        let best = best_match("random access memories", &items).expect("album match");
        assert_eq!(entity_name(&best), "Random Access Memories");
    }

    #[test]
    fn ties_keep_first_occurrence() {
        let items = vec![track("Same", "A"), track("Same", "B")];
        let best = best_match("same", &items).expect("match");
        assert_eq!(entity_name(&best), "Same");
        assert!(matches!(&best, SearchResult::Track(t) if t.artist == "A"));
    }

    #[test]
    fn music_video_loses_to_an_equally_named_song() {
        let video = SearchResult::Track(
            Track::new("v1", "Midnight City", "M83").with_video_type(MusicVideoType::Omv),
        );
        let song = SearchResult::Track(
            Track::new("s1", "Midnight City", "M83").with_video_type(MusicVideoType::Atv),
        );
        let best = best_match("midnight city", &[video, song.clone()]).expect("match");
        assert_eq!(best, song);
    }

    #[test]
    fn exact_music_video_still_wins_over_an_unrelated_song() {
        let video = SearchResult::Track(
            Track::new("v1", "Midnight City", "M83").with_video_type(MusicVideoType::Omv),
        );
        let song = SearchResult::Track(
            Track::new("s1", "Starboy", "The Weeknd").with_video_type(MusicVideoType::Atv),
        );
        let best = best_match("midnight city", &[video, song]).expect("match");
        assert!(matches!(&best, SearchResult::Track(track) if track.id == "v1"));
    }

    #[test]
    fn video_penalty_lowers_the_top_result_score() {
        let video = SearchResult::Track(
            Track::new("v1", "Midnight City", "M83").with_video_type(MusicVideoType::Ugc),
        );
        let song = SearchResult::Track(
            Track::new("s1", "Midnight City", "M83").with_video_type(MusicVideoType::Atv),
        );
        let delta = raw_score("midnight city", &song) - raw_score("midnight city", &video);
        assert!((delta - VIDEO_TOP_RESULT_PENALTY).abs() < 1e-6);
    }
}
