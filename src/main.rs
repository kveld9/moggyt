//! moggyt: native YouTube Music desktop client (Slint).
//!
//! Composition root: constructs every live adapter, resolves runtime directories,
//! loads stored settings, and injects dependencies into the Slint application shell.
//! Domain logic lives in [`moggyt::domain`]; all I/O hides behind ports implemented
//! in [`moggyt::infra`].

use moggyt::infra::{
    DiskMetaStore, DiskQueueStore, DiskSettingsStore, DiskSkipStore, KeyringSessionStore,
    LastFmClient, LiveCatalog, LrclibClient, LyricOverrideDir, Scrobbler, SettingsStore,
    SingleInstanceGuard, TrayHandle, default_config_dir,
};
use moggyt::slint_shell::app::AppDependencies;
use moggyt::slint_shell::init::{
    app_data_dir, init_cover_cache, init_lastfm_scrobbler, init_live_audio,
    init_secondary_lastfm_scrobbler, tracing_subscriber_setup,
};

fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    let guard = SingleInstanceGuard::try_acquire();
    if guard.is_none() {
        eprintln!("moggyt is already running; exiting.");
    }
    guard
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn init_tray() -> Option<TrayHandle> {
    let tray = TrayHandle::try_new();
    if tray.is_none() {
        tracing::warn!("tray unavailable; running without system tray icon");
    }
    tray
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn main() -> anyhow::Result<()> {
    let Some(instance) = acquire_single_instance() else {
        return Ok(());
    };

    tracing_subscriber_setup();

    let data_dir = app_data_dir();
    let config_dir = default_config_dir();

    let settings_store = DiskSettingsStore::new(config_dir.clone());
    let settings = settings_store.load();

    let audio = init_live_audio(&data_dir);
    let catalog = LiveCatalog::new();
    let session = KeyringSessionStore::moggyt();
    let lyrics = LrclibClient::new();
    let covers = init_cover_cache(&data_dir);

    let lyric_overrides = Some(LyricOverrideDir::new(config_dir.join("lyrics")));
    let tray = init_tray();

    let queue_store = DiskQueueStore::new(config_dir.clone());
    let skip_store = DiskSkipStore::new(config_dir.clone());
    let meta_store = DiskMetaStore::new(config_dir);

    let scrobbler = init_lastfm_scrobbler(settings.lastfm_enabled);
    tracing::debug!("last.fm scrobbler live: {}", scrobbler.is_live());

    let secondary_scrobbler = init_secondary_lastfm_scrobbler(settings.secondary_lastfm_enabled);
    let sec_live = secondary_scrobbler.as_ref().is_some_and(LastFmClient::is_live);
    tracing::debug!("secondary last.fm scrobbler live: {sec_live}");

    moggyt::slint_shell::app::run(AppDependencies {
        instance,
        catalog,
        audio,
        session,
        lyrics,
        covers,
        settings,
        settings_store: Box::new(settings_store),
        queue_store: Box::new(queue_store),
        skip_store: Box::new(skip_store),
        meta_store: Box::new(meta_store),
        scrobbler: Box::new(scrobbler),
        secondary_scrobbler: secondary_scrobbler.map(|c| Box::new(c) as Box<dyn Scrobbler>),
        lyric_overrides,
        tray,
    })
}
