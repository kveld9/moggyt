//! Background stream URL prefetching for upcoming queue tracks off the UI thread.

use std::sync::{Arc, Mutex};

use tracing::{debug, warn};

use crate::infra::{AudioBackend, YtDlpResolver};

fn acquire_prefetcher<A: AudioBackend>(audio: &Arc<Mutex<A>>) -> Option<YtDlpResolver> {
    let guard = audio.lock().ok()?;
    guard.stream_prefetcher()
}

/// Spawns a background thread to prefetch the stream URL for the next track into cache.
///
/// Returns `None` without spawning if `video_id` is `None`, blank whitespace,
/// if the audio backend mutex is poisoned, or if the backend returns no prefetcher handle.
///
/// The spawned thread runs as `"moggyt-stream-prefetch"`. Never logs resolved URLs.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub fn spawn_stream_prefetch<A: AudioBackend>(
    video_id: Option<String>,
    audio: &Arc<Mutex<A>>,
) -> Option<std::thread::JoinHandle<()>> {
    let id = video_id?;
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return None;
    }
    let prefetcher = acquire_prefetcher(audio)?;
    let target_id = trimmed.to_string();

    match std::thread::Builder::new().name("moggyt-stream-prefetch".into()).spawn(move || {
        if !prefetcher.prefetch(&target_id) {
            debug!(video_id = %target_id, "failed to prefetch stream URL");
        }
    }) {
        Ok(handle) => Some(handle),
        Err(err) => {
            warn!(error = %err, "failed to spawn stream prefetch thread");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::{AudioError, AudioQuality, AudioStub};

    #[test]
    fn spawn_stream_prefetch_none_id_returns_none() {
        let audio = Arc::new(Mutex::new(AudioStub::new()));
        assert!(spawn_stream_prefetch(None, &audio).is_none());
    }

    #[test]
    fn spawn_stream_prefetch_blank_id_returns_none() {
        let audio = Arc::new(Mutex::new(AudioStub::new()));
        for blank in ["", "   ", "\t", "\n"] {
            assert!(spawn_stream_prefetch(Some(blank.into()), &audio).is_none());
        }
    }

    #[test]
    fn spawn_stream_prefetch_audio_stub_returns_none() {
        let audio = Arc::new(Mutex::new(AudioStub::new()));
        assert!(spawn_stream_prefetch(Some("video-123".into()), &audio).is_none());
    }

    #[test]
    fn spawn_stream_prefetch_poisoned_lock_returns_none() {
        let audio = Arc::new(Mutex::new(AudioStub::new()));
        let a_clone = Arc::clone(&audio);
        let _ = std::thread::spawn(move || {
            let _guard = a_clone.lock().unwrap();
            panic!("poisoning audio lock");
        })
        .join();

        assert!(spawn_stream_prefetch(Some("video-123".into()), &audio).is_none());
    }

    struct MockPrefetchBackend {
        resolver: YtDlpResolver,
    }

    impl AudioBackend for MockPrefetchBackend {
        fn play(&mut self, _track_id: &str) -> Result<(), AudioError> {
            Ok(())
        }
        fn pause(&mut self) {}
        fn resume_play(&mut self) {}
        fn seek(&mut self, _pos: u64) {}
        fn set_volume(&mut self, _vol: u8) {}
        fn volume(&self) -> u8 {
            100
        }
        fn position_secs(&self) -> u64 {
            0
        }
        fn is_playing(&self) -> bool {
            false
        }
        fn last_requested(&self) -> Option<&str> {
            None
        }
        fn stream_prefetcher(&self) -> Option<YtDlpResolver> {
            Some(self.resolver.clone())
        }
    }

    #[test]
    fn spawn_stream_prefetch_spawns_thread_and_warms_cache() {
        let resolver = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let video_id = "prefetch-warm-id";
        resolver.cache().insert(
            video_id,
            AudioQuality::Standard,
            "https://googlevideo.test/stream".into(),
            std::time::Instant::now(),
        );

        let backend = Arc::new(Mutex::new(MockPrefetchBackend { resolver: resolver.clone() }));
        let handle = spawn_stream_prefetch(Some(video_id.into()), &backend);
        let handle = handle.expect("prefetch thread spawned");
        handle.join().expect("prefetch thread join succeeds");

        assert!(
            resolver
                .cache()
                .get(video_id, AudioQuality::Standard, std::time::Instant::now())
                .is_some()
        );
    }
}
