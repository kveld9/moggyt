//! moggyt: native YouTube Music client (eframe/egui).
//!
//! Composition root: builds logging, instantiates the catalog/audio
//! adapters, injects them into the UI shell, and bootstraps eframe.
//! Domain logic lives in [`moggyt::domain`]; all I/O hides behind
//! ports implemented in [`moggyt::infra`].

use std::path::{Path, PathBuf};

use anyhow::Context;
use moggyt::app::{AppState, MoggytApp};
use moggyt::infra::{
    CredentialStore, DesktopPresence, DiskCoverCache, DiskQueueStore, DiskSettingsStore,
    DiskSkipStore, KeyringCredentialStore, KeyringSessionStore, LastFmClient, LiveAudio,
    LiveCatalog, LrclibClient, RodioBackend, SessionStore, SettingsStore, SingleInstanceGuard,
    TrayHandle, client_from_credentials, default_config_dir, ensure_sidecar,
};

fn main() -> anyhow::Result<()> {
    let _instance = match SingleInstanceGuard::try_acquire() {
        Some(g) => g,
        None => {
            eprintln!("moggyt is already running; exiting.");
            return Ok(());
        }
    };

    tracing_subscriber_setup();

    let data_dir = app_data_dir();
    let audio = init_live_audio(&data_dir);
    let catalog = LiveCatalog::new();
    let session = KeyringSessionStore::moggyt();
    let lyrics = LrclibClient::new();
    let covers = init_cover_cache(&data_dir);
    let mut state = AppState::new(catalog, audio, session, lyrics, covers);
    if let Some(cookies) = state.session.load() {
        state.catalog.set_session_cookies(Some(&cookies));
    }
    match TrayHandle::try_new() {
        Some(h) => state.set_tray(h),
        None => tracing::warn!("tray unavailable; running without system tray icon"),
    }
    let presence = DesktopPresence::new();
    tracing::debug!("desktop media presence live: {}", presence.is_live());
    state.set_presence(presence);
    tracing::debug!("discord rich presence live: {}", state.discord.is_live());

    let settings_dir = default_config_dir();
    let settings_store = DiskSettingsStore::new(settings_dir.clone());
    let settings = settings_store.load();
    state.set_settings_store(Box::new(settings_store));
    let queue_store = DiskQueueStore::new(settings_dir.clone());
    state.set_queue_store(Box::new(queue_store));

    let skip_store = DiskSkipStore::new(settings_dir);
    state.set_skip_store(Box::new(skip_store));

    let scrobbler = init_lastfm_scrobbler(settings.lastfm_enabled);
    tracing::debug!("last.fm scrobbler live: {}", scrobbler.is_live());
    state.set_scrobbler(Box::new(scrobbler));
    state.set_credential_store(Box::new(KeyringCredentialStore::moggyt()));

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "moggyt",
        options,
        Box::new(|cc| {
            moggyt::app::theme::apply(&cc.egui_ctx, &state.theme.palette());
            Ok(Box::new(MoggytApp::new(state)))
        }),
    )
    .map_err(|err| anyhow::anyhow!("eframe failed: {err}"))?;

    Ok(())
}

/// Resolves the application data directory using standard platform conventions.
///
/// On Windows, targets `%APPDATA%/moggyt`.
/// On Unix and other platforms, checks `$XDG_DATA_HOME/moggyt`, falling back to
/// `$HOME/.local/share/moggyt` or `.moggyt` relative directory.
fn app_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("moggyt");
        }
        PathBuf::from(".").join(".moggyt")
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(xdg).join("moggyt");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/moggyt");
        }
        PathBuf::from(".").join(".moggyt")
    }
}

/// Creates directory tree if missing, ignoring AlreadyExists.
fn ensure_data_dir(data_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir).with_context(|| {
        format!("failed to create application data directory at '{}'", data_dir.display())
    })
}

/// Attempts to initialize the live rodio backend and yt-dlp sidecar.
fn init_rodio_backend(data_dir: &Path) -> anyhow::Result<RodioBackend> {
    ensure_data_dir(data_dir)?;
    let binary =
        ensure_sidecar(data_dir).context("failed to locate or download yt-dlp sidecar binary")?;
    RodioBackend::new(binary).context("failed to initialize rodio audio output stream")
}

/// Initializes audio backend with live rodio playback, falling back to muted stub on any failure.
fn init_live_audio(data_dir: &Path) -> LiveAudio {
    match init_rodio_backend(data_dir) {
        Ok(backend) => LiveAudio::Live(backend),
        Err(err) => {
            tracing::warn!("audio initialization failed; running with muted stub: {err:#}");
            LiveAudio::muted()
        }
    }
}

/// Initializes the disk cover cache in `data_dir/covers`, falling back to a temporary directory
/// if primary creation fails. If both attempts fail, logs error and exits with code 1.
fn init_cover_cache(data_dir: &Path) -> DiskCoverCache {
    let primary = data_dir.join("covers");
    if let Ok(cache) = DiskCoverCache::new(&primary) {
        return cache;
    }
    tracing::warn!("primary cover cache failed at '{}'; using temp fallback", primary.display());
    let fallback = std::env::temp_dir().join("moggyt-covers-fallback");
    DiskCoverCache::new(&fallback).unwrap_or_else(|err| {
        tracing::error!("fallback cover cache failed at '{}': {err:#}", fallback.display());
        eprintln!("Fatal: failed to initialize cover cache: {err}");
        std::process::exit(1);
    })
}

/// Initializes the Last.fm client based on persistent settings and stored keyring credentials.
///
/// Falls back safely to a dormant client on any keyring access error or missing credentials.
fn init_lastfm_scrobbler(enabled: bool) -> LastFmClient {
    if !enabled {
        tracing::debug!("Last.fm scrobbling disabled in settings; keeping dormant client");
        return LastFmClient::new("", "");
    }
    let store = KeyringCredentialStore::moggyt();
    let Some(creds) = store.load_lastfm() else {
        tracing::warn!(
            "Last.fm enabled in settings but credentials absent or unreadable; scrobbler dormant"
        );
        return LastFmClient::new("", "");
    };
    if !creds.is_complete() {
        tracing::warn!("Last.fm credentials incomplete; scrobbler dormant");
        return LastFmClient::new("", "");
    }
    tracing::info!("initializing live Last.fm scrobbler from stored credentials");
    client_from_credentials(&creds)
}

fn tracing_subscriber_setup() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt().with_env_filter(filter).try_init();
}
