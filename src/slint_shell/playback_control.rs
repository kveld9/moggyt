//! Playback control, track completion, queue progression, volume, seeking,
//! and presence updates on SlintAppState.
//!
//! Hexagonal invariant: non-blocking audio port calls and presence sync without UI event loop stalls.

use std::time::Instant;

use slint::{ModelRc, VecModel};

use crate::domain::Track;
use crate::i18n::tr;
use crate::infra::{AudioBackend, AudioQuality, Catalog};
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::format::current_unix_secs;
use crate::slint_shell::platform::rgba_to_image;
use crate::slint_shell::playback_worker::PlayTrackResult;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::{LyricLineData, MoggytWindow};

/// Playback start record: when the track started and which track it was.
#[derive(Debug, Clone)]
pub struct StartedPlay {
    pub instant: Instant,
    pub unix_secs: u64,
    pub track: Track,
}

impl StartedPlay {
    /// Captures the start instant and unix epoch timestamp for the given track.
    pub fn now(track: Track) -> Self {
        Self { instant: Instant::now(), unix_secs: current_unix_secs(), track }
    }
}

fn dispatch_audio_play(audio: &mut dyn AudioBackend, id: &str, crossfade_seconds: u32) {
    if audio.is_playing() && crossfade_seconds > 0 {
        audio.play_crossfade(id, std::time::Duration::from_secs(crossfade_seconds as u64));
    } else {
        audio.play(id);
    }
}

impl SlintAppState {
    fn notify_now_playing(&self, artist: &str, title: &str) {
        if let Ok(mut s) = self.scrobbler.lock() {
            s.now_playing(artist, title);
        }
        if let Some(sec) = &self.secondary_scrobbler {
            if let Ok(mut s) = sec.lock() {
                s.now_playing(artist, title);
            }
        }
    }

    /// Advances or retreats queue cursor in-memory to target index without triggering playback.
    pub fn play_track_at_index_in_memory(&mut self, index: usize) {
        if index >= self.queue.len() {
            return;
        }
        while self.queue.cursor() < index {
            self.queue.advance();
        }
        while self.queue.cursor() > index {
            self.queue.retreat();
        }
    }

    /// Advances or retreats queue cursor to target index and starts playback quietly.
    pub fn play_track_at_index(&mut self, index: usize) {
        self.play_track_at_index_in_memory(index);
        self.play_current_quiet();
    }

    /// Plays the track currently under the cursor without changing navigation page.
    pub fn play_current_quiet(&mut self) {
        let Some(track) = self.queue.current().cloned() else {
            return;
        };
        let (id, artist, title) = (track.id.clone(), track.artist.clone(), track.title.clone());
        if let Ok(mut a) = self.audio.lock() {
            dispatch_audio_play(&mut **a, &id, self.crossfade_seconds);
        }
        self.started_at = Some(StartedPlay::now(track));
        self.notify_now_playing(&artist, &title);
        self.last_skip_target = None;
        self.cached_skip_ranges = None;
        self.sync_presence_track(&artist, &title, true);
        self.refresh_lyrics();
    }

    /// Toggles playback: pauses if playing, resumes if requested, or starts head.
    pub fn toggle_play(&mut self) {
        let is_playing = self.audio.lock().map(|a| a.is_playing()).unwrap_or(false);
        let has_last = self.audio.lock().map(|a| a.last_requested().is_some()).unwrap_or(false);
        if is_playing {
            if let Ok(mut a) = self.audio.lock() {
                a.pause();
            }
            self.sync_presence_playing(false);
        } else if has_last {
            if let Ok(mut a) = self.audio.lock() {
                a.resume_play();
            }
            self.sync_presence_playing(true);
        } else {
            self.play_current_quiet();
        }
    }

    /// Advances cursor to next track, scrobbling the completed track if eligible.
    pub fn next_track(&mut self) {
        self.maybe_scrobble_current();
        self.queue.advance();
        self.play_current_quiet();
    }

    /// Retreats cursor to previous track, scrobbling the completed track if eligible.
    pub fn prev_track(&mut self) {
        self.maybe_scrobble_current();
        self.queue.retreat();
        self.play_current_quiet();
    }

