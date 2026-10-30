//! Slint UI application wiring adapter.
//!
//! This module only wires injected ports.
//! Hexagonal architecture invariant: this module connects injected port adapters
//! to [`crate::slint_shell::MoggytWindow`] without containing direct I/O or private domain rules.

use std::sync::{Arc, Mutex};

use slint::ComponentHandle;

use crate::infra::{
    AudioBackend, Catalog, CoverCache, LyricOverrideDir, LyricsSource, QueueStore, Scrobbler,
    SessionStore, Settings, SettingsStore, SingleInstanceGuard, SkipStore, TrayHandle,
};

pub use crate::slint_shell::covers::{
    request_detail_covers, request_explore_covers, request_library_covers,
    request_queue_track_cover,
};
pub use crate::slint_shell::explore::execute_explore;
pub use crate::slint_shell::format::{
    PLAYBACK_SPEED_PRESETS, current_unix_secs, cycle_playback_speed_preset, format_playback_speed,
    format_queue_label, format_update_status,
};
pub use crate::slint_shell::labels::{StaticLabels, sync_static_labels};
pub use crate::slint_shell::page::SlintPage;
pub use crate::slint_shell::playback_control::apply_play_track_completion;
pub use crate::slint_shell::player_bar::PlayerBarPrefs;
pub use crate::slint_shell::state::{SharedState, SlintAppState};
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
    pub scrobbler: Box<dyn Scrobbler>,
    pub secondary_scrobbler: Option<Box<dyn Scrobbler>>,
    pub lyric_overrides: Option<LyricOverrideDir>,
    pub tray: Option<TrayHandle>,
}

/// Composition function launching the Slint desktop UI wired to injected platform adapters.
///
/// TODO Gap: Settings modal: DiskSettingsStore persists volume, speed, quality, DSP
/// toggles, and Last.fm credentials; playback speed and volume are fully wired, but
/// ui/shell.slint lacks an interactive settings dialog for DSP and credential management.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub fn run<C, A, S, L, K>(deps: AppDependencies<C, A, S, L, K>) -> anyhow::Result<()>
where
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
{
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
        scrobbler,
        secondary_scrobbler,
        lyric_overrides,
        tray,
    } = deps;

    let mut state = SlintAppState::new(catalog, audio, session, lyrics, covers);
    state.lyric_overrides = lyric_overrides;
    state.tray = tray;

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

    state.skip_store = skip_store;

    state.scrobbler = Arc::new(Mutex::new(scrobbler));
    state.secondary_scrobbler = secondary_scrobbler.map(|s| Arc::new(Mutex::new(s)));

    handle_cli_args(&mut state);
    state.updater_rx = Some(spawn_updater_check());

    let (window, _timer) = build_window(state)?;
    window.run().map_err(|err| anyhow::anyhow!("Slint execution failed: {err}"))?;

    let _ = with_ui_state::<C, A, S, L, K, _>(|st| {
        let _ = persist_state_on_exit(st);
    });

    Ok(())
}

/// Persists the active playback queue to storage on application exit.
pub fn persist_state_on_exit<C, A, S, L, K>(
    state: &SlintAppState<C, A, S, L, K>,
) -> anyhow::Result<()>
where
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
{
    state.queue_store.save(&state.queue)
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

        persist_state_on_exit(&state).expect("failed to persist queue on exit");

        let loaded = state.queue_store.load();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.tracks(), &[track1, track2]);
    }
}
