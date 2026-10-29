//! Slint UI application wiring and composition root adapter.
//!
//! Hexagonal architecture invariant: this module connects injected port adapters
//! to [`MoggytWindow`] without containing direct I/O or private domain rules.

use std::sync::{Arc, Mutex};

use crate::infra::{
    DiskMetaStore, DiskQueueStore, DiskSettingsStore, DiskSkipStore, KeyringSessionStore,
    LastFmClient, LiveCatalog, LrclibClient, LyricOverrideDir, Scrobbler, SingleInstanceGuard,
    TrayHandle, default_config_dir,
};

pub use crate::slint_shell::covers::{
    apply_play_track_completion, request_detail_covers, request_explore_covers,
    request_library_covers, request_queue_track_cover,
};
pub use crate::slint_shell::explore::execute_explore;
pub use crate::slint_shell::format::{
    PLAYBACK_SPEED_PRESETS, current_unix_secs, cycle_playback_speed_preset, format_playback_speed,
    format_queue_label, format_update_status,
};
pub use crate::slint_shell::init::{
    app_data_dir, ensure_data_dir, init_cover_cache, init_lastfm_scrobbler, init_live_audio,
    init_rodio_backend, init_secondary_lastfm_scrobbler, tracing_subscriber_setup,
};
pub use crate::slint_shell::labels::{StaticLabels, sync_static_labels};
pub use crate::slint_shell::page::SlintPage;
pub use crate::slint_shell::state::{SharedState, SlintAppState};
pub use crate::slint_shell::window::build_window;

use crate::slint_shell::init::{check_updater_status, handle_cli_args};

/// Composition function launching the Slint desktop UI wired to live platform adapters.
///
/// TODO Gap: Settings modal: DiskSettingsStore persists volume, speed, quality, DSP
/// toggles, and Last.fm credentials; playback speed and volume are fully wired, but
/// ui/shell.slint lacks an interactive settings dialog for DSP and credential management.
pub fn run() -> anyhow::Result<()> {
    let _instance = match SingleInstanceGuard::try_acquire() {
        Some(g) => g,
        None => {
            eprintln!("moggyt is already running; exiting.");
            return Ok(());
        }
    };

    tracing_subscriber_setup();

    let data_dir = app_data_dir();
    let config_dir = default_config_dir();
    let audio = init_live_audio(&data_dir);
    let catalog = LiveCatalog::new();
    let session = KeyringSessionStore::moggyt();
    let lyrics = LrclibClient::new();
    let covers = init_cover_cache(&data_dir);

    let mut state = SlintAppState::new(catalog, audio, session, lyrics, covers);

    let lyric_overrides = LyricOverrideDir::new(config_dir.join("lyrics"));
    state.lyric_overrides = Some(lyric_overrides);

    if let Some(h) = TrayHandle::try_new() {
        state.tray = Some(h);
    } else {
        tracing::warn!("tray unavailable; running without system tray icon");
    }

    let settings_store = DiskSettingsStore::new(config_dir.clone());
    let settings = settings_store.load();
    if let Ok(mut a) = state.audio.lock() {
        a.set_volume(settings.volume);
        a.set_speed(settings.playback_speed);
        a.set_quality(settings.quality);
        a.set_skip_silence(settings.skip_silence);
        a.set_loudness_normalization(settings.loudness_normalization);
    }
    state.hi_res_covers = settings.hi_res_covers;
    state.theme = settings.theme;
    state.locale = settings.locale;
    if let Ok(mut c) = state.catalog.lock() {
        c.set_locale(settings.locale);
        let refreshed = c.home_shelves();
        if refreshed.is_empty() {
            tracing::warn!(
                "failed to re-fetch home shelves for locale {:?}; retaining fallback shelves",
                settings.locale
            );
        } else {
            state.home_shelves = refreshed;
        }
    }
    state.settings_store = Box::new(settings_store);

    let queue_store = DiskQueueStore::new(config_dir.clone());
    let saved_queue = queue_store.load();
    if !saved_queue.is_empty() {
        state.queue = saved_queue;
    }
    state.queue_store = Box::new(queue_store);

    let skip_store = DiskSkipStore::new(config_dir.clone());
    state.skip_store = Box::new(skip_store);
    let meta_store = DiskMetaStore::new(config_dir);
    state.meta_store = Box::new(meta_store);

    let scrobbler = init_lastfm_scrobbler(settings.lastfm_enabled);
    tracing::debug!("last.fm scrobbler live: {}", scrobbler.is_live());
    state.scrobbler = Arc::new(Mutex::new(Box::new(scrobbler)));

    let secondary_scrobbler = init_secondary_lastfm_scrobbler(settings.secondary_lastfm_enabled);
    let sec_live = secondary_scrobbler.as_ref().is_some_and(LastFmClient::is_live);
    tracing::debug!("secondary last.fm scrobbler live: {sec_live}");
    state.secondary_scrobbler =
        secondary_scrobbler.map(|c| Arc::new(Mutex::new(Box::new(c) as Box<dyn Scrobbler>)));

    handle_cli_args(&mut state);
    check_updater_status(&mut state);

    let (window, _timer) = build_window(state)?;
    window.run().map_err(|err| anyhow::anyhow!("Slint execution failed: {err}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thin_composition_reexports_resolve() {
        assert_eq!(SlintPage::Search.to_index(), 0);
        let labels = StaticLabels::resolve(crate::i18n::Locale::English);
        assert_eq!(labels.nav_search, "Search");
        assert_eq!(format_queue_label(crate::i18n::Locale::English, 2), "Queue (2)");
    }
}