    /// Adjusts position relative to current playback point, clamped at zero.
    pub fn seek_by(&mut self, delta_secs: i64) {
        if let Ok(mut a) = self.audio.lock() {
            let current = a.position_secs() as i64;
            let target = current.saturating_add(delta_secs).max(0) as u64;
            a.seek(target);
        }
    }

    /// Updates playback volume level and persists preference to disk.
    pub fn set_volume(&mut self, volume: u8) {
        if self.sleep_fade_base_volume.is_some() {
            self.sleep_fade_base_volume = Some(volume);
        }
        if let Ok(mut a) = self.audio.lock() {
            a.set_volume(volume);
        }
        let mut settings = self.settings_store.load();
        settings.volume = volume;
        let _ = self.settings_store.save(&settings);
    }

    /// Updates audio quality preference and persists preference to disk.
    pub fn set_audio_quality(&mut self, quality: AudioQuality) {
        if let Ok(mut a) = self.audio.lock() {
            a.set_quality(quality);
        }
        let mut settings = self.settings_store.load();
        settings.quality = quality;
        let _ = self.settings_store.save(&settings);
    }

    /// Updates playback speed factor and persists preference to disk.
    pub fn set_playback_speed(&mut self, speed: f32) {
        let actual = if let Ok(mut a) = self.audio.lock() {
            a.set_speed(speed);
            a.speed()
        } else {
            1.0
        };
        let mut settings = self.settings_store.load();
        settings.playback_speed = actual;
        let _ = self.settings_store.save(&settings);
    }

    /// Toggles the songs-only preference and persists preference to disk.
    pub fn toggle_songs_only(&mut self) {
        self.songs_only = !self.songs_only;
        let mut settings = self.settings_store.load();
        settings.songs_only = self.songs_only;
        let _ = self.settings_store.save(&settings);
    }

    /// Evaluates auto-skip intervals for active track, seeking past matched range.
    pub fn check_auto_skip(&mut self) {
        let Some(track) = self.queue.current() else {
            self.last_skip_target = None;
            self.cached_skip_ranges = None;
            return;
        };
        let Ok(mut audio) = self.audio.try_lock() else {
            return;
        };
        let current_pos = u32::try_from(audio.position_secs()).unwrap_or(u32::MAX);
        if self
            .last_skip_target
            .as_ref()
            .is_some_and(|(id, t)| id != &track.id || current_pos >= *t)
        {
            self.last_skip_target = None;
        }
        if !self.cached_skip_ranges.as_ref().is_some_and(|(id, _)| id == &track.id) {
            let ranges = self.skip_store.load().remove(&track.id);
            self.cached_skip_ranges = Some((track.id.clone(), ranges));
        }
        let Some((_, Some(ranges))) = self.cached_skip_ranges.as_ref() else {
            return;
        };
        let Some(target) = ranges.jump_target(current_pos) else {
            return;
        };
        let already_skipped =
            self.last_skip_target.as_ref().is_some_and(|(id, t)| id == &track.id && *t == target);
        if target > current_pos && !already_skipped {
            self.last_skip_target = Some((track.id.clone(), target));
            audio.seek(target as u64);
        }
    }

    fn append_autoplay_radio(&mut self, current_id: &str) {
        let mut recs = self.catalog.lock().map(|c| c.radio(current_id)).unwrap_or_default();
        if self.songs_only {
            recs = crate::domain::retain_songs(recs);
        }
        let worker_tx = self.worker_tx.clone();
        let generation = self.play_generation;
        for track in recs {
            if !self.queue.tracks().iter().any(|t| t.id == track.id) {
                if let Some(ref tx) = worker_tx {
                    request_queue_track_cover(
                        track.artwork.as_deref(),
                        generation,
                        &self.search_covers,
                        tx,
                    );
                }
                self.queue.push(track);
            }
        }
    }

