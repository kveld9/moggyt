//! Real rodio playback backend implementing the [`AudioBackend`] port.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use rodio::{Decoder, OutputStream, Sink, Source};

use super::audio::{AudioBackend, AudioError, clamp_speed};
use super::audio_dsp::{SkipSilence, append_normalized, normalization_stage};
use super::audio_fade::{spawn_fade_in, spawn_fade_out};
use super::audio_stream_cache_worker::cache_download_worker;
use super::stream::{AudioQuality, ResolvedStream};
use super::stream_chain::StreamChain;
use super::stream_disk_cache::StreamDiskCache;
use super::stream_reqwest::open_audio_stream_with_prefix;

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

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn log_open_stream_error(track_id: &str, err: anyhow::Error) -> anyhow::Error {
    tracing::warn!(track_id = %track_id, error = ?err, "failed to open audio stream");
    err
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn log_decode_stream_error(
    track_id: &str,
    err: impl std::error::Error + Send + Sync + 'static,
) -> anyhow::Error {
    let anyhow_err = anyhow::Error::new(err)
        .context(format!("failed to decode audio stream for track '{track_id}'"));
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
    anchor: Option<Instant>,
    paused_accum: Duration,
    skip_silence: bool,
    loudness_normalization: bool,
    fade_generation: Arc<AtomicU64>,
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
            anchor: None,
            paused_accum: Duration::ZERO,
            skip_silence: false,
            loudness_normalization: false,
            fade_generation: Arc::new(AtomicU64::new(0)),
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

    fn append_dsp_to_sink<R>(&self, sink: &Sink, source: Decoder<R>, loudness_db: Option<f32>)
    where
        R: std::io::Read + std::io::Seek + Send + Sync + 'static,
    {
        let stage = normalization_stage(self.loudness_normalization, loudness_db);
        if self.skip_silence {
            append_normalized(sink, SkipSilence::new(source), stage);
        } else {
            append_normalized(sink, source, stage);
        }
    }

    fn apply_playback_source<R>(
        &mut self,
        track_id: &str,
        source: Decoder<R>,
        loudness_db: Option<f32>,
    ) where
        R: std::io::Read + std::io::Seek + Send + Sync + 'static,
    {
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

        self.anchor = Some(Instant::now());
        self.paused_accum = Duration::ZERO;
        self.last_requested = Some(track_id.to_owned());
        self.track_duration = total_duration;
    }

    fn decode_source(
        &self,
        url: &str,
        track_id: &str,
        prefix: Option<Vec<u8>>,
    ) -> anyhow::Result<
        Decoder<super::stream_reader::RangeReader<super::stream_reqwest::ReqwestRangeTransport>>,
    > {
        let stream = open_audio_stream_with_prefix(url, prefix)
            .map_err(|err| log_open_stream_error(track_id, err))?;
        Decoder::new(stream).map_err(|err| log_decode_stream_error(track_id, err))
    }

    fn apply_crossfade_source<R>(
        &mut self,
        track_id: &str,
        source: Decoder<R>,
        duration: Duration,
        loudness_db: Option<f32>,
    ) where
        R: std::io::Read + std::io::Seek + Send + Sync + 'static,
    {
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

        self.anchor = Some(Instant::now());
        self.paused_accum = Duration::ZERO;
        self.last_requested = Some(track_id.to_owned());
        self.track_duration = total_duration;
    }

    fn play_resolved(&mut self, track_id: &str, stream: &ResolvedStream) -> anyhow::Result<()> {
        let prefix = self.resolver.take_prefetched(&stream.url);
        let source = self.decode_source(&stream.url, track_id, prefix)?;
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
        let source = self.decode_source(&stream.url, track_id, prefix)?;
        self.apply_crossfade_source(track_id, source, duration, stream.loudness_db);
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

    fn freeze_anchor(&mut self) {
        if let Some(anchor) = self.anchor.take() {
            self.paused_accum = self.paused_accum.saturating_add(anchor.elapsed());
        }
    }

    fn reanchor(&mut self) {
        if self.anchor.is_none() {
            self.anchor = Some(Instant::now());
        }
    }

    fn reanchor_seek(&mut self, target: Duration) {
        if self.anchor.is_some() {
            self.anchor = Some(Instant::now());
        }
        self.paused_accum = target;
    }

    fn current_position(&self) -> Duration {
        let active = self.anchor.map_or(Duration::ZERO, |anchor| anchor.elapsed());
        self.paused_accum.saturating_add(active)
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
    ) -> anyhow::Result<Decoder<std::io::BufReader<std::fs::File>>> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to open cached stream at '{}'", path.display()))?;
        Decoder::new(std::io::BufReader::new(file))
            .map_err(|err| log_decode_stream_error(track_id, err))
    }

    fn cached_source(&self, track_id: &str) -> Option<Decoder<std::io::BufReader<std::fs::File>>> {
        let path = self.stream_cache.as_ref()?.get(track_id)?;
        Self::decode_cached_file(&path, track_id).ok()
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
        if cache.max_bytes() == 0 || cache.get(track_id).is_some() {
            return;
        }
        let track_id = track_id.to_string();
        let url = url.to_string();
        std::thread::Builder::new()
            .name("stream-cache-download".to_string())
            .spawn(move || cache_download_worker(cache, &track_id, &url))
            .ok();
    }

    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    fn retry_play_with_fresh_url(&mut self, track_id: &str) -> Result<(), AudioError> {
        self.resolver.invalidate(track_id);
        let Some(stream) = self.resolver.resolve_stream(track_id) else {
            tracing::warn!(track_id = %track_id, "failed to re-resolve audio stream URL on retry");
            return Err(AudioError::Unavailable);
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
            return Err(AudioError::Unavailable);
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
            return Err(AudioError::Unavailable);
        };
        if let Err(err) = self.play_resolved_crossfade(track_id, &stream, duration) {
            tracing::warn!(track_id = %track_id, error = ?err, "failed to crossfade audio stream; retrying immediate play");
            return self.retry_play_with_fresh_url(track_id);
        }
        Ok(())
    }

    fn stream_prefetcher(&self) -> Option<StreamChain> {
        Some((*self.resolver).clone())
    }

    fn has_cached_stream(&self, track_id: &str) -> bool {
        self.stream_cache.as_ref().is_some_and(|cache| cache.get(track_id).is_some())
    }

    fn pause(&mut self) {
        self.sink.pause();
        self.freeze_anchor();
    }

    fn resume_play(&mut self) {
        if self.last_requested.is_some() {
            self.sink.play();
            self.reanchor();
        }
    }

    fn seek(&mut self, position_secs: u64) {
        let target = Duration::from_secs(position_secs);
        let _ = self.sink.try_seek(target);
        self.reanchor_seek(target);
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

    fn position_secs(&self) -> u64 {
        self.current_position().as_secs()
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
mod tests {
    use super::*;
    use crate::infra::YtDlpResolver;

    fn test_backend() -> Option<RodioBackend> {
        let mut backend = match RodioBackend::new("/definitely/missing/binary/yt-dlp") {
            Ok(backend) => backend,
            Err(err) => {
                eprintln!("Skipping audio hardware assertions in headless environment: {err:#}");
                return None;
            }
        };
        backend.set_resolver_chain(StreamChain::from_ytdlp(YtDlpResolver::new(
            "/definitely/missing/binary/yt-dlp",
        )));
        Some(backend)
    }

    #[test]
    fn volume_to_amplitude_mapping() {
        for (vol, expected) in
            [(0, 0.0_f32), (50, 0.125_f32), (100, 1.0_f32), (150, 1.0_f32), (25, 0.015625_f32)]
        {
            let actual = volume_to_amplitude(vol);
            assert!(
                (actual - expected).abs() < 1e-6,
                "volume {vol} mapped to {actual}, expected {expected}"
            );
        }
    }

    #[test]
    fn constructor_tolerance_on_missing_binary() {
        match RodioBackend::new("/definitely/missing/binary/yt-dlp") {
            Ok(mut backend) => {
                assert_eq!(backend.volume(), 100);
                assert_eq!(backend.speed(), 1.0);
                assert_eq!(backend.quality(), AudioQuality::Standard);
                assert_eq!(backend.last_requested(), None);
                assert!(!backend.is_playing());
                assert_eq!(backend.position_secs(), 0);
                assert!(!backend.skip_silence());
                assert!(!backend.loudness_normalization());
                backend.set_skip_silence(true);
                assert!(backend.skip_silence());
                backend.set_loudness_normalization(true);
                assert!(backend.loudness_normalization());
            }
            Err(err) => {
                eprintln!("Skipping audio hardware assertions in headless environment: {err:#}");
            }
        }
    }

    #[test]
    fn play_soft_fails_on_missing_binary() {
        let Some(mut backend) = test_backend() else {
            return;
        };

        let _ = backend.play("dQw4w9WgXcQ");
        assert_eq!(backend.last_requested(), None);
        assert!(!backend.is_playing());
        assert_eq!(backend.position_secs(), 0);
    }

    #[test]
    fn play_url_fails_loud_on_invalid_url_without_mutating_state() {
        let Some(mut backend) = test_backend() else {
            return;
        };

        let result = backend.play_url("track-fail", "not-a-valid-url-format");
        assert!(result.is_err(), "play_url must fail loud on invalid url");
        assert_eq!(backend.last_requested(), None);
        assert!(!backend.is_playing());
        assert_eq!(backend.position_secs(), 0);
    }

    #[test]
    fn state_transitions_and_seeking_coherence() {
        let Some(mut backend) = test_backend() else {
            return;
        };

        backend.set_volume(42);
        assert_eq!(backend.volume(), 42);
        backend.set_volume(150);
        assert_eq!(backend.volume(), 100);

        backend.seek(15);
        assert_eq!(backend.position_secs(), 15);

        backend.pause();
        assert_eq!(backend.position_secs(), 15);
        assert!(!backend.is_playing());

        backend.resume_play();
        assert!(!backend.is_playing());
    }

    #[test]
    fn play_invalidates_cache_and_retries_on_stream_failure() {
        let Some(mut backend) = test_backend() else {
            return;
        };

        let track_id = "test-fail-retry";
        backend.resolver.cache().insert(
            track_id,
            AudioQuality::Standard,
            "not-a-valid-url".to_string(),
            Instant::now(),
        );
        assert!(
            backend
                .resolver
                .cache()
                .get(track_id, AudioQuality::Standard, Instant::now())
                .is_some()
        );

        // play should fail on "not-a-valid-url", invalidate the cache entry, and fail second resolve
        let _ = backend.play(track_id);
        assert_eq!(
            backend.resolver.cache().get(track_id, AudioQuality::Standard, Instant::now()),
            None
        );
    }

    #[test]
    fn play_url_crossfade_fails_loud_on_invalid_url_without_mutating_state() {
        let Some(mut backend) = test_backend() else {
            return;
        };

        let result =
            backend.play_url_crossfade("track-cf-fail", "not-a-valid-url", Duration::from_secs(3));
        assert!(result.is_err(), "play_url_crossfade must fail loud on invalid url");
        assert_eq!(backend.last_requested(), None);
        assert!(!backend.is_playing());
    }

    #[test]
    fn play_crossfade_invalidates_cache_and_retries_on_stream_failure() {
        let Some(mut backend) = test_backend() else {
            return;
        };

        let track_id = "test-cf-fail-retry";
        backend.resolver.cache().insert(
            track_id,
            AudioQuality::Standard,
            "not-a-valid-url".to_string(),
            Instant::now(),
        );

        let _ = backend.play_crossfade(track_id, Duration::from_secs(2));
        assert_eq!(
            backend.resolver.cache().get(track_id, AudioQuality::Standard, Instant::now()),
            None
        );
    }

    #[test]
    fn play_crossfade_zero_duration_fallback_handles_missing_binary() {
        let Some(mut backend) = test_backend() else {
            return;
        };

        let _ = backend.play_crossfade("zero-dur-track", Duration::ZERO);
        assert_eq!(backend.last_requested(), None);
    }

    const MINIMAL_WAV_HEADER: [u8; 44] = [
        b'R', b'I', b'F', b'F', 36, 0, 0, 0, b'W', b'A', b'V', b'E', b'f', b'm', b't', b' ', 16, 0,
        0, 0, 1, 0, 1, 0, 68, 172, 0, 0, 136, 88, 1, 0, 2, 0, 16, 0, b'd', b'a', b't', b'a', 0, 0,
        0, 0,
    ];

    #[test]
    fn backend_stream_cache_accessors() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-test-backend-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let cache = Arc::new(StreamDiskCache::new(&temp_dir, 1024 * 1024).expect("cache init"));
        cache.put("cached-track-1", &MINIMAL_WAV_HEADER).expect("put wav in cache");

        let Some(mut backend) = test_backend() else {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return;
        };

        assert!(backend.stream_cache().is_none());
        assert!(!backend.has_cached_stream("cached-track-1"));
        backend.set_stream_cache(Some(Arc::clone(&cache)));
        assert!(backend.stream_cache().is_some());
        assert!(backend.has_cached_stream("cached-track-1"));
        assert!(!backend.has_cached_stream("other-track"));
        backend.set_stream_cache(None);
        assert!(backend.stream_cache().is_none());
        assert!(!backend.has_cached_stream("cached-track-1"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn play_cached_stream_bypasses_resolver_when_in_cache() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-test-cached-stream-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let cache = Arc::new(StreamDiskCache::new(&temp_dir, 1024 * 1024).expect("cache init"));
        cache.put("cached-track-1", &MINIMAL_WAV_HEADER).expect("put wav in cache");

        let mut backend =
            match RodioBackend::with_cache("/definitely/missing/binary/yt-dlp", Some(cache)) {
                Ok(backend) => backend,
                Err(err) => {
                    eprintln!(
                        "Skipping audio hardware assertions in headless environment: {err:#}"
                    );
                    let _ = std::fs::remove_dir_all(&temp_dir);
                    return;
                }
            };

        assert_eq!(backend.play("cached-track-1"), Ok(()));
        assert_eq!(backend.last_requested(), Some("cached-track-1"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn sink_replacement_coherence_on_consecutive_playback() {
        let Some(mut backend) = test_backend() else {
            return;
        };

        let initial_sink = Arc::clone(&backend.sink);

        let source1 =
            Decoder::new(std::io::Cursor::new(MINIMAL_WAV_HEADER)).expect("valid wav decoder 1");
        backend.apply_playback_source("track-1", source1, None);

        assert!(!Arc::ptr_eq(&initial_sink, &backend.sink));
        let second_sink = Arc::clone(&backend.sink);
        assert_eq!(backend.last_requested(), Some("track-1"));
        assert!(backend.is_playing());

        let source2 =
            Decoder::new(std::io::Cursor::new(MINIMAL_WAV_HEADER)).expect("valid wav decoder 2");
        backend.apply_playback_source("track-2", source2, None);

        assert!(!Arc::ptr_eq(&second_sink, &backend.sink));
        assert_eq!(backend.last_requested(), Some("track-2"));
        assert!(backend.is_playing());
    }

    #[test]
    fn rodio_backend_set_session_cookies_forwards_to_resolver() {
        let Some(mut backend) = test_backend() else {
            return;
        };
        backend.set_session_cookies(Some("SAPISID=rodio_cookie"));
        let prefetcher = backend.stream_prefetcher().expect("prefetcher available");
        assert_eq!(prefetcher.session_cookies(), Some("SAPISID=rodio_cookie"));
        backend.set_session_cookies(None);
        let prefetcher = backend.stream_prefetcher().expect("prefetcher available");
        assert_eq!(prefetcher.session_cookies(), None);
    }
}
