//! Real rodio playback backend implementing the [`AudioBackend`] port.

use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context;
use rodio::{Decoder, OutputStream, Sink};

use super::audio::{AudioBackend, AudioStub};
use super::stream::AudioQuality;
use super::ytdlp::YtDlpResolver;

/// Maps an integer volume level (0..=100, clamped) to an amplitude scaling factor (0.0..=1.0).
pub fn volume_to_amplitude(volume: u8) -> f32 {
    volume.min(100) as f32 / 100.0
}

/// Downloads raw audio payload bytes via blocking HTTP GET.
fn fetch_audio_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let response = reqwest::blocking::get(url)
        .with_context(|| format!("failed to fetch audio stream from '{url}'"))?
        .error_for_status()
        .with_context(|| format!("http error response downloading audio stream from '{url}'"))?;
    let bytes = response
        .bytes()
        .with_context(|| format!("failed to read audio stream payload from '{url}'"))?;
    Ok(bytes.to_vec())
}

/// Real rodio playback backend maintaining an active sink and position tracking.
pub struct RodioBackend {
    _stream: OutputStream,
    sink: Sink,
    resolver: YtDlpResolver,
    last_requested: Option<String>,
    volume: u8,
    anchor: Option<Instant>,
    paused_accum: Duration,
}

impl RodioBackend {
    /// Acquires the default output audio stream and initializes an empty playback sink.
    ///
    /// Fails loud with descriptive context when running in headless environments
    /// where no default audio output device is present.
    pub fn new(binary: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let stream = rodio::OutputStreamBuilder::open_default_stream()
            .context("failed to acquire default audio output stream")?;
        let sink = Sink::connect_new(stream.mixer());
        sink.set_volume(1.0);

        Ok(Self {
            _stream: stream,
            sink,
            resolver: YtDlpResolver::new(binary),
            last_requested: None,
            volume: 100,
            anchor: None,
            paused_accum: Duration::ZERO,
        })
    }

    /// Downloads and decodes audio from `url`, immediately starting playback.
    ///
    /// Fails loud on transport, HTTP status, or stream decoding errors without mutating state.
    pub fn play_url(&mut self, track_id: &str, url: &str) -> anyhow::Result<()> {
        let bytes = fetch_audio_bytes(url)?;
        let cursor = Cursor::new(bytes);
        let source = Decoder::new(cursor)
            .with_context(|| format!("failed to decode audio stream for track '{track_id}'"))?;

        self.sink.stop();
        self.sink.append(source);
        self.sink.play();
        self.anchor = Some(Instant::now());
        self.paused_accum = Duration::ZERO;
        self.last_requested = Some(track_id.to_owned());
        self.sink.set_volume(volume_to_amplitude(self.volume));

        Ok(())
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

    /// Returns the currently configured audio quality preference.
    pub fn quality(&self) -> AudioQuality {
        self.resolver.quality()
    }
}

impl AudioBackend for RodioBackend {
    /// Resolves stream URL via [`YtDlpResolver`] and begins playback.
    ///
    /// Soft-fails to a no-state-change on missing binary or decoding failures.
    /// Records `last_requested` strictly upon successful playback start.
    fn play(&mut self, track_id: &str) {
        let Some(url) = self.resolver.resolve(track_id) else {
            return;
        };
        let _ = self.play_url(track_id, &url);
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
        self.volume = volume.min(100);
        self.sink.set_volume(volume_to_amplitude(self.volume));
    }

    fn volume(&self) -> u8 {
        self.volume
    }

    fn position_secs(&self) -> u64 {
        self.current_position().as_secs()
    }

    fn is_playing(&self) -> bool {
        !self.sink.is_paused() && !self.sink.empty()
    }

    fn last_requested(&self) -> Option<&str> {
        self.last_requested.as_deref()
    }

    fn set_quality(&mut self, quality: AudioQuality) {
        self.resolver.set_quality(quality);
    }
}

/// Dual-mode audio backend: hardware playback with graceful fallback to an in-memory stub.
pub enum LiveAudio {
    /// Live rodio backend connected to audio hardware.
    Live(RodioBackend),
    /// Muted fallback storing state in memory when hardware or dependencies are unavailable.
    Muted(AudioStub),
}

impl LiveAudio {
    /// Creates a muted backend variant.
    pub fn muted() -> Self {
        Self::Muted(AudioStub::new())
    }

    /// Returns whether this backend is actively backed by audio hardware.
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Live(_))
    }
}

impl AudioBackend for LiveAudio {
    fn play(&mut self, track_id: &str) {
        match self {
            Self::Live(backend) => backend.play(track_id),
            Self::Muted(stub) => stub.play(track_id),
        }
    }

    fn pause(&mut self) {
        match self {
            Self::Live(backend) => backend.pause(),
            Self::Muted(stub) => stub.pause(),
        }
    }

