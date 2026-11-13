//! Playback control, track completion, queue progression, volume, seeking,
//! and presence updates on SlintAppState.
//!
//! Hexagonal invariant: non-blocking audio port calls and presence sync without UI event loop stalls.

use std::time::{Duration, Instant};

use slint::{ComponentHandle, ModelRc, VecModel};

use super::sync::set_lyrics_highlight;
use crate::domain::{PlaybackPhase, Track};
use crate::infra::{AudioBackend, AudioQuality, Catalog};
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::format::current_unix_secs;
use crate::slint_shell::platform::rgba_to_image;
use crate::slint_shell::playback_worker::PlayTrackResult;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::sync::observe_playback_phase;
use crate::slint_shell::{LyricLineData, MoggytWindow, PlayerUi};

/// Playback start record: when the track started and which track it was.
#[derive(Debug, Clone)]
pub struct StartedPlay {
    /// Accumulated audible listening time before the current resume.
    pub listened: std::time::Duration,
    /// Time of the current resume; `None` while paused or after freezing.
    pub resumed_at: Option<Instant>,
    pub unix_secs: u64,
    pub track: Track,
}

impl StartedPlay {
    /// Captures the start instant and unix epoch timestamp for the given track.
    pub fn now(track: Track) -> Self {
        Self {
            listened: Duration::ZERO,
            resumed_at: Some(Instant::now()),
            unix_secs: current_unix_secs(),
            track,
        }
    }

    /// Freezes listened-time accumulation; idempotent when already paused.
    pub fn on_pause(&mut self) {
        if let Some(resumed) = self.resumed_at.take() {
            self.listened += resumed.elapsed();
        }
    }

    /// Restarts listened-time accumulation; idempotent when already running.
    pub fn on_resume(&mut self) {
        if self.resumed_at.is_none() {
            self.resumed_at = Some(Instant::now());
        }
    }

    /// Total audible listening duration accumulated so far.
    pub fn listened(&self) -> Duration {
        match self.resumed_at {
            Some(resumed) => self.listened + resumed.elapsed(),
            None => self.listened,
        }
    }
}

