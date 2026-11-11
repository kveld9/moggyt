//! Slint UI application wiring adapter.
//!
//! This module only wires injected ports.
//! Hexagonal architecture invariant: this module connects injected port adapters
//! to [`crate::slint_shell::MoggytWindow`] without containing direct I/O or private domain rules.

use std::sync::{Arc, Mutex};

use slint::ComponentHandle;

use crate::domain::ThemeId;
use crate::infra::{
    AudioBackend, Catalog, CoverCache, CustomThemeFile, DeviceIdentityMonitor, DiscordPresence,
    LyricOverrideDir, LyricsSource, PlayLogStore, QueueStore, Scrobbler, SessionStore, Settings,
    SettingsStore, SingleInstanceGuard, SkipStore, SponsorBlockClient, TrayHandle,
    VersionOverrideStore,
};

pub use crate::slint_shell::covers::{
    request_detail_covers, request_explore_covers, request_library_covers,
    request_queue_track_cover,
};
pub use crate::slint_shell::explore::execute_explore;
pub use crate::slint_shell::format::{
    PLAYBACK_SPEED_PRESETS, current_unix_secs, cycle_playback_speed_preset, format_playback_speed,
    format_queue_label, format_sleep_timer_status, format_update_status, timer_preset_from_index,
    timer_preset_to_index,
};
pub use crate::slint_shell::labels::{StaticLabels, sync_static_labels};
pub use crate::slint_shell::page::SlintPage;
pub use crate::slint_shell::playback_control::apply_play_track_completion;
pub use crate::slint_shell::player_bar::PlayerBarPrefs;
pub use crate::slint_shell::state::{
    PersistenceError, PersistenceStore, SharedAudio, SharedCatalog, SharedCovers, SharedLyrics,
    SharedState, SlintAppState,
};
pub use crate::slint_shell::window::build_window;
use crate::slint_shell::window::with_ui_state;

use crate::slint_shell::init::{handle_cli_args, spawn_updater_check};

/// Collaborator dependencies and platform handles injected into the Slint application shell.
pub struct AppDependencies<C, A, S, L, K> {
    pub instance: SingleInstanceGuard,
    pub catalog: C,
    pub audio: A,
    pub session: S,
    pub lyrics: L,
    pub covers: K,
    pub settings: Settings,
    pub settings_store: Box<dyn SettingsStore>,
    pub queue_store: Box<dyn QueueStore>,
    pub skip_store: Box<dyn SkipStore>,
    pub play_log_store: Box<dyn PlayLogStore>,
    pub scrobbler: Box<dyn Scrobbler>,
    pub secondary_scrobbler: Option<Box<dyn Scrobbler>>,
    pub lyric_overrides: Option<LyricOverrideDir>,
    pub version_store: Box<dyn VersionOverrideStore>,
    pub tray: Option<TrayHandle>,
    /// Loaded custom theme file watcher for runtime reloading.
    pub custom_theme: CustomThemeFile,
}

/// Composition function launching the Slint desktop UI wired to injected platform adapters.
///
/// TODO Gap: Settings modal: DiskSettingsStore persists volume, speed, quality, DSP
/// toggles, and Last.fm credentials; playback speed and volume are fully wired, but
/// ui/shell.slint lacks an interactive settings dialog for DSP and credential management.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub fn run(
    deps: AppDependencies<
        impl Catalog + Send + 'static,
        impl AudioBackend + Send + 'static,
        impl SessionStore + 'static,
        impl LyricsSource + Send + Sync + 'static,
        impl CoverCache + Send + 'static,
    >,
) -> anyhow::Result<()> {
    let AppDependencies {
        instance: _instance,
        catalog,
        audio,
        session,
        lyrics,
        covers,
        settings,
        settings_store,
        queue_store,
        skip_store,
        play_log_store,
        scrobbler,
        secondary_scrobbler,
        lyric_overrides,
        version_store,
        tray,
        custom_theme,
    } = deps;

    let mut state = SlintAppState::new(catalog, audio, session, lyrics, covers);
    state.device_monitor = Some(DeviceIdentityMonitor::start());
    state.custom_theme = Some(custom_theme);
    state.lyric_overrides = lyric_overrides;
    state.version_overrides = version_store.load();
    state.version_store = version_store;
    state.tray = tray;

    if let Ok(mut a) = state.audio.lock() {
        a.set_volume(settings.volume);
        a.set_speed(settings.playback_speed);
        a.set_quality(settings.quality);
        a.set_skip_silence(settings.skip_silence);
        a.set_loudness_normalization(settings.loudness_normalization);
    }
    if settings.sponsorblock_enabled {
        match SponsorBlockClient::new() {
            Ok(provider) => state.skip_provider = Some(Arc::new(provider)),
            Err(err) => tracing::warn!(error = %err, "failed to build community skip client"),
        }
    }
    state.crossfade_seconds = settings.crossfade_seconds;
    state.stream_cache_limit_mb = settings.stream_cache_limit_mb;
    state.hi_res_covers = settings.hi_res_covers;
    state.songs_only = settings.songs_only;
    if settings.discord_presence_enabled {
        state.discord = DiscordPresence::new();
    }
    state.theme = settings.theme;
    state.locale = settings.locale;
    if let Some(ref name) = settings.custom_theme_name {
        if let Some(ref mut ct) = state.custom_theme {
            ct.select_theme(Some(name));
        }
    }
    if state.theme == ThemeId::Custom {
        state.reload_custom_theme();
    }
    state.player_bar = PlayerBarPrefs::from_settings(&settings);
    state.recent_searches = settings.recent_searches;
    if let Ok(mut c) = state.catalog.lock() {
        c.set_locale(settings.locale);
    }
    state.settings_store = settings_store;

    let saved_queue = queue_store.load();
    if !saved_queue.is_empty() {
        state.queue = saved_queue;
    }
    state.queue_store = queue_store;
    state.persistence_error = detect_persistence_error(&*state.settings_store, &*state.queue_store);

    state.skip_store = skip_store;
    state.play_log_store = play_log_store;

    state.scrobbler = Arc::new(Mutex::new(scrobbler));
    state.secondary_scrobbler = secondary_scrobbler.map(|s| Arc::new(Mutex::new(s)));

    handle_cli_args(&mut state);
    state.updater_rx = Some(spawn_updater_check());

    let (window, _timer) = build_window(state)?;
    window.run().map_err(|err| anyhow::anyhow!("Slint execution failed: {err}"))?;

    let _ = with_ui_state(|st| {
        let _ = persist_state_on_exit(st);
    });

    Ok(())
}

