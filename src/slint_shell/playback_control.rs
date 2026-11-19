//! Playback control, track completion, queue progression, volume, seeking,
//! and presence updates on SlintAppState.
//!
//! Hexagonal invariant: non-blocking audio port calls and presence sync without UI event loop stalls.

use std::time::{Duration, Instant};

use slint::{ComponentHandle, ModelRc, VecModel};

use super::sync::set_lyrics_highlight;
use crate::domain::{PlaybackPhase, Track};
use crate::infra::{AudioBackend, AudioQuality};
use crate::slint_shell::format::current_unix_secs;
use crate::slint_shell::platform::rgba_to_image;
use crate::slint_shell::playback_worker::PlayTrackResult;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::sync::sync_window;
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
        self.queue.set_cursor(index);
    }

    /// Plays the library songs list from `index`, replacing the queue.
    pub fn play_library_songs(&mut self, index: usize) {
        if index >= self.library_songs.len() {
            return;
        }
        self.replace_context(self.library_songs.clone(), index);
        self.persist_queue();
    }

    /// Advances or retreats queue cursor to target index and requests playback.
    pub fn play_track_at_index(&mut self, index: usize) {
        self.play_track_at_index_in_memory(index);
        self.persist_queue();
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
        self.persist_queue();
        self.request_current_play();
    }

    /// Retreats cursor to previous track, scrobbling the completed track if eligible.
    pub fn prev_track(&mut self) {
        self.queue.retreat();
        self.persist_queue();
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

    /// Clears the queue, retaining the active track when loaded or being resolved, or clearing fully when idle.
    pub fn clear_queue(&mut self) {
        if self.is_current_track_loaded() {
            self.clear_queue_keeping_current();
        } else {
            self.clear_queue_fully();
        }
    }

    /// Returns whether the cursor track is loaded in the audio backend or being resolved.
    fn is_current_track_loaded(&self) -> bool {
        let Some(current) = self.queue.current() else {
            return false;
        };
        if self.playback_phase == PlaybackPhase::Resolving {
            return true;
        }
        let Ok(audio) = self.audio.lock() else {
            return false;
        };
        audio.last_requested() == Some(current.id.as_str())
    }

    fn clear_queue_keeping_current(&mut self) {
        self.queue.retain_current();
        self.gapless.reset();
        self.persist_queue();
    }

    fn clear_queue_fully(&mut self) {
        if let Ok(mut audio) = self.audio.lock() {
            audio.pause();
        }
        self.pause_play_ledger();
        self.queue.clear();
        self.set_playback_phase(PlaybackPhase::Empty);
        self.sync_presence_playing(false);
        self.persist_queue();
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
}

/// Applies completed playback worker results back to the Slint window on the event loop.
pub fn apply_play_track_completion(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    res: PlayTrackResult,
) {
    if !res.success {
        super::playback_failure::report_playback_failure(window, state, &res);
        window.global::<PlayerUi>().set_is_playing(false);
        return;
    }
    super::playback_failure::clear_playback_failure(window, state);
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

    let presence_track = state.resolve_presence_track(&res.track_id, &res.artist, &res.title);
    state.sync_presence_track(presence_track, true);
    state.started_at =
        state.queue.tracks().iter().find(|t| t.id == res.track_id).cloned().map(StartedPlay::now);
    state.last_skip_target = None;
    state.cached_skip_ranges = None;
    sync_window(window, state);
    window.global::<PlayerUi>().set_is_playing(true);
}

#[cfg(test)]
#[path = "playback_control_tests.rs"]
mod tests;
