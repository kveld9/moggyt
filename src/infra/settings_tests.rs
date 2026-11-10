#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("moggyt-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            let _ = std::fs::create_dir_all(&path);
            Self(path)
        }

        fn path(&self) -> PathBuf {
            self.0.clone()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn corrupt_files(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().contains(".corrupt."))
            .collect()
    }

    #[test]
    fn test_1_given_no_settings_file_when_loaded_then_returns_default() {
        let temp = TempDir::new("test-1");
        let store = DiskSettingsStore::new(temp.path());
        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn test_damaged_settings_json_is_quarantined_not_overwritten() {
        let temp = TempDir::new("test-dmg");
        let store = DiskSettingsStore::new(temp.path());
        let payload = b"{\"theme\": \"white\", \"locale\": ";
        std::fs::write(store.file_path(), payload).unwrap();

        assert_eq!(store.load(), Settings::default());
        assert!(store.is_quarantined());
        assert!(!store.file_path().exists());

        let corrupt = corrupt_files(&temp.path());
        assert_eq!(corrupt.len(), 1);
        assert_eq!(std::fs::read(&corrupt[0]).unwrap(), payload);
        assert!(!store.file_path().exists());
    }

    #[test]
    fn second_corruption_preserves_first_backup() {
        let temp = TempDir::new("test-dmg2");
        let store = DiskSettingsStore::new(temp.path());
        let payload1 = b"corrupt payload 1";
        std::fs::write(store.file_path(), payload1).unwrap();
        assert_eq!(store.load(), Settings::default());

        let payload2 = b"corrupt payload 2";
        std::fs::write(store.file_path(), payload2).unwrap();
        assert_eq!(store.load(), Settings::default());

        let files = corrupt_files(&temp.path());
        assert_eq!(files.len(), 2);
        let contents: Vec<_> = files.iter().map(|p| std::fs::read(p).unwrap()).collect();
        assert!(contents.contains(&payload1.to_vec()) && contents.contains(&payload2.to_vec()));
    }

    #[test]
    fn newer_settings_schema_is_incompatible_not_empty() {
        let temp = TempDir::new("test-newer");
        let store = DiskSettingsStore::new(temp.path());
        let payload = br#"{"version": 2, "theme": "white", "locale": "en"}"#;
        std::fs::write(store.file_path(), payload).unwrap();

        assert_eq!(store.load(), Settings::default());
        assert_eq!(store.health(), StoreHealth::Incompatible);
        assert!(!store.is_quarantined());
        assert_eq!(std::fs::read(store.file_path()).unwrap(), payload);
    }

    #[test]
    fn settings_without_version_still_loads() {
        let temp = TempDir::new("test-nover");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), br#"{"theme": "black-oled", "locale": "en"}"#).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.theme, ThemeId::BlackOled);
        assert_eq!(store.health(), StoreHealth::Ok);
    }

    #[test]
    fn save_writes_current_schema_version() {
        let temp = TempDir::new("test-save-ver");
        let store = DiskSettingsStore::new(temp.path());
        store.save(&Settings::default()).expect("save succeeds");

        let bytes = std::fs::read(store.file_path()).expect("read file");
        let val: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
        assert_eq!(val["version"], SETTINGS_SCHEMA_VERSION);
    }

    #[test]
    fn clear_health_allows_future_loads() {
        let temp = TempDir::new("test-clr-health");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), b"broken payload").unwrap();
        let _ = store.load();
        assert!(store.is_quarantined());

        store.clear_health();
        assert_eq!(store.health(), StoreHealth::Ok);
    }

    #[test]
    fn test_quarantined_settings_block_writes_until_ack() {
        let temp = TempDir::new("test-block-ack");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), b"broken payload").unwrap();
        let _ = store.load();
        assert!(store.is_quarantined());

        assert!(store.save(&Settings::default()).is_err());
        store.clear_health();
        assert!(store.save(&Settings::default()).is_ok());
    }

    #[test]
    fn test_5_given_settings_change_when_saved_then_round_trips_through_load_with_atomic_write() {
        let temp = TempDir::new("test-5");
        let store = DiskSettingsStore::new(temp.path());

        let modified = Settings {
            version: SETTINGS_SCHEMA_VERSION,
            theme: ThemeId::BlackOled,
            custom_theme_name: Some("rose-pine".to_string()),
            locale: Locale::Spanish,
            volume: 75,
            playback_speed: 1.25,
            crossfade_seconds: 5,
            stream_cache_limit_mb: 1024,
            quality: AudioQuality::High,
            discord_presence_enabled: true,
            lastfm_api_key: "api-key-abc".to_string(),
            lastfm_api_secret: "api-secret-xyz".to_string(),
            lastfm_enabled: true,
            lastfm_username: "testuser".to_string(),
            secondary_lastfm_enabled: true,
            hi_res_covers: true,
            songs_only: true,
            skip_silence: true,
            loudness_normalization: true,
            player_bar_mode: PlayerBarMode::Floating,
            player_bar_show_skip_seconds: true,
            player_bar_show_artwork: false,
            player_bar_show_like: false,
            player_bar_show_shuffle: false,
            player_bar_show_repeat: false,
            player_bar_show_queue: false,
            player_bar_show_volume: false,
            ..Settings::default()
        };

        store.save(&modified).expect("save should succeed");
        assert_eq!(store.load(), modified);
        assert!(store.file_path().exists());
    }

    #[test]
    fn test_8_given_fallback_settings_when_constructed_then_locale_is_detected() {
        let fallback = Settings::fallback();
        assert_eq!(fallback.theme, ThemeId::White);
        assert_eq!(fallback.locale, Locale::detect());
        assert_eq!(fallback.volume, 100);
        assert_eq!(fallback, Settings { locale: Locale::detect(), ..Settings::default() });
    }

    #[test]
    fn test_old_schema_settings_json_backward_compatibility() {
        let json = r#"{"theme": "white", "locale": "en"}"#;
        let loaded: Settings = serde_json::from_str(json).expect("deserialization succeeds");
        assert!(loaded.lastfm_api_key.is_empty());
        assert!(loaded.lastfm_api_secret.is_empty());
        assert_eq!(loaded, Settings::default());
    }

    #[test]
    fn test_lastfm_credentials_serde_defaults_and_roundtrip() {
        let empty: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert!(empty.lastfm_api_key.is_empty() && empty.lastfm_api_secret.is_empty());
        let json = r#"{"lastfm_api_key": "my-key", "lastfm_api_secret": "my-secret"}"#;
        let loaded: Settings = serde_json::from_str(json).expect("deserialize explicit");
        assert_eq!(loaded.lastfm_api_key, "my-key");
        assert_eq!(loaded.lastfm_api_secret, "my-secret");
        let roundtrip = serde_json::to_string(&loaded).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert_eq!(decoded.lastfm_api_key, "my-key");
        assert_eq!(decoded.lastfm_api_secret, "my-secret");
    }

    #[test]
    fn test_songs_only_setting_defaults_false_and_roundtrips() {
        let default_settings = Settings::default();
        assert!(!default_settings.songs_only);
        let missing_key: Settings = serde_json::from_str("{}").expect("deserialize empty json");
        assert!(!missing_key.songs_only);
        let json_true = r#"{"songs_only": true}"#;
        let loaded: Settings = serde_json::from_str(json_true).expect("deserialize true");
        assert!(loaded.songs_only);
        let roundtrip = serde_json::to_string(&loaded).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert!(decoded.songs_only);
    }

    #[test]
    fn test_player_bar_settings_serde_defaults_and_explicit_deserialization() {
        let empty: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(empty.player_bar_mode, PlayerBarMode::Classic);
        assert!(empty.player_bar_show_artwork && empty.player_bar_show_like);
        assert!(empty.player_bar_show_shuffle && empty.player_bar_show_repeat);
        assert!(!empty.player_bar_show_skip_seconds && empty.player_bar_show_queue);
        assert!(empty.player_bar_show_volume);

        let explicit_json = r#"{
            "player_bar_mode": "floating", "player_bar_show_artwork": false,
            "player_bar_show_like": false, "player_bar_show_shuffle": false,
            "player_bar_show_repeat": false, "player_bar_show_skip_seconds": true,
            "player_bar_show_queue": false, "player_bar_show_volume": false
        }"#;
        let explicit: Settings = serde_json::from_str(explicit_json).expect("deserialize explicit");
        assert_eq!(explicit.player_bar_mode, PlayerBarMode::Floating);
        assert!(!explicit.player_bar_show_artwork && !explicit.player_bar_show_like);
        assert!(!explicit.player_bar_show_shuffle && !explicit.player_bar_show_repeat);
        assert!(explicit.player_bar_show_skip_seconds && !explicit.player_bar_show_queue);
        assert!(!explicit.player_bar_show_volume);
    }

    #[test]
    fn test_volume_clamped_on_load_from_disk() {
        let temp = TempDir::new("test-clamp");
        let store = DiskSettingsStore::new(temp.path());

        let json = r#"{"theme": "mono", "locale": "es", "volume": 150, "quality": "compact"}"#;
        std::fs::write(store.file_path(), json).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.theme, ThemeId::Mono);
        assert_eq!(loaded.locale, Locale::Spanish);
        assert_eq!(loaded.volume, 100);
        assert_eq!(loaded.playback_speed, 1.0);
        assert_eq!(loaded.quality, AudioQuality::Compact);
        assert!(!loaded.discord_presence_enabled && !loaded.lastfm_enabled);
        assert!(loaded.lastfm_username.is_empty() && !loaded.secondary_lastfm_enabled);
        assert!(!loaded.hi_res_covers && !loaded.skip_silence && !loaded.loudness_normalization);
        assert_eq!(loaded.player_bar_mode, PlayerBarMode::Classic);
        assert!(loaded.player_bar_show_artwork && loaded.player_bar_show_like);
        assert!(loaded.player_bar_show_shuffle && loaded.player_bar_show_repeat);
        assert!(!loaded.player_bar_show_skip_seconds && loaded.player_bar_show_queue);
        assert!(loaded.player_bar_show_volume);
        assert_eq!(loaded.crossfade_seconds, 0);
    }

    #[test]
    fn test_legacy_keyboard_shortcuts_key_is_ignored_on_load() {
        let legacy_json = r#"{"keyboard_shortcuts": false, "volume": 80}"#;
        let loaded: Settings = serde_json::from_str(legacy_json)
            .expect("deserialize legacy settings with keyboard_shortcuts");
        assert_eq!(loaded.volume, 80);
    }

    #[test]
    fn test_discord_presence_setting_defaults_false_and_roundtrips() {
        let default_settings = Settings::default();
        assert!(!default_settings.discord_presence_enabled);
        let missing_key: Settings = serde_json::from_str("{}").expect("deserialize empty json");
        assert!(!missing_key.discord_presence_enabled);
        let json_true = r#"{"discord_presence_enabled": true}"#;
        let loaded: Settings = serde_json::from_str(json_true).expect("deserialize true");
        assert!(loaded.discord_presence_enabled);
        let roundtrip = serde_json::to_string(&loaded).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert!(decoded.discord_presence_enabled);
    }

    #[test]
    fn test_lastfm_and_hires_covers_serde_defaults_and_explicit_deserialization() {
        let with_both_false: Settings =
            serde_json::from_str(r#"{"lastfm_enabled": false, "hi_res_covers": false}"#)
                .expect("deserialize false");
        assert!(!with_both_false.lastfm_enabled && with_both_false.lastfm_username.is_empty());
        assert!(!with_both_false.hi_res_covers);
        let with_both_true: Settings = serde_json::from_str(
            r#"{"lastfm_enabled": true, "lastfm_username": "moggy", "hi_res_covers": true}"#,
        )
        .expect("deserialize true");
        assert!(with_both_true.lastfm_enabled && with_both_true.hi_res_covers);
        assert_eq!(with_both_true.lastfm_username, "moggy");
        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert!(!omitted.lastfm_enabled && omitted.lastfm_username.is_empty());
        assert!(!omitted.hi_res_covers);
    }

    #[test]
    fn test_dsp_settings_serde_defaults_and_explicit_deserialization() {
        let with_both_false: Settings =
            serde_json::from_str(r#"{"skip_silence": false, "loudness_normalization": false}"#)
                .expect("deserialize false");
        assert!(!with_both_false.skip_silence && !with_both_false.loudness_normalization);

        let with_both_true: Settings =
            serde_json::from_str(r#"{"skip_silence": true, "loudness_normalization": true}"#)
                .expect("deserialize true");
        assert!(with_both_true.skip_silence && with_both_true.loudness_normalization);

        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert!(!omitted.skip_silence && !omitted.loudness_normalization);
    }

    #[test]
    fn test_secondary_lastfm_serde_back_compat() {
        // Old settings payload lacking secondary_lastfm_enabled
        let json = r#"{"theme": "white", "locale": "en", "lastfm_enabled": true}"#;
        let loaded: Settings = serde_json::from_str(json).expect("deserialization succeeds");
        assert!(loaded.lastfm_enabled);
        assert!(!loaded.secondary_lastfm_enabled);

        // Explicit true deserializes as true
        let loaded_true: Settings =
            serde_json::from_str(r#"{"secondary_lastfm_enabled": true}"#).unwrap();
        assert!(loaded_true.secondary_lastfm_enabled);

        // Empty object defaults to false
        let empty: Settings = serde_json::from_str("{}").expect("deserialization succeeds");
        assert!(!empty.secondary_lastfm_enabled);
    }

    #[test]
    fn test_playback_speed_serde_defaults_and_explicit_deserialization() {
        let explicit: Settings =
            serde_json::from_str(r#"{"playback_speed": 1.5}"#).expect("deserialize explicit");
        assert_eq!(explicit.playback_speed, 1.5);

        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(omitted.playback_speed, 1.0);
    }

    #[test]
    fn test_playback_speed_clamped_on_load_from_disk() {
        let temp = TempDir::new("test-speed-clamp");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), r#"{"playback_speed": 99.0}"#).unwrap();
        assert_eq!(store.load().playback_speed, 2.0);
        std::fs::write(store.file_path(), r#"{"playback_speed": 0.1}"#).unwrap();
        assert_eq!(store.load().playback_speed, 0.5);
        std::fs::write(store.file_path(), r#"{"playback_speed": -1.0}"#).unwrap();
        assert_eq!(store.load().playback_speed, 1.0);
    }

    #[test]
    fn test_recent_searches_missing_field_loads_empty() {
        let json = r#"{"theme": "white", "locale": "en"}"#;
        let loaded: Settings = serde_json::from_str(json).expect("deserialization succeeds");
        assert!(loaded.recent_searches.is_empty());
        assert_eq!(loaded.recent_searches.len(), 0);
    }

    #[test]
    fn test_recent_searches_round_trips_preserving_order() {
        let mut recents = RecentSearches::default();
        recents.record("first");
        recents.record("second");
        recents.record("third");

        let settings = Settings { recent_searches: recents.clone(), ..Settings::default() };
        let json = serde_json::to_string(&settings).expect("serialize settings");
        let loaded: Settings = serde_json::from_str(&json).expect("deserialize settings");
        assert_eq!(loaded.recent_searches, recents);
        assert_eq!(loaded.recent_searches.entries(), &["third", "second", "first"]);
    }

    #[test]
    fn test_crossfade_seconds_serde_defaults_and_explicit_deserialization() {
        let explicit: Settings =
            serde_json::from_str(r#"{"crossfade_seconds": 8}"#).expect("deserialize explicit");
        assert_eq!(explicit.crossfade_seconds, 8);
        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(omitted.crossfade_seconds, 0);
        let roundtrip = serde_json::to_string(&explicit).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert_eq!(decoded.crossfade_seconds, 8);
    }

    #[test]
    fn test_crossfade_seconds_clamped_on_load_from_disk() {
        let temp = TempDir::new("test-crossfade-clamp");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), r#"{"crossfade_seconds": 99}"#).unwrap();
        assert_eq!(store.load().crossfade_seconds, MAX_CROSSFADE_SECONDS);
        std::fs::write(store.file_path(), r#"{"crossfade_seconds": 0}"#).unwrap();
        assert_eq!(store.load().crossfade_seconds, 0);
        std::fs::write(store.file_path(), r#"{"crossfade_seconds": 7}"#).unwrap();
        assert_eq!(store.load().crossfade_seconds, 7);
    }

    #[test]
    fn test_clamp_crossfade_seconds_boundary_cases() {
        assert_eq!(clamp_crossfade_seconds(0), 0);
        assert_eq!(clamp_crossfade_seconds(1), 1);
        assert_eq!(clamp_crossfade_seconds(15), 15);
        assert_eq!(clamp_crossfade_seconds(16), 15);
        assert_eq!(clamp_crossfade_seconds(100), 15);
    }

    #[test]
    fn test_stream_cache_limit_mb_serde_defaults_and_explicit_deserialization() {
        let explicit: Settings = serde_json::from_str(r#"{"stream_cache_limit_mb": 1024}"#)
            .expect("deserialize explicit");
        assert_eq!(explicit.stream_cache_limit_mb, 1024);
        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(omitted.stream_cache_limit_mb, 512);
        let roundtrip = serde_json::to_string(&explicit).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert_eq!(decoded.stream_cache_limit_mb, 1024);
    }

    #[test]
    fn test_stream_cache_limit_mb_clamped_on_sanitize_and_load() {
        let mut settings = Settings { stream_cache_limit_mb: 20000, ..Settings::default() };
        settings.sanitize();
        assert_eq!(settings.stream_cache_limit_mb, 10240);

        let temp = TempDir::new("test-stream-cache-clamp");
        let store = DiskSettingsStore::new(temp.path());
        std::fs::write(store.file_path(), r#"{"stream_cache_limit_mb": 50000}"#).unwrap();
        assert_eq!(store.load().stream_cache_limit_mb, 10240);
    }

    #[test]
    fn test_custom_theme_name_serde_defaults_and_explicit_deserialization() {
        let explicit: Settings = serde_json::from_str(r#"{"custom_theme_name": "rose-pine"}"#)
            .expect("deserialize explicit");
        assert_eq!(explicit.custom_theme_name.as_deref(), Some("rose-pine"));
        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(omitted.custom_theme_name, None);
        let roundtrip = serde_json::to_string(&explicit).expect("serialize settings");
        let decoded: Settings = serde_json::from_str(&roundtrip).expect("deserialize roundtrip");
        assert_eq!(decoded.custom_theme_name.as_deref(), Some("rose-pine"));
    }

    #[test]
    fn test_sponsorblock_enabled_serde_defaults_and_explicit_deserialization() {
        let explicit: Settings = serde_json::from_str(r#"{"sponsorblock_enabled": false}"#)
            .expect("deserialize explicit");
        assert!(!explicit.sponsorblock_enabled);
        let omitted: Settings = serde_json::from_str("{}").expect("deserialize empty");
        assert!(omitted.sponsorblock_enabled);
    }
}