    /// Advances queue or appends radio recommendations when active track ends or reaches crossfade boundary.
    pub fn continue_autoplay_if_ended(&mut self) {
        let Ok(audio) = self.audio.try_lock() else {
            return;
        };
        if self.queue.is_empty() {
            return;
        }

        if self.crossfade_seconds > 0 && audio.is_playing() {
            let Some(current) = self.queue.current() else {
                return;
            };
            let Some(dur) = current.duration_secs else {
                return;
            };
            let pos = audio.position_secs();
            let dur = dur as u64;
            if dur > pos && (dur - pos) <= self.crossfade_seconds as u64 {
                if self.last_crossfaded_track.as_deref() == Some(&current.id) {
                    return;
                }
                let current_id = current.id.clone();
                let has_next = self.queue.peek_next().is_some();
                if !has_next && !self.autoplay {
                    return;
                }
                self.last_crossfaded_track = Some(current_id.clone());
                drop(audio);
                if !has_next && self.autoplay {
                    self.append_autoplay_radio(&current_id);
                }
                self.next_track();
            }
            return;
        }

        if !self.autoplay || audio.is_playing() {
            return;
        }
        let (Some(current), Some(last)) = (self.queue.current(), self.queue.tracks().last()) else {
            return;
        };
        if !std::ptr::eq(current, last) {
            return;
        }
        let Some(dur) = current.duration_secs else {
            return;
        };
        if audio.position_secs() < dur as u64 {
            return;
        }
        let current_id = current.id.clone();
        drop(audio);
        self.append_autoplay_radio(&current_id);
        self.queue.advance();
        self.play_current_quiet();
    }
}