/// Persists the active playback queue to storage on application exit.
pub fn persist_state_on_exit(state: &mut SlintAppState) -> anyhow::Result<()> {
    state.maybe_scrobble_current();
    state.queue_store.save(&state.queue)
}

fn detect_persistence_error(
    settings: &dyn SettingsStore,
    queue: &dyn QueueStore,
) -> Option<PersistenceError> {
    let settings_health = settings.health();
    if settings_health != crate::infra::StoreHealth::Ok {
        return Some(PersistenceError {
            store: PersistenceStore::Settings,
            health: settings_health,
        });
    }
    let queue_health = queue.health();
    if queue_health != crate::infra::StoreHealth::Ok {
        return Some(PersistenceError { store: PersistenceStore::Queue, health: queue_health });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Track;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_thin_composition_reexports_resolve() {
        assert_eq!(SlintPage::Search.to_index(), 0);
        let labels = StaticLabels::resolve(crate::i18n::Locale::English);
        assert_eq!(labels.nav_search, "Search");
        assert_eq!(format_queue_label(crate::i18n::Locale::English, 2), "Queue (2)");
    }

    #[test]
    fn test_persist_state_on_exit_saves_queue() {
        let mut state = test_wired();
        let track1 = Track::new("t1", "Track 1", "Artist 1");
        let track2 = Track::new("t2", "Track 2", "Artist 2");
        state.queue.push(track1.clone());
        state.queue.push(track2.clone());

        persist_state_on_exit(&mut state).expect("failed to persist queue on exit");

        let loaded = state.queue_store.load();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.tracks(), &[track1, track2]);
    }

    #[test]
    fn exit_credits_current_track() {
        let mut state = test_wired();
        let memory_store = crate::infra::MemoryPlayLogStore::new();
        state.play_log_store = Box::new(memory_store.clone());
        let track = Track::new("t1", "Title 1", "Artist 1").with_duration(200);
        state.started_at = Some(crate::slint_shell::playback_control::StartedPlay {
            unix_secs: 1,
            track,
            listened: std::time::Duration::from_secs(120),
            resumed_at: None,
        });

        persist_state_on_exit(&mut state).expect("persist succeeds");

        let records = memory_store.load().expect("load succeeds");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].listened_secs, 120);
        assert!(state.started_at.is_none());

        persist_state_on_exit(&mut state).expect("persist succeeds second time");
        let records2 = memory_store.load().expect("load succeeds second time");
        assert_eq!(records2.len(), 1);
    }

    #[test]
    fn test_detect_persistence_error_reports_quarantined_settings() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-detect-err-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let _ = std::fs::create_dir_all(&temp_dir);

        let settings = crate::infra::DiskSettingsStore::new(temp_dir.clone());
        let queue = crate::infra::DiskQueueStore::new(temp_dir.clone());

        std::fs::write(settings.file_path(), b"broken payload").unwrap();
        let _ = settings.load();

        let err = detect_persistence_error(&settings, &queue);
        assert_eq!(
            err,
            Some(PersistenceError {
                store: PersistenceStore::Settings,
                health: crate::infra::StoreHealth::Quarantined,
            })
        );

        let clean_temp =
            std::env::temp_dir().join(format!("moggyt-detect-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&clean_temp);
        let _ = std::fs::create_dir_all(&clean_temp);
        let clean_settings = crate::infra::DiskSettingsStore::new(clean_temp.clone());
        let clean_queue = crate::infra::DiskQueueStore::new(clean_temp.clone());
        assert_eq!(detect_persistence_error(&clean_settings, &clean_queue), None);

        let _ = std::fs::remove_dir_all(&temp_dir);
        let _ = std::fs::remove_dir_all(&clean_temp);
    }

    #[test]
    fn test_dismiss_persistence_error_clears_error_and_unblocks_stores() {
        let mut state = test_wired();
        state.persistence_error = Some(PersistenceError {
            store: PersistenceStore::Settings,
            health: crate::infra::StoreHealth::Quarantined,
        });

        state.dismiss_persistence_error();
        assert_eq!(state.persistence_error, None);
    }
}
