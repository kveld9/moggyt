//! Lyrics retrieval port and deterministic LRCLIB fake.

/// A single timed word (or syllable) inside a lyric line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricWord {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricLine {
    pub start_ms: u64,
    pub text: String,
    /// Per-word timings when the provider supplies them; empty for LRCLIB lines.
    pub words: Vec<LyricWord>,
}

impl LyricLine {
    /// Builds a line without word timings.
    pub fn new(start_ms: u64, text: String) -> Self {
        Self { start_ms, text, words: Vec::new() }
    }

    /// Builds a line carrying per-word timings.
    pub fn with_words(start_ms: u64, text: String, words: Vec<LyricWord>) -> Self {
        Self { start_ms, text, words }
    }

    /// Fraction (`0.0..=1.0`) of the line already sung at `pos_ms`.
    ///
    /// Returns `0.0` for lines without word timings so the renderer falls back to
    /// the line-level highlight.
    pub fn sung_fraction(&self, pos_ms: u64) -> f32 {
        let total = self.words.len();
        if total == 0 {
            return 0.0;
        }
        let started = self.words.partition_point(|word| word.start_ms <= pos_ms);
        if started == 0 {
            return 0.0;
        }
        if started >= total {
            return 1.0;
        }
        let current = &self.words[started - 1];
        let span = current.end_ms.saturating_sub(current.start_ms).max(1);
        let inside = pos_ms.saturating_sub(current.start_ms).min(span) as f32 / span as f32;
        ((started - 1) as f32 + inside) / total as f32
    }

