//! Real rodio playback backend implementing the [`AudioBackend`] port.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Context;
use rodio::{Decoder, OutputStream, Sink, Source};

use super::audio::{AudioBackend, AudioError, clamp_speed};
use super::audio_dsp::{SkipSilence, append_normalized, normalization_stage};
use super::audio_fade::{spawn_fade_in, spawn_fade_out};
use super::audio_resample::ResamplingSource;
use super::audio_stream_cache_worker::cache_download_worker;
use super::opus_source::{OpusWebmSource, is_opus_webm, sniff_webm_header};
use super::stream::{AudioQuality, ResolvedStream};
use super::stream_chain::StreamChain;
use super::stream_disk_cache::StreamDiskCache;
use super::stream_prefetch_cache::PrefetchedPrefix;
use super::stream_reqwest::open_audio_stream_with_prefix;
use crate::domain::{GaplessSlot, StartedGapless};

/// Maps an integer volume level (0..=100, clamped) to an amplitude scaling factor (0.0..=1.0).
///
/// The curve is cubic (`gain = (volume / 100)^3`), matching the perceptual mapping used by
/// PulseAudio. A linear curve compresses most of the perceived loudness change into the
/// lowest fifth of the slider; at 50 percent this curve yields an amplitude of 0.125
/// (-18 dB), which preserves usable resolution at quiet listening levels. Levels above 100
/// clamp to full amplitude.
pub fn volume_to_amplitude(volume: u8) -> f32 {
    let normalized = volume.min(100) as f32 / 100.0;
    normalized * normalized * normalized
}

