//! Artist matching between Spotify source artists and YouTube Music candidate channels.
//!
//! Invariant: this module performs no I/O, network calls, or clocks.

use super::matching::{WEIGHT_ARTIST_PRIMARY, WEIGHT_ARTIST_SECONDARY, token_set};

/// Trailing suffix appended to YouTube Music auto-generated topic channels.
pub const TOPIC_CHANNEL_SUFFIX: &str = "- Topic";

/// Strips a trailing topic channel suffix from a channel name.
///
/// Returns the trimmed channel name without a trailing [`TOPIC_CHANNEL_SUFFIX`].
pub fn strip_topic_suffix(channel: &str) -> &str {
    let trimmed = channel.trim();
    if let Some(stripped) = trimmed.strip_suffix(TOPIC_CHANNEL_SUFFIX) {
        stripped.trim()
    } else {
        trimmed
    }
}

/// Splits a comma-separated source artist string into individual artist names.
///
/// Splits on commas, trims each segment, and filters out empty entries.
pub fn split_source_artists(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(ToString::to_string).collect()
}

fn is_primary_artist_match(source_artist: &str, channel_tokens: &[String]) -> bool {
    let source_tokens = token_set(source_artist);
    !source_tokens.is_empty() && source_tokens.iter().all(|t| channel_tokens.contains(t))
}

fn has_secondary_artist_match(secondary_artists: &[String], channel_tokens: &[String]) -> bool {
    secondary_artists.iter().any(|artist| is_primary_artist_match(artist, channel_tokens))
}

fn is_featured_artist_match(featured: Option<&str>, channel_tokens: &[String]) -> bool {
    let Some(feat) = featured else {
        return false;
    };
    let feat_tokens = token_set(feat);
    !feat_tokens.is_empty() && feat_tokens.iter().any(|t| channel_tokens.contains(t))
}

fn score_from_candidates(
    artists: &[String],
    featured: Option<&str>,
    channel_tokens: &[String],
) -> f32 {
    if is_primary_artist_match(&artists[0], channel_tokens) {
        WEIGHT_ARTIST_PRIMARY
    } else if has_secondary_artist_match(&artists[1..], channel_tokens)
        || is_featured_artist_match(featured, channel_tokens)
    {
        WEIGHT_ARTIST_SECONDARY
    } else {
        0.0
    }
}

/// Scores candidate channel against source artist and optional featured artist.
pub fn artist_score(source_artist: &str, candidate_channel: &str, featured: Option<&str>) -> f32 {
    let channel_clean = strip_topic_suffix(candidate_channel);
    let channel_tokens = token_set(channel_clean);
    let artists = split_source_artists(source_artist);
    if channel_tokens.is_empty() || artists.is_empty() {
        return 0.0;
    }
    score_from_candidates(&artists, featured, &channel_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artist_score_variations() {
        assert_eq!(artist_score("Queen", "Queen", None), WEIGHT_ARTIST_PRIMARY);
        assert_eq!(artist_score("Queen", "Queen - Topic", None), WEIGHT_ARTIST_PRIMARY);
        assert_eq!(
            artist_score("Simon & Garfunkel", "Simon & Garfunkel", None),
            WEIGHT_ARTIST_PRIMARY
        );
        assert_eq!(artist_score("Future", "Drake", Some("Drake")), WEIGHT_ARTIST_SECONDARY);
        assert_eq!(artist_score("Queen", "The Beatles", None), 0.0);
    }

    #[test]
    fn artist_score_multi_artist_primary() {
        assert_eq!(artist_score("ANOTR, 54 Ultra", "ANOTR", None), WEIGHT_ARTIST_PRIMARY);
    }

    #[test]
    fn artist_score_multi_artist_secondary() {
        assert_eq!(artist_score("ANOTR, 54 Ultra", "54 Ultra", None), WEIGHT_ARTIST_SECONDARY);
    }

    #[test]
    fn artist_score_multi_artist_comma_in_name() {
        assert_eq!(
            artist_score("Tyler, The Creator", "Tyler, The Creator", None),
            WEIGHT_ARTIST_PRIMARY
        );
    }

    #[test]
    fn artist_score_multi_artist_no_match() {
        assert_eq!(artist_score("A, B", "Somebody Else", None), 0.0);
    }

    #[test]
    fn split_source_artists_cases() {
        assert_eq!(split_source_artists(" A ,B,, C "), vec!["A", "B", "C"]);
        assert_eq!(split_source_artists(""), Vec::<String>::new());
    }

    #[test]
    fn topic_channel_suffix_stripping_and_scoring() {
        assert_eq!(strip_topic_suffix("Queen - Topic"), "Queen");
        assert_eq!(strip_topic_suffix("Queen"), "Queen");
        assert_eq!(artist_score("Queen", "Queen - Topic", None), WEIGHT_ARTIST_PRIMARY);
    }
}