/// Applies completed playback worker results back to the Slint window on the event loop.
pub fn apply_play_track_completion(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    res: PlayTrackResult,
) {
    if !res.success {
        window.set_active_track_title(tr(state.locale, "Failed to play track").into());
        window.set_is_playing(false);
        return;
    }
    window.set_is_playing(true);
    state.lyrics_lines = res.lyrics_lines;
    state.lyrics_key = Some((res.artist.clone(), res.title.clone()));

    let line_data: Vec<LyricLineData> = state
        .lyrics_lines
        .iter()
        .enumerate()
        .map(|(i, l)| LyricLineData { index: i as i32, text: l.text.clone().into() })
        .collect();
    window.set_lyrics_lines(ModelRc::new(VecModel::from(line_data)));
    window.set_active_lyric_index(state.active_lyric_index());

    if let Some(cover) = res.cover_data {
        if let Some(img) = rgba_to_image(&cover.rgba, cover.width, cover.height) {
            state.current_cover_image = Some(img.clone());
            state.now_playing_cover = Some(img.clone());
            state.search_covers.insert(cover.url, img.clone());
            window.set_cover_image(img.clone());
            window.set_now_playing_cover(img);
            window.set_has_cover(true);
        } else {
            state.current_cover_image = None;
            state.now_playing_cover = None;
            window.set_has_cover(false);
            window.set_now_playing_cover(slint::Image::default());
        }
    } else {
        state.current_cover_image = None;
        state.now_playing_cover = None;
        window.set_has_cover(false);
        window.set_now_playing_cover(slint::Image::default());
    }

    state.sync_presence_track(&res.artist, &res.title, true);
    state.started_at =
        state.queue.tracks().iter().find(|t| t.id == res.track_id).cloned().map(StartedPlay::now);
    state.last_skip_target = None;
    state.cached_skip_ranges = None;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    use crate::domain::{SkipRanges, SkipSegment, Track};
    use crate::infra::SkipStore;
    use crate::slint_shell::test_support::test_wired;

    #[derive(Debug)]
    struct CountingSkipStore {
        load_count: Arc<AtomicUsize>,
        ranges: HashMap<String, SkipRanges>,
    }

    impl SkipStore for CountingSkipStore {
        fn load(&self) -> HashMap<String, SkipRanges> {
            self.load_count.fetch_add(1, Ordering::SeqCst);
            self.ranges.clone()
        }

        fn save(&self, _map: &HashMap<String, SkipRanges>) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_playback_controls_toggle_and_queue_navigation() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));

        state.play_current_quiet();
        assert!(state.audio.lock().unwrap().is_playing());
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));

        state.toggle_play();
        assert!(!state.audio.lock().unwrap().is_playing());

        state.toggle_play();
        assert!(state.audio.lock().unwrap().is_playing());

        state.next_track();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));

        state.prev_track();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));
    }

    #[test]
    fn test_seek_and_volume_controls() {
        let mut state = test_wired();
        state.set_volume(75);
        assert_eq!(state.audio.lock().unwrap().volume(), 75);

        state.seek_by(15);
        assert_eq!(state.audio.lock().unwrap().position_secs(), 15);

        state.seek_by(-50);
        assert_eq!(state.audio.lock().unwrap().position_secs(), 0);
    }

    #[test]
    fn test_auto_skip_evaluation() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        let mut ranges = crate::domain::SkipRanges::new();
        ranges.insert(SkipSegment::new(10, 20).unwrap());
        let mut map = std::collections::HashMap::new();
        map.insert("t1".to_string(), ranges);
        let _ = state.skip_store.save(&map);

        state.seek_by(12);
        state.check_auto_skip();
        assert_eq!(state.audio.lock().unwrap().position_secs(), 20);
    }

    #[test]
    fn test_auto_skip_repeated_checks_load_once() {
        let mut state = test_wired();
        let counter = Arc::new(AtomicUsize::new(0));
        let mut ranges = SkipRanges::new();
        ranges.insert(SkipSegment::new(10, 20).unwrap());
        let mut map = HashMap::new();
        map.insert("t1".to_string(), ranges);

        state.skip_store =
            Box::new(CountingSkipStore { load_count: Arc::clone(&counter), ranges: map });

        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.check_auto_skip();
        state.check_auto_skip();
        state.check_auto_skip();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_auto_skip_switching_track_loads_again() {
        let mut state = test_wired();
        let counter = Arc::new(AtomicUsize::new(0));
        let mut ranges1 = SkipRanges::new();
        ranges1.insert(SkipSegment::new(10, 20).unwrap());
        let mut ranges2 = SkipRanges::new();
        ranges2.insert(SkipSegment::new(30, 40).unwrap());
        let mut map = HashMap::new();
        map.insert("t1".to_string(), ranges1);
        map.insert("t2".to_string(), ranges2);

        state.skip_store =
            Box::new(CountingSkipStore { load_count: Arc::clone(&counter), ranges: map });

        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.check_auto_skip();
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));
        state.queue.advance();
        state.check_auto_skip();
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_cycle_playback_speed_in_state() {
        let mut state = test_wired();
        assert_eq!(state.audio.lock().unwrap().speed(), 1.0);

        let next = state.cycle_playback_speed();
        assert_eq!(next, 1.25);
        assert_eq!(state.audio.lock().unwrap().speed(), 1.25);
        assert_eq!(state.settings_store.load().playback_speed, 1.25);

        let next2 = state.cycle_playback_speed();
        assert_eq!(next2, 1.5);
        assert_eq!(state.audio.lock().unwrap().speed(), 1.5);
    }

    #[test]
    fn test_set_audio_quality_in_state() {
        let mut state = test_wired();
        assert_eq!(state.audio.lock().unwrap().quality(), AudioQuality::Standard);
        assert_eq!(state.settings_store.load().quality, AudioQuality::Standard);

        state.set_audio_quality(AudioQuality::High);
        assert_eq!(state.audio.lock().unwrap().quality(), AudioQuality::High);
        assert_eq!(state.settings_store.load().quality, AudioQuality::High);
    }

    #[test]
    fn test_toggle_songs_only_flips_and_persists() {
        let mut state = test_wired();
        assert!(!state.songs_only);
        assert!(!state.settings_store.load().songs_only);

        state.toggle_songs_only();
        assert!(state.songs_only);
        assert!(state.settings_store.load().songs_only);

        state.toggle_songs_only();
        assert!(!state.songs_only);
        assert!(!state.settings_store.load().songs_only);
    }

    #[derive(Debug, Default)]
    struct RadioVideoCatalog;

    impl Catalog for RadioVideoCatalog {
        fn search(&self, _query: &str) -> Vec<Track> {
            Vec::new()
        }
        fn search_filtered(
            &self,
            _query: &crate::domain::SearchQuery,
        ) -> Vec<crate::domain::SearchResult> {
            Vec::new()
        }
        fn radio(&self, _seed_track_id: &str) -> Vec<Track> {
            vec![
                Track::new("vid-omv", "OMV Track", "Artist")
                    .with_duration(180)
                    .with_video_type(crate::domain::MusicVideoType::Omv),
                Track::new("song-atv", "ATV Track", "Artist")
                    .with_duration(200)
                    .with_video_type(crate::domain::MusicVideoType::Atv),
            ]
        }
        fn home_shelves(&self) -> Vec<crate::domain::Shelf> {
            Vec::new()
        }
        fn playlists(&self) -> Vec<crate::domain::Playlist> {
            Vec::new()
        }
        fn saved_albums(&self) -> Vec<crate::domain::Album> {
            Vec::new()
        }
        fn detail(
            &self,
            _browse_id: &str,
            _kind: crate::domain::DetailKind,
        ) -> Option<crate::domain::Detail> {
            None
        }
    }

    #[test]
    fn test_continue_autoplay_if_ended_filters_video_recommendations_when_songs_only() {
        let mut state = test_wired();
        state.catalog = Arc::new(Mutex::new(Box::new(RadioVideoCatalog)));
        state.autoplay = true;
        state.songs_only = true;

        let seed = Track::new("seed", "Seed", "Artist").with_duration(100);
        state.queue.push(seed);
        state.play_track_at_index_in_memory(0);
        state.audio.lock().unwrap().seek(100);

        state.continue_autoplay_if_ended();

        assert_eq!(state.queue.len(), 2);
        assert_eq!(state.queue.tracks()[1].id, "song-atv");
    }

    #[test]
    fn test_continue_autoplay_crossfade_timing_and_advance() {
        let mut state = test_wired();
        state.crossfade_seconds = 5;

        let t1 = Track::new("cf-1", "Track 1", "Artist").with_duration(100);
        let t2 = Track::new("cf-2", "Track 2", "Artist").with_duration(120);
        state.queue.push(t1);
        state.queue.push(t2);

        state.play_track_at_index(0);
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("cf-1"));

        // At 94s (remaining 6s > crossfade_seconds 5): should NOT crossfade yet
        state.audio.lock().unwrap().seek(94);
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("cf-1"));

        // At 96s (remaining 4s <= crossfade_seconds 5): triggers crossfade advance to track 2
        state.audio.lock().unwrap().seek(96);
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 1);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("cf-2"));
        assert_eq!(state.last_crossfaded_track, Some("cf-1".to_string()));

        // Repeated call should NOT re-trigger crossfade for cf-1
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 1);
    }

    #[test]
    fn test_continue_autoplay_crossfade_disabled_when_zero() {
        let mut state = test_wired();
        state.crossfade_seconds = 0;

        let t1 = Track::new("no-cf-1", "Track 1", "Artist").with_duration(100);
        let t2 = Track::new("no-cf-2", "Track 2", "Artist").with_duration(120);
        state.queue.push(t1);
        state.queue.push(t2);

        state.play_track_at_index(0);
        // Seek to 98s (2s left): with crossfade_seconds == 0, audio is still playing so no advance
        state.audio.lock().unwrap().seek(98);
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("no-cf-1"));
    }

    #[test]
    fn test_play_current_quiet_uses_crossfade_when_playing_and_configured() {
        let mut state = test_wired();
        state.crossfade_seconds = 8;

        let t1 = Track::new("pq-1", "Track 1", "Artist").with_duration(100);
        let t2 = Track::new("pq-2", "Track 2", "Artist").with_duration(120);
        state.queue.push(t1);
        state.queue.push(t2);

        // First play: audio is not playing yet, so immediate play
        state.play_track_at_index(0);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("pq-1"));

        // Advance cursor in memory and play while audio is playing
        state.queue.advance();
        state.play_current_quiet();
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("pq-2"));
    }

    #[test]
    fn test_set_crossfade_seconds_clamps_and_persists() {
        let mut state = test_wired();
        assert_eq!(state.crossfade_seconds, 0);

        state.set_crossfade_seconds(10);
        assert_eq!(state.crossfade_seconds, 10);
        assert_eq!(state.settings_store.load().crossfade_seconds, 10);

        state.set_crossfade_seconds(50);
        assert_eq!(state.crossfade_seconds, crate::infra::MAX_CROSSFADE_SECONDS);
        assert_eq!(
            state.settings_store.load().crossfade_seconds,
            crate::infra::MAX_CROSSFADE_SECONDS
        );
    }
}
