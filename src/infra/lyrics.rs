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

#[cfg(test)]
mod tests {
    use super::*;

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
}
