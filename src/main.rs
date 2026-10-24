//! moggyt: native YouTube Music client (eframe/egui).
//!
//! Composition root: builds logging, instantiates the catalog/audio
//! adapters, injects them into the UI shell, and bootstraps eframe.
//! Domain logic lives in [`moggyt::domain`]; all I/O hides behind
//! ports implemented in [`moggyt::infra`].

use std::path::{Path, PathBuf};

use anyhow::Context;
use moggyt::app::{AppState, MoggytApp};
use moggyt::infra::{LiveAudio, RodioBackend, YtmCatalog, ensure_sidecar};

fn main() -> anyhow::Result<()> {
    tracing_subscriber_setup();

    let data_dir = app_data_dir();
    let audio = init_live_audio(&data_dir);
    let catalog = YtmCatalog::new();
    let state = AppState::new(catalog, audio);

    let options = eframe::NativeOptions::default();
    eframe::run_native("moggyt", options, Box::new(|_ctx| Ok(Box::new(MoggytApp::new(state)))))
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

fn tracing_subscriber_setup() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt().with_env_filter(filter).try_init();
}