    fn resume_play(&mut self) {
        match self {
            Self::Live(backend) => backend.resume_play(),
            Self::Muted(stub) => stub.resume_play(),
        }
    }

    fn seek(&mut self, position_secs: u64) {
        match self {
            Self::Live(backend) => backend.seek(position_secs),
            Self::Muted(stub) => stub.seek(position_secs),
        }
    }

    fn set_volume(&mut self, volume: u8) {
        match self {
            Self::Live(backend) => backend.set_volume(volume),
            Self::Muted(stub) => stub.set_volume(volume),
        }
    }

    fn volume(&self) -> u8 {
        match self {
            Self::Live(backend) => backend.volume(),
            Self::Muted(stub) => stub.volume(),
        }
    }

    fn position_secs(&self) -> u64 {
        match self {
            Self::Live(backend) => backend.position_secs(),
            Self::Muted(stub) => stub.position_secs(),
        }
    }

    fn is_playing(&self) -> bool {
        match self {
            Self::Live(backend) => backend.is_playing(),
            Self::Muted(stub) => stub.is_playing(),
        }
    }

    fn last_requested(&self) -> Option<&str> {
        match self {
            Self::Live(backend) => backend.last_requested(),
            Self::Muted(stub) => stub.last_requested(),
        }
    }

    fn set_quality(&mut self, quality: AudioQuality) {
        match self {
            Self::Live(backend) => backend.set_quality(quality),
            Self::Muted(stub) => stub.set_quality(quality),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_to_amplitude_mapping() {
        for (vol, expected) in [(0, 0.0_f32), (50, 0.5_f32), (100, 1.0_f32), (150, 1.0_f32)] {
            let actual = volume_to_amplitude(vol);
            assert!(
                (actual - expected).abs() < f32::EPSILON,
                "volume {vol} mapped to {actual}, expected {expected}"
            );
        }
    }

    #[test]
    fn constructor_tolerance_on_missing_binary() {
        match RodioBackend::new("/definitely/missing/binary/yt-dlp") {
            Ok(backend) => {
                assert_eq!(backend.volume(), 100);
                assert_eq!(backend.quality(), AudioQuality::Standard);
                assert_eq!(backend.last_requested(), None);
                assert!(!backend.is_playing());
                assert_eq!(backend.position_secs(), 0);
            }
            Err(err) => {
                eprintln!("Skipping audio hardware assertions in headless environment: {err:#}");
            }
        }
    }

    #[test]
    fn play_soft_fails_on_missing_binary() {
        let mut backend = match RodioBackend::new("/definitely/missing/binary/yt-dlp") {
            Ok(backend) => backend,
            Err(err) => {
                eprintln!("Skipping play assertion in headless environment: {err:#}");
                return;
            }
        };

        backend.play("dQw4w9WgXcQ");
        assert_eq!(backend.last_requested(), None);
        assert!(!backend.is_playing());
        assert_eq!(backend.position_secs(), 0);
    }

    #[test]
    fn play_url_fails_loud_on_invalid_url_without_mutating_state() {
        let mut backend = match RodioBackend::new("/definitely/missing/binary/yt-dlp") {
            Ok(backend) => backend,
            Err(err) => {
                eprintln!("Skipping play_url assertion in headless environment: {err:#}");
                return;
            }
        };

        let result = backend.play_url("track-fail", "not-a-valid-url-format");
        assert!(result.is_err(), "play_url must fail loud on invalid url");
        assert_eq!(backend.last_requested(), None);
        assert!(!backend.is_playing());
        assert_eq!(backend.position_secs(), 0);
    }

    #[test]
    fn state_transitions_and_seeking_coherence() {
        let mut backend = match RodioBackend::new("/definitely/missing/binary/yt-dlp") {
            Ok(backend) => backend,
            Err(err) => {
                eprintln!("Skipping state transitions assertion in headless environment: {err:#}");
                return;
            }
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
    fn live_audio_muted_delegation_table() {
        let mut audio = LiveAudio::muted();
        assert!(!audio.is_live());
        assert_eq!(audio.last_requested(), None);
        assert!(!audio.is_playing());
        assert_eq!(audio.position_secs(), 0);
        assert_eq!(audio.volume(), 100);

        audio.play("track-stub-1");
        assert_eq!(audio.last_requested(), Some("track-stub-1"));
        assert!(audio.is_playing());

        audio.pause();
        assert!(!audio.is_playing());

        audio.resume_play();
        assert!(audio.is_playing());

        audio.seek(42);
        assert_eq!(audio.position_secs(), 42);

        audio.set_volume(80);
        assert_eq!(audio.volume(), 80);

        audio.set_volume(150);
        assert_eq!(audio.volume(), 100);

        audio.set_quality(AudioQuality::High);
        audio.set_quality(AudioQuality::Compact);
    }
}