/// Clamps a playback position to the known track duration.
fn clamp_playback_position(pos: Duration, duration: Option<Duration>) -> Duration {
    duration.map_or(pos, |duration| pos.min(duration))
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn log_open_stream_error(track_id: &str, err: anyhow::Error) -> anyhow::Error {
    tracing::warn!(track_id = %track_id, error = ?err, "failed to open audio stream");
    err
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn log_decode_stream_error(track_id: &str, err: anyhow::Error) -> anyhow::Error {
    let anyhow_err = err.context(format!("failed to decode audio stream for track '{track_id}'"));
    tracing::warn!(track_id = %track_id, error = ?anyhow_err, "failed to decode audio stream");
    anyhow_err
}

/// Real rodio playback backend maintaining an active sink and position tracking.
pub struct RodioBackend {
    _stream: OutputStream,
    sink: Arc<Sink>,
    resolver: Box<StreamChain>,
    stream_cache: Option<Arc<StreamDiskCache>>,
    last_requested: Option<String>,
    track_duration: Option<Duration>,
    volume: u8,
    speed: f32,
    skip_silence: bool,
    loudness_normalization: bool,
    fade_generation: Arc<AtomicU64>,
    /// Next-track slot appended behind the active track.
    gapless: GaplessSlot,
}

impl RodioBackend {
    /// Acquires the default output audio stream and initializes an empty playback sink.
    ///
    /// Fails loud with descriptive context when running in headless environments
    /// where no default audio output device is present.
    pub fn new(binary: impl Into<PathBuf>) -> anyhow::Result<Self> {
        Self::with_cache(binary, None)
    }

    /// Acquires output stream with an optional disk-backed stream cache.
    pub fn with_cache(
        binary: impl Into<PathBuf>,
        stream_cache: Option<Arc<StreamDiskCache>>,
    ) -> anyhow::Result<Self> {
        let stream = rodio::OutputStreamBuilder::open_default_stream()
            .context("failed to acquire default audio output stream")?;
        let sink = Sink::connect_new(stream.mixer());
        sink.set_volume(1.0);
        sink.set_speed(1.0);

        Ok(Self {
            _stream: stream,
            sink: Arc::new(sink),
            resolver: Box::new(StreamChain::new(binary)),
            stream_cache,
            last_requested: None,
            track_duration: None,
            volume: 100,
            speed: 1.0,
            skip_silence: false,
            loudness_normalization: false,
            fade_generation: Arc::new(AtomicU64::new(0)),
            gapless: GaplessSlot::default(),
        })
    }

    /// Replaces the resolver chain, for tests and custom wiring.
    pub fn set_resolver_chain(&mut self, resolver: StreamChain) {
        *self.resolver = resolver;
    }

    /// Updates session cookies for stream resolution authentication.
    pub fn set_session_cookies(&mut self, cookies: Option<&str>) {
        self.resolver.set_session_cookies(cookies);
    }

    /// Configures or clears the backing stream disk cache.
    pub fn set_stream_cache(&mut self, cache: Option<Arc<StreamDiskCache>>) {
        self.stream_cache = cache;
    }

    /// Returns the backing stream disk cache if configured.
    pub fn stream_cache(&self) -> Option<&Arc<StreamDiskCache>> {
        self.stream_cache.as_ref()
    }

    fn output_sample_rate(&self) -> u32 {
        self._stream.config().sample_rate()
    }

    fn append_dsp_to_sink(
        &self,
        sink: &Sink,
        source: Box<dyn Source<Item = f32> + Send>,
        loudness_db: Option<f32>,
    ) {
        let stage = normalization_stage(self.loudness_normalization, loudness_db);
        if self.skip_silence {
            append_normalized(sink, SkipSilence::new(source), stage);
        } else {
            append_normalized(sink, source, stage);
        }
    }

    fn apply_playback_source(
        &mut self,
        track_id: &str,
        source: Box<dyn Source<Item = f32> + Send>,
        loudness_db: Option<f32>,
    ) {
        self.gapless.reset();
        let total_duration = source.total_duration();
        self.fade_generation.fetch_add(1, Ordering::SeqCst);
        let new_sink = Sink::connect_new(self._stream.mixer());
        let new_sink_arc = Arc::new(new_sink);
        let old_sink = std::mem::replace(&mut self.sink, Arc::clone(&new_sink_arc));
        old_sink.stop();

        new_sink_arc.set_speed(self.speed);
        new_sink_arc.set_volume(volume_to_amplitude(self.volume));
        self.append_dsp_to_sink(&new_sink_arc, source, loudness_db);
        new_sink_arc.play();

        self.last_requested = Some(track_id.to_owned());
        self.track_duration = total_duration;
    }

    #[allow(clippy::too_many_arguments)]
    fn decode_source(
        &self,
        url: &str,
        track_id: &str,
        prefix: Option<PrefetchedPrefix>,
        mime_type: Option<&str>,
        target_rate: u32,
    ) -> anyhow::Result<Box<dyn Source<Item = f32> + Send>> {
        let stream = open_audio_stream_with_prefix(url, prefix)
            .map_err(|err| log_open_stream_error(track_id, err))?;
        if is_opus_webm(mime_type, url) {
            let source = OpusWebmSource::new(stream)
                .map_err(|err| log_decode_stream_error(track_id, err))?;
            let resampled = ResamplingSource::new(source, target_rate)
                .map_err(|err| log_decode_stream_error(track_id, err))?;
            Ok(Box::new(resampled))
        } else {
            let decoder = Decoder::new(stream)
                .map_err(|err| log_decode_stream_error(track_id, err.into()))?;
            let resampled = ResamplingSource::new(decoder, target_rate)
                .map_err(|err| log_decode_stream_error(track_id, err))?;
            Ok(Box::new(resampled))
        }
    }

    fn apply_crossfade_source(
        &mut self,
        track_id: &str,
        source: Box<dyn Source<Item = f32> + Send>,
        duration: Duration,
        loudness_db: Option<f32>,
    ) {
        self.gapless.reset();
        let total_duration = source.total_duration();
        let new_sink = Sink::connect_new(self._stream.mixer());
        let new_sink_arc = Arc::new(new_sink);
        let old_sink = std::mem::replace(&mut self.sink, Arc::clone(&new_sink_arc));

        spawn_fade_out(old_sink, duration);

        new_sink_arc.set_speed(self.speed);
        new_sink_arc.set_volume(0.0);
        self.append_dsp_to_sink(&new_sink_arc, source, loudness_db);
        new_sink_arc.play();

        let target_vol = volume_to_amplitude(self.volume);
        let generation_id = self.fade_generation.fetch_add(1, Ordering::SeqCst) + 1;
        spawn_fade_in(
            new_sink_arc,
            target_vol,
            duration,
            Arc::clone(&self.fade_generation),
            generation_id,
        );

        self.last_requested = Some(track_id.to_owned());
        self.track_duration = total_duration;
    }

    fn play_resolved(&mut self, track_id: &str, stream: &ResolvedStream) -> anyhow::Result<()> {
        let prefix = self.resolver.take_prefetched(&stream.url);
        let source = self.decode_source(
            &stream.url,
            track_id,
            prefix,
            stream.mime_type.as_deref(),
            self.output_sample_rate(),
        )?;
        self.apply_playback_source(track_id, source, stream.loudness_db);
        self.maybe_spawn_cache_download(track_id, &stream.url);
        Ok(())
    }

    fn play_resolved_crossfade(
        &mut self,
        track_id: &str,
        stream: &ResolvedStream,
        duration: Duration,
    ) -> anyhow::Result<()> {
        let prefix = self.resolver.take_prefetched(&stream.url);
        let source = self.decode_source(
            &stream.url,
            track_id,
            prefix,
            stream.mime_type.as_deref(),
            self.output_sample_rate(),
        )?;
        self.apply_crossfade_source(track_id, source, duration, stream.loudness_db);
        self.maybe_spawn_cache_download(track_id, &stream.url);
        Ok(())
    }

    fn append_gapless_source(
        &mut self,
        track_id: &str,
        source: Box<dyn Source<Item = f32> + Send>,
        loudness_db: Option<f32>,
    ) {
        let duration = source.total_duration();
        self.append_dsp_to_sink(&self.sink, source, loudness_db);
        self.gapless.arm(track_id.to_owned(), duration.map(|d| d.as_secs()));
        self.last_requested = Some(track_id.to_owned());
    }

    fn try_append_cached_gapless(&mut self, track_id: &str) -> bool {
        let Some(source) = self.cached_source(track_id) else {
            return false;
        };
        self.append_gapless_source(track_id, source, None);
        true
    }

    fn play_gapless_resolved(
        &mut self,
        track_id: &str,
        stream: &ResolvedStream,
    ) -> anyhow::Result<()> {
        let prefix = self.resolver.take_prefetched(&stream.url);
        let source = self.decode_source(
            &stream.url,
            track_id,
            prefix,
            stream.mime_type.as_deref(),
            self.output_sample_rate(),
        )?;
        self.append_gapless_source(track_id, source, stream.loudness_db);
        self.maybe_spawn_cache_download(track_id, &stream.url);
        Ok(())
    }

    /// Downloads and decodes audio from `url`, immediately starting playback.
    ///
    /// Fails loud on transport, HTTP status, or stream decoding errors without mutating state.
    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    pub fn play_url(&mut self, track_id: &str, url: &str) -> anyhow::Result<()> {
        self.play_resolved(track_id, &ResolvedStream::from_url(url))
    }

    /// Downloads and decodes audio from `url`, crossfading with the currently active audio sink.
    ///
    /// Fails loud on transport, HTTP status, or stream decoding errors without mutating state.
    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    pub fn play_url_crossfade(
        &mut self,
        track_id: &str,
        url: &str,
        duration: Duration,
    ) -> anyhow::Result<()> {
        self.play_resolved_crossfade(track_id, &ResolvedStream::from_url(url), duration)
    }

    /// Returns whether skip-silence processing is enabled.
    pub fn skip_silence(&self) -> bool {
        self.skip_silence
    }

    /// Returns whether loudness normalization processing is enabled.
    pub fn loudness_normalization(&self) -> bool {
        self.loudness_normalization
    }

    fn decode_cached_file(
        path: &std::path::Path,
        track_id: &str,
        target_rate: u32,
    ) -> anyhow::Result<Box<dyn Source<Item = f32> + Send>> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to open cached stream at '{}'", path.display()))?;
        let mut reader = std::io::BufReader::new(file);
        let is_webm = sniff_webm_header(&mut reader)
            .with_context(|| format!("failed to inspect cached stream at '{}'", path.display()))?;
        if is_webm {
            let source = OpusWebmSource::new(reader)
                .map_err(|err| log_decode_stream_error(track_id, err))?;
            let resampled = ResamplingSource::new(source, target_rate)
                .map_err(|err| log_decode_stream_error(track_id, err))?;
            Ok(Box::new(resampled))
        } else {
            let decoder = Decoder::new(reader)
                .map_err(|err| log_decode_stream_error(track_id, err.into()))?;
            let resampled = ResamplingSource::new(decoder, target_rate)
                .map_err(|err| log_decode_stream_error(track_id, err))?;
            Ok(Box::new(resampled))
        }
    }

    fn cached_source(&self, track_id: &str) -> Option<Box<dyn Source<Item = f32> + Send>> {
        let path = self.stream_cache.as_ref()?.get(track_id, self.quality())?;
        Self::decode_cached_file(&path, track_id, self.output_sample_rate()).ok()
    }

    fn try_play_cached(&mut self, track_id: &str) -> bool {
        let Some(source) = self.cached_source(track_id) else {
            return false;
        };
        self.apply_playback_source(track_id, source, None);
        true
    }

    fn try_play_crossfade_cached(&mut self, track_id: &str, duration: Duration) -> bool {
        let Some(source) = self.cached_source(track_id) else {
            return false;
        };
        self.apply_crossfade_source(track_id, source, duration, None);
        true
    }

    fn maybe_spawn_cache_download(&self, track_id: &str, url: &str) {
        let Some(cache) = self.stream_cache.clone() else {
            return;
        };
        let quality = self.quality();
        if cache.max_bytes() == 0 || cache.get(track_id, quality).is_some() {
            return;
        }
        let track_id = track_id.to_string();
        let url = url.to_string();
        std::thread::Builder::new()
            .name("stream-cache-download".to_string())
            .spawn(move || cache_download_worker(cache, &track_id, quality, &url))
            .ok();
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn retry_play_with_fresh_url(&mut self, track_id: &str) -> Result<(), AudioError> {
        self.resolver.invalidate(track_id);
        let Some(stream) = self.resolver.resolve_stream(track_id) else {
            tracing::warn!(track_id = %track_id, "failed to re-resolve audio stream URL on retry");
            return Err(AudioError::Resolve(track_id.to_string()));
        };
        self.play_resolved(track_id, &stream).map_err(|err| {
            tracing::warn!(track_id = %track_id, error = ?err, "failed to play audio stream on retry");
            AudioError::Playback(err.to_string())
        })
    }
}

impl AudioBackend for RodioBackend {
    /// Resolves stream URL via [`StreamChain`] and begins playback.
    ///
    /// Soft-fails to a no-state-change on missing binary or decoding failures.
    /// Records `last_requested` strictly upon successful playback start.
    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn play(&mut self, track_id: &str) -> Result<(), AudioError> {
        if self.try_play_cached(track_id) {
            return Ok(());
        }
        let Some(stream) = self.resolver.resolve_stream(track_id) else {
            tracing::warn!(track_id = %track_id, "failed to resolve audio stream URL");
            return Err(AudioError::Resolve(track_id.to_string()));
        };
        if let Err(err) = self.play_resolved(track_id, &stream) {
            tracing::warn!(track_id = %track_id, error = ?err, "failed to play audio stream");
            return self.retry_play_with_fresh_url(track_id);
        }
        Ok(())
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn play_crossfade(&mut self, track_id: &str, duration: Duration) -> Result<(), AudioError> {
        if duration.is_zero() {
            return self.play(track_id);
        }
        if self.try_play_crossfade_cached(track_id, duration) {
            return Ok(());
        }
        let Some(stream) = self.resolver.resolve_stream(track_id) else {
            tracing::warn!(track_id = %track_id, "failed to resolve audio stream URL for crossfade");
            return Err(AudioError::Resolve(track_id.to_string()));
        };
        if let Err(err) = self.play_resolved_crossfade(track_id, &stream, duration) {
            tracing::warn!(track_id = %track_id, error = ?err, "failed to crossfade audio stream; retrying immediate play");
            return self.retry_play_with_fresh_url(track_id);
        }
        Ok(())
    }

    /// Decodes the next track and appends it to the live sink without touching the active source.
    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn play_gapless(&mut self, track_id: &str) -> Result<(), AudioError> {
        if !self.is_playing() {
            return Err(AudioError::Playback(
                "gapless preload requires an active sink".to_string(),
            ));
        }
        if self.try_append_cached_gapless(track_id) {
            return Ok(());
        }
        let Some(stream) = self.resolver.resolve_stream(track_id) else {
            tracing::warn!(track_id = %track_id, "failed to resolve audio stream URL for gapless preload");
            return Err(AudioError::Resolve(track_id.to_string()));
        };
        self.play_gapless_resolved(track_id, &stream).map_err(|err| {
            tracing::warn!(track_id = %track_id, error = ?err, "failed to append gapless audio stream");
            AudioError::Playback(err.to_string())
        })
    }

    fn take_started_gapless(&mut self) -> Option<StartedGapless> {
        if self.sink.empty() {
            self.gapless.reset();
            return None;
        }
        let started = self.gapless.observe(self.sink.len())?;
        self.track_duration = started.duration_secs.map(Duration::from_secs);
        Some(started)
    }

    fn stream_prefetcher(&self) -> Option<StreamChain> {
        Some((*self.resolver).clone())
    }

    fn has_cached_stream(&self, track_id: &str) -> bool {
        self.stream_cache
            .as_ref()
            .is_some_and(|cache| cache.get(track_id, self.quality()).is_some())
    }

    fn pause(&mut self) {
        self.sink.pause();
    }

    fn resume_play(&mut self) {
        if self.last_requested.is_some() {
            self.sink.play();
        }
    }

    fn seek(&mut self, position_secs: u64) {
        let target = Duration::from_secs(position_secs);
        let _ = self.sink.try_seek(target);
    }

    fn set_volume(&mut self, volume: u8) {
        self.fade_generation.fetch_add(1, Ordering::SeqCst);
        self.volume = volume.min(100);
        self.sink.set_volume(volume_to_amplitude(self.volume));
    }

    fn volume(&self) -> u8 {
        self.volume
    }

    fn set_speed(&mut self, speed: f32) {
        let clamped = clamp_speed(speed);
        self.speed = clamped;
        self.sink.set_speed(clamped);
    }

    fn speed(&self) -> f32 {
        self.speed
    }

    /// Returns the audio sink's real playback position, clamped to the known duration.
    fn position_secs(&self) -> u64 {
        clamp_playback_position(self.sink.get_pos(), self.track_duration).as_secs()
    }

    fn duration_secs(&self) -> u64 {
        self.track_duration.map_or(0, |d| d.as_secs())
    }

    fn is_playing(&self) -> bool {
        !self.sink.is_paused() && !self.sink.empty()
    }

    fn is_paused(&self) -> bool {
        self.sink.is_paused() && !self.sink.empty()
    }

    fn is_empty(&self) -> bool {
        self.sink.empty()
    }

    fn last_requested(&self) -> Option<&str> {
        self.last_requested.as_deref()
    }

    fn set_quality(&mut self, quality: AudioQuality) {
        self.resolver.set_quality(quality);
    }

    fn quality(&self) -> AudioQuality {
        self.resolver.quality()
    }

    fn set_skip_silence(&mut self, on: bool) {
        self.skip_silence = on;
    }

    fn set_loudness_normalization(&mut self, on: bool) {
        self.loudness_normalization = on;
    }

    fn set_session_cookies(&mut self, cookies: Option<&str>) {
        self.set_session_cookies(cookies);
    }
}

#[cfg(test)]
#[path = "audio_rodio_tests.rs"]
mod tests;
