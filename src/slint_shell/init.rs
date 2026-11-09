//! Platform initialization, directories, backend adapters, and logging bootstrap.
//!
//! Hexagonal invariant: provides platform concrete adapters to the composition root.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;

use crate::infra::{
    CredentialStore, DiskCoverCache, DiskPlaylistStore, GithubUpdater, KeyringCredentialStore,
    LastFmAppKeys, LastFmClient, LastFmCredentials, LiveAudio, ReleaseInfo, RodioBackend,
    StreamDiskCache, Updater, client_from_credentials, default_config_dir, is_newer,
    provision_sidecar, spawn_sidecar_refresh,
};
use crate::slint_shell::import_cli::{ImportCli, parse_import_args, report_to_json, run_headless};
use crate::slint_shell::import_searcher::LiveTrackSearcher;
use crate::slint_shell::state::SlintAppState;

/// Resolves the application data directory using standard platform conventions.
pub fn app_data_dir() -> PathBuf {
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

/// Resolves the application cache directory using standard platform conventions.
pub fn app_cache_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("moggyt");
        }
        app_data_dir()
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
            return PathBuf::from(xdg).join("moggyt");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".cache/moggyt");
        }
        app_data_dir()
    }
}

/// Creates directory tree if missing, ignoring AlreadyExists.
pub fn ensure_data_dir(data_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir).with_context(|| {
        format!("failed to create application data directory at '{}'", data_dir.display())
    })
}

/// Attempts to initialize the live rodio backend and yt-dlp sidecar with an optional stream cache.
pub fn init_rodio_backend(
    data_dir: &Path,
    stream_cache: Option<Arc<StreamDiskCache>>,
) -> anyhow::Result<RodioBackend> {
    ensure_data_dir(data_dir)?;
    let binary = provision_sidecar(data_dir)
        .context("failed to locate or download yt-dlp sidecar binary")?;
    spawn_sidecar_refresh(binary.clone());
    RodioBackend::with_cache(binary, stream_cache)
        .context("failed to initialize rodio audio output stream")
}

/// Initializes audio backend with live rodio playback, falling back to muted stub on any failure.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub fn init_live_audio(data_dir: &Path, stream_cache: Option<Arc<StreamDiskCache>>) -> LiveAudio {
    match init_rodio_backend(data_dir, stream_cache) {
        Ok(backend) => LiveAudio::Live(backend),
        Err(err) => {
            tracing::warn!("audio initialization failed; running with muted stub: {err:#}");
            LiveAudio::muted()
        }
    }
}

/// Initializes the disk cover cache in `data_dir/covers`, falling back to a temporary directory.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub fn init_cover_cache(data_dir: &Path) -> DiskCoverCache {
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

fn client_from_stored(stored: Option<LastFmCredentials>) -> LastFmClient {
    match stored {
        Some(creds) if creds.is_complete() => client_from_credentials(&creds),
        _ => LastFmClient::new("", ""),
    }
}

pub(crate) fn select_lastfm_client(
    app: Option<LastFmAppKeys>,
    stored: Option<LastFmCredentials>,
) -> LastFmClient {
    if let Some(keys) = app {
        let sk = stored.as_ref().map(|c| c.session_key.trim()).unwrap_or("");
        return match sk.is_empty() {
            false => LastFmClient::from_app_keys(&keys, sk),
            true => LastFmClient::new("", ""),
        };
    }
    client_from_stored(stored)
}

/// Initializes the primary Last.fm client from settings and stored keyring credentials.
pub fn init_lastfm_scrobbler(enabled: bool) -> LastFmClient {
    if !enabled {
        return LastFmClient::new("", "");
    }
    let store = KeyringCredentialStore::moggyt();
    let stored = store.load_lastfm();
    let app = LastFmAppKeys::from_build_env();
    select_lastfm_client(app, stored)
}

/// Initializes the secondary Last.fm client from settings and stored keyring credentials.
pub fn init_secondary_lastfm_scrobbler(enabled: bool) -> Option<LastFmClient> {
    if !enabled {
        return None;
    }
    let store = KeyringCredentialStore::moggyt_secondary();
    let creds = store.load_lastfm()?;
    if !creds.is_complete() {
        return None;
    }
    Some(client_from_credentials(&creds))
}

/// Initializes tracing subscriber logging with default info filter.
pub fn tracing_subscriber_setup() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt().with_env_filter(filter).try_init();
}

fn run_cli_import(cli: &ImportCli) -> i32 {
    let searcher = Arc::new(LiveTrackSearcher::new());
    let store = DiskPlaylistStore::new(default_config_dir());
    let mut progress = |msg: &str| eprintln!("{msg}");
    match run_headless(cli, searcher, &store, &mut progress) {
        Ok(report) => {
            if cli.json {
                print!("{}", report_to_json(&report));
            } else {
                println!(
                    "Imported '{}' ({} / {} matched) into {}",
                    report.playlist_title, report.matched, report.total, report.playlist_id
                );
            }
            0
        }
        Err(err) => {
            eprintln!("{err:#}");
            1
        }
    }
}

/// Runs before any GUI/audio/tray initialization; returns the process exit code when an import was
/// requested.
pub fn run_import_cli_if_requested(args: &[String]) -> Option<i32> {
    match parse_import_args(args) {
        Ok(Some(cli)) => Some(run_cli_import(&cli)),
        Err(err) => {
            eprintln!("{err:#}");
            Some(2)
        }
        Ok(None) => None,
    }
}

// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
fn handle_legacy_cli_args(state: &mut SlintAppState, raw_args: &[String]) {
    let Some(first_arg) = raw_args.first() else { return };
    if first_arg.starts_with("moggyt:") || first_arg.starts_with("handoff:") {
        if state.import_handoff(first_arg) {
            tracing::info!("successfully imported session handoff from CLI argument");
        } else {
            tracing::warn!("failed to import session handoff from CLI argument");
        }
    } else if first_arg.contains("list=") {
        match state.import_playlist_url(first_arg) {
            Ok(count) => tracing::info!("enqueued {count} tracks from CLI playlist URL"),
            Err(err) => tracing::warn!("failed to import playlist from CLI URL: {err:#}"),
        }
    }
}

/// Parses CLI arguments and executes single-instance URL/handoff import commands.
pub(crate) fn handle_cli_args(state: &mut SlintAppState) {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    handle_legacy_cli_args(state, &raw_args);
}

/// Applies release info to application state without performing any I/O.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub(crate) fn apply_updater_release(state: &mut SlintAppState, info: Option<ReleaseInfo>) {
    let Some(info) = info else {
        return;
    };
    let current = GithubUpdater::current_version();
    if is_newer(current, &info.version) {
        tracing::info!(version = %info.version, url = %info.url, "update available");
        state.update_available = true;
        state.latest_version = Some(info.version);
    }
}

/// Spawns a background worker checking for application updates without blocking startup.
pub(crate) fn spawn_updater_check() -> std::sync::mpsc::Receiver<Option<ReleaseInfo>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().name("moggyt-updater".into()).spawn(move || {
        let updater = GithubUpdater::new();
        let _ = tx.send(updater.latest());
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_ensure_data_dir_creates_directory_when_missing() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-init-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);

        ensure_data_dir(&temp_dir).expect("ensure_data_dir should succeed");
        assert!(temp_dir.is_dir());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_app_data_dir_ends_with_expected_folder_name() {
        let dir = app_data_dir();
        assert!(dir.ends_with("moggyt") || dir.ends_with(".moggyt"));
    }

    #[test]
    fn test_init_secondary_lastfm_scrobbler_disabled_returns_none() {
        let scrobbler = init_secondary_lastfm_scrobbler(false);
        assert!(scrobbler.is_none());
    }

    #[test]
    fn test_apply_updater_release_newer_sets_status() {
        let mut state = test_wired();
        assert!(!state.update_available);
        assert_eq!(state.latest_version, None);

        let info = ReleaseInfo {
            version: "v99.0.0".to_string(),
            url: "https://github.com/example/releases/tag/v99.0.0".to_string(),
        };
        apply_updater_release(&mut state, Some(info));

        assert!(state.update_available);
        assert_eq!(state.latest_version.as_deref(), Some("v99.0.0"));
    }

    #[test]
    fn test_apply_updater_release_older_or_equal_leaves_state_alone() {
        let mut state = test_wired();
        let equal_info = ReleaseInfo {
            version: "0.1.0".to_string(),
            url: "https://github.com/example/releases/tag/v0.1.0".to_string(),
        };
        apply_updater_release(&mut state, Some(equal_info));
        assert!(!state.update_available);
        assert_eq!(state.latest_version, None);

        let older_info = ReleaseInfo {
            version: "v0.0.1".to_string(),
            url: "https://github.com/example/releases/tag/v0.0.1".to_string(),
        };
        apply_updater_release(&mut state, Some(older_info));
        assert!(!state.update_available);
        assert_eq!(state.latest_version, None);
    }

    #[test]
    fn test_apply_updater_release_none_changes_nothing() {
        let mut state = test_wired();
        apply_updater_release(&mut state, None);
        assert!(!state.update_available);
        assert_eq!(state.latest_version, None);
    }

    #[test]
    fn test_handle_legacy_cli_args_empty_or_unrecognized_noop() {
        let mut state = test_wired();
        handle_legacy_cli_args(&mut state, &[]);
        assert!(state.queue.is_empty());

        handle_legacy_cli_args(&mut state, &["--unknown-flag".to_string()]);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn test_run_import_cli_if_requested_none_for_empty_or_non_import_args() {
        assert_eq!(run_import_cli_if_requested(&[]), None);

        let unrelated = vec!["--unknown-flag".to_string(), "arg".to_string()];
        assert_eq!(run_import_cli_if_requested(&unrelated), None);
    }

    #[test]
    fn test_run_import_cli_if_requested_error_returns_exit_code_two() {
        let json_alone = vec!["--json".to_string()];
        assert_eq!(run_import_cli_if_requested(&json_alone), Some(2));

        let conflicting = vec![
            "--import-spotify".to_string(),
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".to_string(),
            "--import-csv".to_string(),
            "tracks.csv".to_string(),
        ];
        assert_eq!(run_import_cli_if_requested(&conflicting), Some(2));
    }

    #[test]
    fn test_select_lastfm_client_outcomes() {
        let app = LastFmAppKeys { api_key: "app_key".to_string(), secret: "app_sec".to_string() };
        let complete = LastFmCredentials::new("stored_k", "stored_s", "stored_sk");
        let incomplete = LastFmCredentials::new("stored_k", "", "stored_sk");
        let empty_sk = LastFmCredentials::new("stored_k", "stored_s", "   ");

        // With build-time app keys:
        assert!(select_lastfm_client(Some(app.clone()), Some(complete.clone())).is_live());
        assert!(select_lastfm_client(Some(app.clone()), Some(incomplete.clone())).is_live());
        assert!(!select_lastfm_client(Some(app.clone()), Some(empty_sk)).is_live());
        assert!(!select_lastfm_client(Some(app), None).is_live());

        // Without build-time app keys:
        assert!(select_lastfm_client(None, Some(complete)).is_live());
        assert!(!select_lastfm_client(None, Some(incomplete)).is_live());
        assert!(!select_lastfm_client(None, None).is_live());
    }
}
