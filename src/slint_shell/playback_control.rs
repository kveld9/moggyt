//! Playback control, track completion, queue progression, volume, seeking,
//! and presence updates on SlintAppState.
//!
//! Hexagonal invariant: non-blocking audio port calls and presence sync without UI event loop stalls.

use std::sync::Arc;
use std::time::Instant;

use slint::{ModelRc, VecModel};

use crate::i18n::tr;
use crate::infra::{AudioBackend, Catalog, Presence, should_scrobble};
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::format::current_unix_secs;
use crate::slint_shell::platform::rgba_to_image;
use crate::slint_shell::playback_worker::PlayTrackResult;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::{LyricLineData, MoggytWindow};

impl SlintAppState {
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
        let Some(track) = self.queue.current() else {
            return;
        };
        let id = track.id.clone();
        let artist = track.artist.clone();
        let title = track.title.clone();
        if let Ok(mut a) = self.audio.lock() {
            a.play(&id);
        }
        self.started_at = Some((Instant::now(), current_unix_secs()));
        if let Ok(mut s) = self.scrobbler.lock() {
            s.now_playing(&artist, &title);
        }
        if let Some(sec) = &self.secondary_scrobbler {
            if let Ok(mut s) = sec.lock() {
                s.now_playing(&artist, &title);
            }
        }
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
        if let Ok(mut a) = self.audio.lock() {
            a.set_volume(volume);
        }
        let mut settings = self.settings_store.load();
        settings.volume = volume;
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

    /// Advances queue or appends radio recommendations when active track ends.
    pub fn continue_autoplay_if_ended(&mut self) {
        let Ok(audio) = self.audio.try_lock() else {
            return;
        };
        if !self.autoplay || self.queue.is_empty() || audio.is_playing() {
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
        let recs = self.catalog.lock().map(|c| c.radio(&current_id)).unwrap_or_default();
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
        self.queue.advance();
        self.play_current_quiet();
    }

    /// Submits scrobble record to active scrobbler if playback elapsed threshold was met.
    pub fn maybe_scrobble_current(&mut self) {
        let Some((started_instant, started_unix)) = self.started_at else {
            return;
        };
        let elapsed = started_instant.elapsed().as_secs();
        let Some(current) = self.queue.current() else {
            return;
        };
        let duration = current.duration_secs.map(u64::from);
        if should_scrobble(elapsed, duration) {
            if let Ok(mut s) = self.scrobbler.lock() {
                s.scrobble(&current.artist, &current.title, started_unix);
            }
            if let Some(sec) = &self.secondary_scrobbler {
                if let Ok(mut s) = sec.lock() {
                    s.scrobble(&current.artist, &current.title, started_unix);
                }
            }
        }
        self.started_at = None;
    }

    /// Synchronizes active track metadata with desktop presence and Discord presence.
    pub fn sync_presence_track(&mut self, artist: &str, title: &str, is_playing: bool) {
        let key = (artist.to_string(), title.to_string());
        if self.last_presence_key.as_ref() != Some(&key) {
            self.presence.set_track(artist, title);
            self.discord.set_track(artist, title);
            self.last_presence_key = Some(key);
        }
        self.presence.set_playing(is_playing);
    }

    /// Synchronizes playback status with desktop presence.
    pub fn sync_presence_playing(&mut self, is_playing: bool) {
        self.presence.set_playing(is_playing);
    }

    /// Evaluates sleep timer duration and pauses audio playback on expiration.
    pub fn poll_sleep_timer(&mut self) {
        let audio_arc = Arc::clone(&self.audio);
        let Ok(mut audio) = audio_arc.try_lock() else {
            return;
        };
        if !self.sleep_timer.is_armed() || !audio.is_playing() {
            return;
        }
        let elapsed = audio.position_secs();
        let effective = self
            .sleep_timer_accumulated
            .saturating_add(elapsed.saturating_sub(self.sleep_timer_base_offset));
        if self.sleep_timer.poll_elapsed(effective) {
            audio.pause();
            drop(audio);
            self.sync_presence_playing(false);
        }
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
    state.started_at = Some((Instant::now(), current_unix_secs()));
    state.last_skip_target = None;
    state.cached_skip_ranges = None;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
    fn test_sleep_timer_expiry() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.play_current_quiet();
        state.sleep_timer.set_preset(crate::domain::TimerPreset::Minutes15);
        assert!(state.sleep_timer.is_armed());

        state.seek_by(900);
        state.poll_sleep_timer();
        assert!(!state.audio.lock().unwrap().is_playing());
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
}