impl SlintAppState {
    /// Returns whether a worker play result matches the current play generation.
    pub fn should_apply_play_result(&self, generation: u64) -> bool {
        self.play_generation == generation
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

    /// Advances or retreats queue cursor to target index and requests playback.
    pub fn play_track_at_index(&mut self, index: usize) {
        self.play_track_at_index_in_memory(index);
        self.request_current_play();
    }

    /// Toggles playback: pauses if playing, resumes a paused track, or starts the
    /// current track (which also restarts it after the stream drained).
    pub fn toggle_play(&mut self) {
        let is_playing = self.audio.lock().map(|a| a.is_playing()).unwrap_or(false);
        let has_last = self.audio.lock().map(|a| a.last_requested().is_some()).unwrap_or(false);
        let is_empty = self.audio.lock().map(|a| a.is_empty()).unwrap_or(true);
        if is_playing {
            if let Ok(mut a) = self.audio.lock() {
                a.pause();
            }
            self.pause_play_ledger();
            self.set_playback_phase(PlaybackPhase::Paused);
            self.sync_presence_playing(false);
        } else if has_last && !is_empty {
            if let Ok(mut a) = self.audio.lock() {
                a.resume_play();
            }
            self.resume_play_ledger();
            self.set_playback_phase(PlaybackPhase::Resolving);
            self.sync_presence_playing(true);
        } else {
            self.request_current_play();
        }
    }

    /// Advances cursor to next track, scrobbling the completed track if eligible.
    pub fn next_track(&mut self) {
        self.queue.advance();
        self.request_current_play();
    }

    /// Retreats cursor to previous track, scrobbling the completed track if eligible.
    pub fn prev_track(&mut self) {
        self.queue.retreat();
        self.request_current_play();
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

    /// Toggles autoplay continuation and persists the preference to disk.
    pub fn toggle_autoplay(&mut self) {
        self.autoplay = !self.autoplay;
        let mut settings = self.settings_store.load();
        settings.autoplay_enabled = self.autoplay;
        let _ = self.settings_store.save(&settings);
    }

    /// Empties the queue, stops playback, and persists the empty queue.
    pub fn clear_queue(&mut self) {
        if let Ok(mut audio) = self.audio.lock() {
            audio.pause();
        }
        self.pause_play_ledger();
        self.queue.clear();
        self.set_playback_phase(PlaybackPhase::Empty);
        self.sync_presence_playing(false);
        let _ = self.queue_store.save(&self.queue);
    }

    /// Evaluates auto-skip intervals for active track, seeking past matched range.
    pub fn check_auto_skip(&mut self) {
        let Some(track) = self.queue.current().cloned() else {
            self.last_skip_target = None;
            self.cached_skip_ranges = None;
            return;
        };
        self.request_sponsor_ranges(&track);
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
            let local = self.skip_store.load().remove(&track.id);
            let ranges = local.or_else(|| self.available_sponsor_ranges(&track.id).cloned());
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

    fn enqueue_radio_recommendations(&mut self, recs: Vec<Track>) {
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

    fn fetch_autoplay_radio(&self, current_id: &str) -> Option<Vec<Track>> {
        let Ok(catalog) = self.catalog.lock() else {
            tracing::warn!("failed to fetch autoplay radio: catalog lock poisoned");
            return None;
        };
        match catalog.radio(current_id) {
            Ok(tracks) => Some(tracks),
            Err(err) => {
                tracing::warn!("failed to fetch autoplay radio: {err}");
                None
            }
        }
    }

    fn append_autoplay_radio(&mut self, current_id: &str) {
        let Some(recs) = self.fetch_autoplay_radio(current_id) else {
            return;
        };
        let filtered = if self.songs_only { crate::domain::retain_songs(recs) } else { recs };
        self.enqueue_radio_recommendations(filtered);
    }

    fn track_remaining_secs(&self, audio: &dyn AudioBackend) -> Option<u64> {
        let dur = self.queue.current()?.duration_secs? as u64;
        let pos = audio.position_secs();
        (dur > pos).then(|| dur - pos)
    }

    fn can_crossfade_track(&self, track_id: &str) -> bool {
        if self.last_crossfaded_track.as_deref() == Some(track_id) {
            return false;
        }
        self.queue.peek_next().is_some() || self.autoplay
    }

    fn crossfade_candidate(&self, audio: &dyn AudioBackend) -> Option<(String, bool)> {
        let remaining = self.track_remaining_secs(audio)?;
        if remaining > self.crossfade_seconds as u64 {
            return None;
        }
        let current_id = &self.queue.current()?.id;
        if !self.can_crossfade_track(current_id) {
            return None;
        }
        Some((current_id.clone(), self.queue.peek_next().is_some()))
    }

    fn handle_playing_crossfade(&self, audio: &dyn AudioBackend) -> Option<(String, bool)> {
        if self.crossfade_seconds == 0 {
            return None;
        }
        self.crossfade_candidate(audio)
    }

    fn execute_crossfade(&mut self, current_id: String, has_next: bool) {
        self.last_crossfaded_track = Some(current_id.clone());
        if !has_next && self.autoplay {
            self.append_autoplay_radio(&current_id);
        }
        self.next_track();
    }

    fn advance_ended_playback(&mut self) {
        if self.playback_phase != PlaybackPhase::Ended {
            return;
        }
        if self.queue.peek_next().is_some() {
            self.next_track();
            return;
        }
        if !self.autoplay {
            self.maybe_scrobble_current();
            return;
        }
        let Some(current) = self.queue.current() else {
            return;
        };
        let current_id = current.id.clone();
        self.append_autoplay_radio(&current_id);
        self.queue.advance();
        self.request_current_play();
    }

    /// Advances queue or appends radio recommendations when active track ends or reaches crossfade boundary.
    ///
    /// Pausing never advances playback; only the `Ended` phase advances to the next track.
    pub fn continue_autoplay_if_ended(&mut self) {
        observe_playback_phase(self);
        if self.queue.is_empty() {
            return;
        }
        if self.playback_phase == PlaybackPhase::Ended {
            self.advance_ended_playback();
            return;
        }
        if self.playback_phase == PlaybackPhase::Playing {
            let candidate = if let Ok(audio) = self.audio.try_lock() {
                self.handle_playing_crossfade(&**audio)
            } else {
                None
            };
            if let Some((id, has_next)) = candidate {
                self.execute_crossfade(id, has_next);
            }
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
        super::playback_failure::notify_playback_failure(window, state, &res.title);
        window.global::<PlayerUi>().set_is_playing(false);
        return;
    }
    window.global::<PlayerUi>().set_is_playing(true);
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
    set_lyrics_highlight(window, String::new(), String::new());

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
    use std::sync::{Arc, Mutex};

    use super::*;

    use crate::domain::{PlaybackPhase, Track};
    use crate::infra::CatalogError;
    use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
    use crate::slint_shell::test_support::{EndedAudio, MockLibraryCatalog, test_wired};

    #[test]
    fn test_playback_controls_toggle_and_queue_navigation() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));

        state.request_current_play();
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

    #[test]
    fn test_toggle_autoplay_flips_and_persists() {
        let mut state = test_wired();
        assert!(!state.autoplay);
        assert!(!state.settings_store.load().autoplay_enabled);

        state.toggle_autoplay();
        assert!(state.autoplay);
        assert!(state.settings_store.load().autoplay_enabled);

        state.toggle_autoplay();
        assert!(!state.autoplay);
        assert!(!state.settings_store.load().autoplay_enabled);
    }

    #[test]
    fn test_clear_queue_empties_stops_and_persists() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
        state.request_current_play();
        assert!(state.audio.lock().unwrap().is_playing());

        state.clear_queue();
        assert!(state.queue.is_empty());
        assert!(!state.audio.lock().unwrap().is_playing());
        assert!(state.queue_store.load().is_empty());
        assert_eq!(state.playback_phase, PlaybackPhase::Empty);
    }

    #[test]
    fn test_continue_autoplay_if_ended_filters_video_recommendations_when_songs_only() {
        let mut state = test_wired();
        let catalog = MockLibraryCatalog::default().with_radio(vec![
            Track::new("vid-omv", "OMV Track", "Artist")
                .with_duration(180)
                .with_video_type(crate::domain::MusicVideoType::Omv),
            Track::new("song-atv", "ATV Track", "Artist")
                .with_duration(200)
                .with_video_type(crate::domain::MusicVideoType::Atv),
        ]);
        state.catalog = Arc::new(Mutex::new(Box::new(catalog)));
        state.autoplay = true;
        state.songs_only = true;
        let seed = Track::new("seed", "Seed", "Artist").with_duration(100);
        state.queue.push(seed);
        state.play_track_at_index_in_memory(0);
        state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("seed", 100))));
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 2);
        assert_eq!(state.queue.tracks()[1].id, "song-atv");
    }

    #[test]
    fn radio_failure_does_not_append_tracks() {
        let mut state = test_wired();
        let catalog = MockLibraryCatalog::default().with_radio_result(Err(CatalogError::Transport));
        state.catalog = Arc::new(Mutex::new(Box::new(catalog)));
        state.autoplay = true;
        let seed = Track::new("seed", "Seed", "Artist").with_duration(100);
        state.queue.push(seed);
        assert_eq!(state.queue.len(), 1);
        state.append_autoplay_radio("seed");
        assert_eq!(state.queue.len(), 1);
    }

    #[test]
    fn test_continue_autoplay_crossfade_timing_and_advance() {
        let mut state = test_wired();
        state.crossfade_seconds = 5;

        state.queue.push(Track::new("cf-1", "Track 1", "Artist").with_duration(100));
        state.queue.push(Track::new("cf-2", "Track 2", "Artist").with_duration(120));

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

        state.queue.push(Track::new("no-cf-1", "Track 1", "Artist").with_duration(100));
        state.queue.push(Track::new("no-cf-2", "Track 2", "Artist").with_duration(120));

        state.play_track_at_index(0);
        // Seek to 98s (2s left): with crossfade_seconds == 0, audio is still playing so no advance
        state.audio.lock().unwrap().seek(98);
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("no-cf-1"));
    }

    #[test]
    fn request_current_play_uses_crossfade_when_playing_and_configured() {
        let mut state = test_wired();
        state.crossfade_seconds = 8;

        state.queue.push(Track::new("pq-1", "Track 1", "Artist").with_duration(100));
        state.queue.push(Track::new("pq-2", "Track 2", "Artist").with_duration(120));

        // First play: audio is not playing yet, so immediate play
        state.play_track_at_index(0);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("pq-1"));

        // Advance cursor in memory and play while audio is playing
        state.queue.advance();
        state.request_current_play();
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

    #[test]
    fn test_continue_autoplay_advances_queue_when_crossfade_zero_and_ended() {
        let mut state = test_wired();
        state.crossfade_seconds = 0;

        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));

        state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("t1", 180))));
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("t1"));

        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 1);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("t2"));
        assert!(state.audio.lock().unwrap().is_playing());
    }

    #[test]
    fn test_continue_autoplay_no_advance_when_last_track_and_autoplay_disabled() {
        let mut state = test_wired();
        state.crossfade_seconds = 0;
        state.autoplay = false;

        let t1 = Track::new("t1", "Track 1", "Artist 1").with_duration(180);
        state.queue.push(t1);

        state.play_track_at_index(0);
        assert_eq!(state.queue.cursor(), 0);

        state.audio.lock().unwrap().pause();
        assert!(!state.audio.lock().unwrap().is_playing());

        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.queue.len(), 1);
        assert!(!state.audio.lock().unwrap().is_playing());
    }

    #[test]
    fn pause_mid_queue_does_not_advance() {
        let mut state = test_wired();
        state.queue.push(Track::new("a", "Track A", "Artist").with_duration(180));
        state.queue.push(Track::new("b", "Track B", "Artist").with_duration(200));
        state.play_track_at_index(0);
        state.toggle_play();
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("a"));
        assert_eq!(state.playback_phase, PlaybackPhase::Paused);
    }

    #[test]
    fn ended_advances_to_next() {
        let mut state = test_wired();
        state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("a", 180))));
        state.queue.push(Track::new("a", "Track A", "Artist").with_duration(180));
        state.queue.push(Track::new("b", "Track B", "Artist").with_duration(200));
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 1);
        assert_eq!(state.audio.lock().unwrap().last_requested(), Some("b"));
    }

    #[test]
    fn ended_last_track_autoplay_off_stays() {
        let mut state = test_wired();
        state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("a", 180))));
        state.queue.push(Track::new("a", "Track A", "Artist").with_duration(180));
        state.autoplay = false;
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.playback_phase, PlaybackPhase::Ended);
    }

    #[test]
    fn restored_queue_startup_does_not_advance() {
        let mut state = test_wired();
        state.queue.push(Track::new("a", "Track A", "Artist").with_duration(180));
        state.queue.push(Track::new("b", "Track B", "Artist").with_duration(200));
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.cursor(), 0);
        assert_eq!(state.playback_phase, PlaybackPhase::Empty);
    }

    #[test]
    fn generation_bumped_on_every_transition() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut state = test_wired();
        state.worker_tx = Some(tx);
        state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2"));
        state.next_track();
        let g1 = state.play_generation;
        state.prev_track();
        let g2 = state.play_generation;
        // state.drain_media_keys is not drivable without presence, so call next_track twice instead.
        state.next_track();
        let g3 = state.play_generation;
        state.next_track();
        let g4 = state.play_generation;
        let gens: Vec<_> = (0..4)
            .map(|_| match rx.recv().expect("cmd") {
                PlaybackWorkerCommand::Play(req) => req.generation,
                _ => panic!("expected Play"),
            })
            .collect();
        assert!(gens.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(gens, vec![g1, g2, g3, g4]);
    }

    #[test]
    fn stale_play_result_is_discarded() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
        let old = state.play_generation;
        let _ = state.begin_play_request().expect("request");
        assert!(!state.should_apply_play_result(old));
        assert!(state.should_apply_play_result(state.play_generation));
    }

    #[test]
    fn toggle_play_restarts_current_track_when_sink_drained() {
        let mut state = test_wired();
        state.audio = Arc::new(Mutex::new(Box::new(EndedAudio::new("a", 180))));
        state.queue.push(Track::new("a", "Track A", "Artist").with_duration(180));

        state.toggle_play();

        let audio = state.audio.lock().unwrap();
        assert_eq!(audio.last_requested(), Some("a"));
        assert!(audio.is_playing());
        assert!(!audio.is_empty());
    }
}
