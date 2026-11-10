//! Lyrics retrieval port and deterministic LRCLIB fake.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricLine {
    pub start_ms: u64,
    pub text: String,
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
                LyricLine { start_ms: 0, text: format!("{trimmed_title} - {trimmed_artist}") },
                LyricLine { start_ms: 5000, text: "Deterministic synced lyrics".to_owned() },
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
        let primary = CountingStub::with_result(vec![LyricLine {
            start_ms: 100,
            text: "Primary".to_owned(),
        }]);
        let secondary = CountingStub::with_result(vec![LyricLine {
            start_ms: 200,
            text: "Secondary".to_owned(),
        }]);
        let cascade = LyricsCascade::new(primary, secondary);

        let hit = cascade.lookup("Artist", "Title", None).expect("lyrics");
        assert_eq!(hit.lines[0].text, "Primary");
        assert_eq!(cascade.primary().call_count(), 1);
        assert_eq!(cascade.secondary().call_count(), 0);
    }

    #[test]
    fn cascade_primary_miss_falls_through_to_secondary() {
        let primary = CountingStub::missing();
        let secondary = CountingStub::with_result(vec![LyricLine {
            start_ms: 200,
            text: "Secondary".to_owned(),
        }]);
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
}
