use super::*;
use crate::infra::YtDlpResolver;
use std::time::Instant;

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

    // A seek with no loaded source must not fabricate a playback position.
    backend.seek(15);
    assert_eq!(backend.position_secs(), 0);

    backend.pause();
    assert_eq!(backend.position_secs(), 0);
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
        backend.resolver.cache().get(track_id, AudioQuality::Standard, Instant::now()).is_some()
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
    b'R', b'I', b'F', b'F', 36, 0, 0, 0, b'W', b'A', b'V', b'E', b'f', b'm', b't', b' ', 16, 0, 0,
    0, 1, 0, 1, 0, 68, 172, 0, 0, 136, 88, 1, 0, 2, 0, 16, 0, b'd', b'a', b't', b'a', 0, 0, 0, 0,
];

#[test]
fn backend_stream_cache_accessors() {
    let temp_dir =
        std::env::temp_dir().join(format!("moggyt-test-backend-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let cache = Arc::new(StreamDiskCache::new(&temp_dir, 1024 * 1024).expect("cache init"));
    cache
        .put("cached-track-1", AudioQuality::Standard, &MINIMAL_WAV_HEADER)
        .expect("put wav in cache");

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
    cache
        .put("cached-track-1", AudioQuality::Standard, &MINIMAL_WAV_HEADER)
        .expect("put wav in cache");

    let mut backend =
        match RodioBackend::with_cache("/definitely/missing/binary/yt-dlp", Some(cache)) {
            Ok(backend) => backend,
            Err(err) => {
                eprintln!("Skipping audio hardware assertions in headless environment: {err:#}");
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
    backend.apply_playback_source("track-1", Box::new(source1), None);

    assert!(!Arc::ptr_eq(&initial_sink, &backend.sink));
    let second_sink = Arc::clone(&backend.sink);
    assert_eq!(backend.last_requested(), Some("track-1"));
    assert!(backend.is_playing());

    let source2 =
        Decoder::new(std::io::Cursor::new(MINIMAL_WAV_HEADER)).expect("valid wav decoder 2");
    backend.apply_playback_source("track-2", Box::new(source2), None);

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

#[test]
fn clamp_playback_position_never_exceeds_duration() {
    let secs = Duration::from_secs;
    assert_eq!(clamp_playback_position(secs(5), Some(secs(3))), secs(3));
    assert_eq!(clamp_playback_position(secs(2), Some(secs(3))), secs(2));
    assert_eq!(clamp_playback_position(secs(300), None), secs(300));
}
