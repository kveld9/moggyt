//! Background stream URL prefetching for upcoming queue tracks off the UI thread.

use std::sync::{Arc, Mutex};

use tracing::{debug, warn};

use crate::infra::AudioBackend;

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
    let (prefetcher, has_cached_stream) = {
        let guard = audio.lock().ok()?;
        (guard.stream_prefetcher()?, guard.has_cached_stream(trimmed))
    };
    let target_id = trimmed.to_string();

    match std::thread::Builder::new().name("moggyt-stream-prefetch".into()).spawn(move || {
        if !prefetcher.prefetch(&target_id) {
            debug!(video_id = %target_id, "failed to prefetch stream URL");
            return;
        }
        if !has_cached_stream {
            prefetcher.prefetch_prefix(&target_id);
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
    use crate::infra::{AudioError, AudioQuality, AudioStub, StreamChain, YtDlpResolver};

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
        chain: StreamChain,
        has_cached: bool,
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
        fn stream_prefetcher(&self) -> Option<StreamChain> {
            Some(self.chain.clone())
        }
        fn has_cached_stream(&self, _track_id: &str) -> bool {
            self.has_cached
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

        let chain = StreamChain::from_ytdlp(resolver.clone());
        let backend = Arc::new(Mutex::new(MockPrefetchBackend { chain, has_cached: false }));
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

    #[derive(Default)]
    struct CountingPrefixFetcher {
        calls: Mutex<usize>,
    }

    impl CountingPrefixFetcher {
        fn call_count(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }

    impl crate::infra::PrefixFetcher for CountingPrefixFetcher {
        fn fetch_prefix(
            &self,
            _url: &str,
            _max_bytes: usize,
        ) -> Option<crate::infra::PrefetchedPrefix> {
            *self.calls.lock().unwrap() += 1;
            Some(crate::infra::PrefetchedPrefix { bytes: vec![0xAA; 512], total_len: None })
        }
    }

    #[test]
    fn spawn_stream_prefetch_skips_prefix_when_track_is_cached() {
        let resolver = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let video_id = "cached-track-id";
        resolver.cache().insert(
            video_id,
            AudioQuality::Standard,
            "https://googlevideo.test/cached-stream".into(),
            std::time::Instant::now(),
        );
        let fetcher = Arc::new(CountingPrefixFetcher::default());
        let chain = StreamChain::from_ytdlp(resolver)
            .with_prefix_fetcher(Arc::clone(&fetcher) as Arc<dyn crate::infra::PrefixFetcher>);

        let backend = Arc::new(Mutex::new(MockPrefetchBackend { chain, has_cached: true }));
        let handle = spawn_stream_prefetch(Some(video_id.into()), &backend);
        let handle = handle.expect("prefetch thread spawned");
        handle.join().expect("prefetch thread join succeeds");

        assert_eq!(fetcher.call_count(), 0);
    }

    #[test]
    fn spawn_stream_prefetch_fetches_prefix_when_track_is_not_cached() {
        let resolver = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let video_id = "uncached-track-id";
        resolver.cache().insert(
            video_id,
            AudioQuality::Standard,
            "https://googlevideo.test/uncached-stream".into(),
            std::time::Instant::now(),
        );
        let fetcher = Arc::new(CountingPrefixFetcher::default());
        let chain = StreamChain::from_ytdlp(resolver)
            .with_prefix_fetcher(Arc::clone(&fetcher) as Arc<dyn crate::infra::PrefixFetcher>);

        let backend = Arc::new(Mutex::new(MockPrefetchBackend { chain, has_cached: false }));
        let handle = spawn_stream_prefetch(Some(video_id.into()), &backend);
        let handle = handle.expect("prefetch thread spawned");
        handle.join().expect("prefetch thread join succeeds");

        assert_eq!(fetcher.call_count(), 1);
    }
}