    /// Splits the line text into the sung prefix and the remaining suffix.
    ///
    /// The cut is proportional to `fraction` and always lands on a char boundary.
    pub fn split_at_fraction(&self, fraction: f32) -> (String, String) {
        let chars: Vec<char> = self.text.chars().collect();
        let cut = ((chars.len() as f32) * fraction.clamp(0.0, 1.0)).round() as usize;
        let cut = cut.min(chars.len());
        (chars[..cut].iter().collect(), chars[cut..].iter().collect())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedLyrics {
    pub lines: Vec<LyricLine>,
}

pub trait LyricsSource {
    fn lookup(&self, artist: &str, title: &str, duration_secs: Option<u64>)
    -> Option<SyncedLyrics>;
}

#[derive(Debug, Default)]
pub struct LrclibStub;

impl LrclibStub {
    pub fn new() -> Self {
        Self
    }
}

impl LyricsSource for LrclibStub {
    fn lookup(
        &self,
        artist: &str,
        title: &str,
        _duration_secs: Option<u64>,
    ) -> Option<SyncedLyrics> {
        let trimmed_artist = artist.trim();
        let trimmed_title = title.trim();
        if trimmed_artist.is_empty() || trimmed_title.is_empty() {
            return None;
        }

        Some(SyncedLyrics {
            lines: vec![
                LyricLine::new(0, format!("{trimmed_title} - {trimmed_artist}")),
                LyricLine::new(5000, "Deterministic synced lyrics".to_owned()),
            ],
        })
    }
}

/// Lyrics cascade that queries sources in order and returns the first hit.
#[derive(Debug, Default)]
pub struct LyricsCascade<A, B> {
    primary: A,
    secondary: B,
}

impl<A, B> LyricsCascade<A, B> {
    pub fn new(primary: A, secondary: B) -> Self {
        Self { primary, secondary }
    }

    pub fn primary(&self) -> &A {
        &self.primary
    }

    pub fn secondary(&self) -> &B {
        &self.secondary
    }
}

impl<A: LyricsSource, B: LyricsSource> LyricsSource for LyricsCascade<A, B> {
    fn lookup(
        &self,
        artist: &str,
        title: &str,
        duration_secs: Option<u64>,
    ) -> Option<SyncedLyrics> {
        self.primary
            .lookup(artist, title, duration_secs)
            .or_else(|| self.secondary.lookup(artist, title, duration_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct CountingStub {
        calls: AtomicUsize,
        result: Option<SyncedLyrics>,
    }

    impl CountingStub {
        fn with_result(lines: Vec<LyricLine>) -> Self {
            Self { calls: AtomicUsize::new(0), result: Some(SyncedLyrics { lines }) }
        }

        fn missing() -> Self {
            Self { calls: AtomicUsize::new(0), result: None }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl LyricsSource for CountingStub {
        fn lookup(
            &self,
            _artist: &str,
            _title: &str,
            _duration_secs: Option<u64>,
        ) -> Option<SyncedLyrics> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.clone()
        }
    }

    #[test]
    fn lyrics_lookup_returns_synced_lines_or_none_on_blank() {
        let lyrics = LrclibStub::new();
        let hit = lyrics.lookup("Radiohead", "Creep", Some(236)).expect("lyrics");
        assert_eq!(hit.lines.len(), 2);
        assert_eq!(hit.lines[0].start_ms, 0);

        for (artist, title) in [("", "Creep"), ("Radiohead", ""), (" ", " ")] {
            assert_eq!(lyrics.lookup(artist, title, None), None);
        }
    }

    #[test]
    fn lyrics_stub_ignores_duration() {
        let lyrics = LrclibStub::new();
        let without_dur = lyrics.lookup("Radiohead", "Creep", None);
        let with_dur = lyrics.lookup("Radiohead", "Creep", Some(9999));
        assert_eq!(without_dur, with_dur);
    }

    #[test]
    fn cascade_primary_hit_short_circuits_secondary() {
        let primary = CountingStub::with_result(vec![LyricLine::new(100, "Primary".to_owned())]);
        let secondary =
            CountingStub::with_result(vec![LyricLine::new(200, "Secondary".to_owned())]);
        let cascade = LyricsCascade::new(primary, secondary);

        let hit = cascade.lookup("Artist", "Title", None).expect("lyrics");
        assert_eq!(hit.lines[0].text, "Primary");
        assert_eq!(cascade.primary().call_count(), 1);
        assert_eq!(cascade.secondary().call_count(), 0);
    }

    #[test]
    fn cascade_primary_miss_falls_through_to_secondary() {
        let primary = CountingStub::missing();
        let secondary =
            CountingStub::with_result(vec![LyricLine::new(200, "Secondary".to_owned())]);
        let cascade = LyricsCascade::new(primary, secondary);

        let hit = cascade.lookup("Artist", "Title", None).expect("lyrics");
        assert_eq!(hit.lines[0].text, "Secondary");
        assert_eq!(cascade.primary().call_count(), 1);
        assert_eq!(cascade.secondary().call_count(), 1);
    }

    #[test]
    fn cascade_both_miss_returns_none() {
        let primary = CountingStub::missing();
        let secondary = CountingStub::missing();
        let cascade = LyricsCascade::new(primary, secondary);

        assert_eq!(cascade.lookup("Artist", "Title", None), None);
        assert_eq!(cascade.primary().call_count(), 1);
        assert_eq!(cascade.secondary().call_count(), 1);
    }

    #[test]
    fn cascade_accessors_return_injected_sources() {
        let primary = CountingStub::missing();
        let secondary = CountingStub::missing();
        let cascade = LyricsCascade::new(primary, secondary);

        assert_eq!(cascade.primary().call_count(), 0);
        assert_eq!(cascade.secondary().call_count(), 0);
    }

    #[test]
    fn test_sung_fraction_without_words_is_zero() {
        let line = LyricLine::new(1_000, "no word timings".to_owned());
        assert_eq!(line.sung_fraction(5_000), 0.0);
    }

    #[test]
    fn test_sung_fraction_tracks_word_progress() {
        let line = LyricLine::with_words(
            0,
            "one two three four".to_owned(),
            vec![
                LyricWord { start_ms: 0, end_ms: 1000, text: "one".to_owned() },
                LyricWord { start_ms: 1000, end_ms: 2000, text: "two".to_owned() },
                LyricWord { start_ms: 2000, end_ms: 3000, text: "three".to_owned() },
                LyricWord { start_ms: 3000, end_ms: 4000, text: "four".to_owned() },
            ],
        );
        assert_eq!(line.sung_fraction(0), 0.0);
        assert!((line.sung_fraction(500) - 0.125).abs() < 1e-6);
        assert!((line.sung_fraction(1500) - 0.375).abs() < 1e-6);
        assert_eq!(line.sung_fraction(9_000), 1.0);
    }

    #[test]
    fn test_split_at_fraction_keeps_char_boundaries() {
        let line = LyricLine::new(0, "cancion sin palabras".to_owned());
        let (sung, rest) = line.split_at_fraction(0.5);
        assert_eq!(format!("{sung}{rest}"), "cancion sin palabras");
        assert!(!sung.is_empty() && !rest.is_empty());
        let (all, none) = line.split_at_fraction(1.0);
        assert_eq!(all, "cancion sin palabras");
        assert!(none.is_empty());
    }
}
