//! Lyrics fetching, offset adjustment, and local overrides for [`AppState`].

use crate::app::AppState;
use crate::domain::LyricsSnippetError;
use crate::infra::{
    AudioBackend, Catalog, CoverCache, LyricOverrideDir, LyricsSource, SessionStore,
};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Injects a local lyric overrides directory.
    pub fn set_lyric_overrides(&mut self, overrides: LyricOverrideDir) {
        self.lyric_overrides = Some(overrides);
    }

    /// Sets whether lyrics search lookup mode is enabled.
    pub fn set_lyrics_mode(&mut self, enabled: bool) {
        self.lyrics_mode = enabled;
        self.search_error = None;
        self.lyrics_error = None;
    }

    /// Returns whether lyrics search lookup mode is enabled.
    pub fn is_lyrics_mode(&self) -> bool {
        self.lyrics_mode
    }

    pub(crate) fn record_lyrics_error(&mut self, err: LyricsSnippetError) {
        let msg = err.to_string();
        self.search_error = Some(msg.clone());
        self.lyrics_error = Some(msg);
        self.lyrics_results.clear();
        self.search_results.clear();
    }

    /// Fetches and caches synced lyrics for the currently playing track.
    ///
    /// No-op if lyrics for the current (artist, title) pair have already been resolved.
    /// Override content refreshes on track change like provider content (no file watcher).
    pub fn refresh_lyrics(&mut self) {
        let Some(track) = self.queue.current() else {
            if self.lyrics_key.is_some() {
                self.lyrics_key = None;
                self.lyrics_lines.clear();
                self.lyrics_from_override = false;
            }
            return;
        };
        let key = (track.artist.clone(), track.title.clone());
        if self.lyrics_key.as_ref() == Some(&key) {
            return;
        }
        let override_hit = self.lyric_overrides.as_ref().and_then(|dir| dir.load(&track.id));
        if let Some(override_lyrics) = override_hit {
            self.lyrics_lines = override_lyrics.lines;
            self.lyrics_from_override = true;
        } else {
            let duration = track.duration_secs.map(u64::from);
            self.lyrics_lines =
                self.lyrics.lookup(&key.0, &key.1, duration).map(|s| s.lines).unwrap_or_default();
            self.lyrics_from_override = false;
        }
        self.lyrics_key = Some(key);
    }

    /// Adjusts the user lyrics timing offset by `delta_ms` with saturating arithmetic.
    pub fn nudge_lyrics_offset(&mut self, delta_ms: i64) {
        self.lyrics_offset_ms = self.lyrics_offset_ms.saturating_add(delta_ms);
    }

    /// Computes user-visible lyrics timestamps by applying current offset clamped at zero.
    pub fn visible_lyric_lines(&self) -> Vec<(u64, String)> {
        self.lyrics_lines
            .iter()
            .map(|line| {
                let shifted = (line.start_ms as i64).saturating_add(self.lyrics_offset_ms);
                let visible_ms = shifted.max(0) as u64;
                (visible_ms, line.text.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{TrackingLyricsSource, wired};
    use super::*;
    use crate::app::Page;
    use crate::domain::Track;
    use crate::infra::{
        AudioStub, CatalogStub, LyricLine, LyricOverrideDir, MemoryCoverCache, MemorySessionStore,
    };

    #[test]
    fn lyrics_offset_and_visible_lines_clamping_table() {
        let mut state = wired();
        state.lyrics_lines = vec![
            LyricLine { start_ms: 0, text: "intro".into() },
            LyricLine { start_ms: 5_000, text: "verse".into() },
        ];
        for (delta, off, fst, snd) in [
            (0, 0, 0, 5_000),
            (250, 250, 250, 5_250),
            (-500, -250, 0, 4_750),
            (-5_000, -5_250, 0, 0),
        ] {
            state.nudge_lyrics_offset(delta);
            assert_eq!(state.lyrics_offset_ms, off);
            assert_eq!(
                state.visible_lyric_lines(),
                vec![(fst, "intro".into()), (snd, "verse".into())]
            );
        }
        state.nudge_lyrics_offset(i64::MAX);
        assert_eq!(state.lyrics_offset_ms, i64::MAX - 5_250);
        state.nudge_lyrics_offset(10_000);
        assert_eq!(state.lyrics_offset_ms, i64::MAX);
    }

    #[test]
    fn refresh_lyrics_lifecycle_tracks_active_song_and_refills_on_change() {
        let mut state = wired();
        state.refresh_lyrics();
        assert!(state.lyrics_key.is_none() && state.lyrics_lines.is_empty());

        state.search_text = "midnight".into();
        state.submit_search();
        state.refresh_lyrics();
        assert_eq!(state.lyrics_key, Some(("Stub Artist".into(), "midnight".into())));
        assert_eq!(state.lyrics_lines.len(), 2);
        assert_eq!(state.lyrics_lines[0].text, "midnight - Stub Artist");

        // Skip on same key
        state.refresh_lyrics();
        assert_eq!(state.lyrics_lines.len(), 2);

        // Refill on track change via stub (2 lines)
        state.queue.push(Track::new("s2", "sunrise", "Sunrise Band"));
        state.queue.advance();
        state.refresh_lyrics();
        assert_eq!(state.lyrics_key, Some(("Sunrise Band".into(), "sunrise".into())));
        assert_eq!(state.lyrics_lines.len(), 2);
        assert_eq!(state.lyrics_lines[0].text, "sunrise - Sunrise Band");
    }

    #[test]
    fn refresh_lyrics_forwards_track_duration_to_lyrics_source() {
        let lyrics = TrackingLyricsSource::default();
        let recorded = lyrics.recorded.clone();
        let mut state = AppState::new(
            CatalogStub::new(),
            AudioStub::new(),
            MemorySessionStore::new(),
            lyrics,
            MemoryCoverCache::new(),
        );

        // Track with duration passes Some(secs)
        let track_with_dur = Track::new("t1", "Paranoid Android", "Radiohead").with_duration(383);
        state.queue.push(track_with_dur);
        state.refresh_lyrics();

        // Advance to track without duration passes None
        let track_without_dur = Track::new("t2", "Karma Police", "Radiohead");
        state.queue.push(track_without_dur);
        state.queue.advance();
        state.refresh_lyrics();

        let entries = recorded.lock().unwrap().clone();
        assert_eq!(
            entries,
            vec![
                ("Radiohead".into(), "Paranoid Android".into(), Some(383)),
                ("Radiohead".into(), "Karma Police".into(), None),
            ]
        );
    }

    #[test]
    fn test_app_state_prefers_override_over_stub_provider_lines_and_falls_back_when_absent() {
        let mut state = wired();
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-test-override-pref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let override_dir = LyricOverrideDir::new(&temp_dir);
        state.set_lyric_overrides(override_dir.clone());

        state.queue.push(Track::new("video-pref-1", "Prefer Song", "Prefer Artist"));

        // When override file does not exist, falls back to provider
        state.refresh_lyrics();
        assert!(!state.lyrics_from_override);
        assert_eq!(state.lyrics_lines.len(), 2);
        assert_eq!(state.lyrics_lines[1].text, "Deterministic synced lyrics");

        // Save override for the current track id
        let lrc = "[00:02.50]Local override line\n";
        override_dir.save("video-pref-1", lrc).expect("save override");

        // Simulate track change or cache clear
        state.lyrics_key = None;
        state.refresh_lyrics();
        assert!(state.lyrics_from_override);
        assert_eq!(state.lyrics_lines.len(), 1);
        assert_eq!(state.lyrics_lines[0].start_ms, 2500);
        assert_eq!(state.lyrics_lines[0].text, "Local override line");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn app_state_lyrics_submit_surfaces_validation_errors_for_blank_input() {
        let mut state = wired();
        state.set_lyrics_mode(true);
        assert!(state.is_lyrics_mode());
        state.search_text = "   ".into();
        state.submit_search();

        assert_eq!(state.search_error.as_deref(), Some("lyrics snippet cannot be blank"));
        assert_eq!(state.lyrics_error.as_deref(), Some("lyrics snippet cannot be blank"));
        assert!(state.lyrics_results.is_empty());
        assert!(state.search_results.is_empty());
        assert_eq!(state.page, Page::Search);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn app_state_lyrics_submit_surfaces_validation_errors_for_overlong_input() {
        let mut state = wired();
        state.lyrics_mode = true;
        state.search_text = "x".repeat(201);
        state.submit_search();

        assert!(state.search_error.is_some());
        assert!(state.lyrics_results.is_empty());
        assert_eq!(state.page, Page::Search);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn app_state_lyrics_submit_populates_results_and_enqueues_hit() {
        let mut state = wired();
        state.lyrics_mode = true;
        state.search_text = "  midnight breeze  ".into();
        state.submit_search();

        assert_eq!(state.search_error, None);
        assert_eq!(state.lyrics_error, None);
        assert_eq!(state.lyrics_results.len(), 1);
        assert_eq!(state.lyrics_results[0].id, "stub-1");
        assert_eq!(state.lyrics_results[0].title, "Midnight City");
        assert_eq!(state.search_results, state.lyrics_results);
        assert_eq!(state.page, Page::Queue);
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("stub-1"));
    }

    #[test]
    fn app_state_direct_submit_lyrics_search_call() {
        let mut state = wired();
        state.search_text = "whisper softly".into();
        state.submit_lyrics_search();

        assert_eq!(state.search_error, None);
        assert_eq!(state.lyrics_results.len(), 1);
        assert_eq!(state.lyrics_results[0].id, "stub-2");
        assert_eq!(state.lyrics_results[0].title, "Echoes of Silence");
        assert_eq!(state.page, Page::Queue);
    }

    #[test]
    fn app_state_lyrics_submit_miss_clears_results_and_stays_on_page() {
        let mut state = wired();
        state.lyrics_mode = true;
        state.search_text = "lyrics not matching anything".into();
        state.submit_search();

        assert_eq!(state.search_error, None);
        assert!(state.lyrics_results.is_empty());
        assert_eq!(state.page, Page::Search);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn test_offset_applies_to_override_lines() {
        let mut state = wired();
        let temp_dir = std::env::temp_dir()
            .join(format!("moggyt-test-override-offset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let override_dir = LyricOverrideDir::new(&temp_dir);
        let lrc = "[00:10.00]Override line\n";
        override_dir.save("video-offset-1", lrc).expect("save override");
        state.set_lyric_overrides(override_dir);

        state.queue.push(Track::new("video-offset-1", "Offset Song", "Offset Artist"));
        state.refresh_lyrics();
        assert!(state.lyrics_from_override);

        // Baseline (0ms offset)
        assert_eq!(state.visible_lyric_lines(), vec![(10000, "Override line".to_string())]);

        // Positive offset (+500ms)
        state.nudge_lyrics_offset(500);
        assert_eq!(state.visible_lyric_lines(), vec![(10500, "Override line".to_string())]);

        // Negative offset (-2000ms): 10000 + 500 - 2000 = 8500ms
        state.nudge_lyrics_offset(-2000);
        assert_eq!(state.visible_lyric_lines(), vec![(8500, "Override line".to_string())]);

        // Clamping at zero: nudge by -20000ms
        state.nudge_lyrics_offset(-20000);
        assert_eq!(state.visible_lyric_lines(), vec![(0, "Override line".to_string())]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
