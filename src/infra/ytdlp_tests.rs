#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;

    #[test]
    fn watch_url_shape() {
        assert_eq!(watch_url("dQw4w9WgXcQ"), "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(watch_url("abc-123_XYZ"), "https://www.youtube.com/watch?v=abc-123_XYZ");
    }

    #[test]
    fn parse_version_variants() {
        let cases = [
            ("2024.10.07\n", Some("2024.10.07".to_string())),
            ("\n  \n2025.01.15\nsecond line\n", Some("2025.01.15".to_string())),
            ("", None),
            ("   \n\t  \r\n  ", None),
        ];

        for (input, expected) in cases {
            assert_eq!(parse_version(input), expected, "failed on input {input:?}");
        }
    }

    #[test]
    fn sidecar_needs_update_edges() {
        let cases = [
            (0, false),
            (YTDLP_UPDATE_INTERVAL_SECS - 1, false),
            (YTDLP_UPDATE_INTERVAL_SECS, true),
            (YTDLP_UPDATE_INTERVAL_SECS + 1, true),
            (u64::MAX, true),
        ];

        for (age, expected) in cases {
            assert_eq!(sidecar_needs_update(age), expected, "failed for age {age}");
        }
    }

    #[test]
    fn format_args_exact_vec() {
        let args = format_args("dQw4w9WgXcQ");
        let expected = vec![
            "-f".to_string(),
            "bestaudio[ext=m4a]/bestaudio/best".to_string(),
            "-g".to_string(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
        ];
        assert_eq!(args, expected);
    }

    #[test]
    fn release_asset_name_non_empty_and_valid() {
        let name = release_asset_name();
        assert!(!name.is_empty());
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        assert_eq!(name, "yt-dlp_linux");
    }

    #[test]
    fn resolve_soft_fails_on_blank_id_without_spawning() {
        let resolver = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        assert_eq!(resolver.resolve(""), None);
        assert_eq!(resolver.resolve("   "), None);
    }

    #[test]
    fn resolve_soft_fails_on_missing_binary() {
        let resolver = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        assert_eq!(resolver.resolve("dQw4w9WgXcQ"), None);
        assert!(resolver.fetch_audio_url("dQw4w9WgXcQ").is_err());
    }

    #[test]
    fn format_args_with_quality_table() {
        for (q, expected_fmt) in [
            (AudioQuality::Compact, "bestaudio[ext=m4a][abr<=64]/bestaudio[abr<=64]"),
            (AudioQuality::Standard, "bestaudio[ext=m4a]/bestaudio/best"),
            (
                AudioQuality::High,
                "bestaudio[ext=webm][acodec=opus]/bestaudio[ext=m4a]/bestaudio/best",
            ),
        ] {
            let args = format_args_with_quality("v123", q);
            assert_eq!(
                args,
                vec![
                    "-f".to_string(),
                    expected_fmt.to_string(),
                    "-g".to_string(),
                    "https://www.youtube.com/watch?v=v123".to_string(),
                ]
            );
        }
    }

    #[test]
    fn is_refresh_due_edges() {
        assert!(is_refresh_due(None));
        assert!(!is_refresh_due(Some(0)));
        assert!(!is_refresh_due(Some(YTDLP_REFRESH_COOLDOWN_SECS - 1)));
        assert!(is_refresh_due(Some(YTDLP_REFRESH_COOLDOWN_SECS)));
    }

    #[test]
    fn release_download_url_format() {
        let url = release_download_url();
        assert!(url.starts_with("https://github.com/yt-dlp/yt-dlp/releases/latest/download/"));
        assert!(url.ends_with(release_asset_name()));
    }

    #[test]
    fn refresh_sidecar_if_due_skips_when_binary_missing() {
        let missing = Path::new("/definitely/missing/binary/yt-dlp");
        assert!(!refresh_sidecar_if_due(missing));
    }

    #[test]
    fn resolver_default_quality_is_standard_and_mutators_work() {
        let mut resolver = YtDlpResolver::new("/bin/dummy");
        assert_eq!(resolver.quality(), AudioQuality::Standard);

        resolver.set_quality(AudioQuality::High);
        assert_eq!(resolver.quality(), AudioQuality::High);

        resolver.set_quality(AudioQuality::Compact);
        assert_eq!(resolver.quality(), AudioQuality::Compact);
    }

    #[test]
    fn resolver_cache_hit_avoids_spawning_and_different_quality_misses() {
        let resolver = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let video_id = "dQw4w9WgXcQ";
        let cached_url = "https://googlevideo.example.com/audio-stream";
        resolver.cache().insert(
            video_id,
            AudioQuality::Standard,
            cached_url.to_string(),
            Instant::now(),
        );

        // Hit returns cached URL without spawning missing binary
        assert_eq!(resolver.resolve(video_id), Some(cached_url.to_string()));

        // Different quality misses cache and fails on missing binary
        let mut high_resolver = resolver.clone();
        high_resolver.set_quality(AudioQuality::High);
        assert_eq!(high_resolver.resolve(video_id), None);

        // Invalidation removes cached URL
        resolver.invalidate(video_id);
        assert_eq!(resolver.resolve(video_id), None);
    }

    #[test]
    fn resolver_prefetch_returns_true_when_cached_and_false_on_missing_binary() {
        let resolver = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let video_id = "dQw4w9WgXcQ";
        assert!(!resolver.prefetch(video_id));
        assert!(!resolver.prefetch(""));
        assert!(!resolver.prefetch("   "));

        resolver.cache().insert(
            video_id,
            AudioQuality::Standard,
            "https://cached.test/stream".into(),
            Instant::now(),
        );
        assert!(resolver.prefetch(video_id));
    }

    #[cfg(unix)]
    fn create_fake_sidecar(path: &Path) {
        std::fs::write(path, b"#!/bin/sh\necho 2024.01.01\n").expect("write fake sidecar");
        set_executable_permissions(path).expect("set executable permissions");
    }

    struct TempDirGuard(PathBuf);

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn test_temp_dir(tag: &str) -> (TempDirGuard, PathBuf) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("moggyt-test-ytdlp-{tag}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test temp dir");
        (TempDirGuard(path.clone()), path)
    }

    #[test]
    fn zero_byte_binary_rejected() {
        let (_guard, dir) = test_temp_dir("zero-byte");
        let binary = dir.join(sidecar_binary_name());
        std::fs::write(&binary, b"").expect("write empty file");
        let _ = set_executable_permissions(&binary);
        assert!(!is_valid_sidecar(&binary));
    }

    #[test]
    fn html_payload_rejected() {
        let (_guard, dir) = test_temp_dir("html-payload");
        let binary = dir.join(sidecar_binary_name());
        std::fs::write(&binary, b"  \n<html><body>502 Bad Gateway</body></html>")
            .expect("write html");
        let _ = set_executable_permissions(&binary);
        assert!(!is_valid_sidecar(&binary));
    }

    #[test]
    #[cfg(unix)]
    fn stale_binary_used_when_refresh_fails() {
        use std::fs::{File, FileTimes};
        use std::time::{Duration, SystemTime};

        let (_guard, data_dir) = test_temp_dir("stale-binary");
        let binary = data_dir.join(sidecar_binary_name());
        create_fake_sidecar(&binary);

        let file = File::open(&binary).expect("open binary");
        let two_days_ago = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);
        file.set_times(FileTimes::new().set_modified(two_days_ago)).expect("set modified time");
        drop(file);

        let provisioned = provision_sidecar(&data_dir).expect("should provision usable binary");
        assert_eq!(provisioned, binary);
    }

    #[test]
    #[cfg(unix)]
    fn cooldown_does_not_block_usable_binary() {
        let (_guard, data_dir) = test_temp_dir("cooldown");
        let binary = data_dir.join(sidecar_binary_name());
        create_fake_sidecar(&binary);

        {
            let mut guard = REFRESH_STATE.lock().unwrap();
            *guard = Some(Instant::now());
        }

        let provisioned = provision_sidecar(&data_dir).expect("should provision usable binary");
        assert_eq!(provisioned, binary);

        {
            let mut guard = REFRESH_STATE.lock().unwrap();
            *guard = None;
        }
    }

    #[test]
    #[cfg(unix)]
    fn provision_creates_data_dir() {
        let (_guard, base) = test_temp_dir("creates-dir");
        let sub_dir = base.join("nested_missing");
        let _ = std::fs::remove_dir_all(&sub_dir);
        assert!(!sub_dir.exists());

        std::fs::create_dir_all(&sub_dir).expect("create sub_dir");
        let binary = sub_dir.join(sidecar_binary_name());
        create_fake_sidecar(&binary);

        let provisioned = provision_sidecar(&sub_dir).expect("provision should succeed");
        assert_eq!(provisioned, binary);
        assert!(sub_dir.is_dir());
    }
}
