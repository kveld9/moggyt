//! Platform initialization, directories, backend adapters, and logging bootstrap.
//!
//! Hexagonal invariant: provides platform concrete adapters to the composition root.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::infra::{
    AudioBackend, Catalog, CoverCache, CredentialStore, DiskCoverCache, GithubUpdater,
    KeyringCredentialStore, LastFmClient, LiveAudio, LyricsSource, ReleaseInfo, RodioBackend,
    SessionStore, Updater, client_from_credentials, ensure_sidecar, is_newer,
};
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

/// Creates directory tree if missing, ignoring AlreadyExists.
pub fn ensure_data_dir(data_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir).with_context(|| {
        format!("failed to create application data directory at '{}'", data_dir.display())
    })
}

/// Attempts to initialize the live rodio backend and yt-dlp sidecar.
pub fn init_rodio_backend(data_dir: &Path) -> anyhow::Result<RodioBackend> {
    ensure_data_dir(data_dir)?;
    let binary =
        ensure_sidecar(data_dir).context("failed to locate or download yt-dlp sidecar binary")?;
    RodioBackend::new(binary).context("failed to initialize rodio audio output stream")
}

/// Initializes audio backend with live rodio playback, falling back to muted stub on any failure.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub fn init_live_audio(data_dir: &Path) -> LiveAudio {
    match init_rodio_backend(data_dir) {
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

/// Initializes the primary Last.fm client from settings and stored keyring credentials.
pub fn init_lastfm_scrobbler(enabled: bool) -> LastFmClient {
    if !enabled {
        return LastFmClient::new("", "");
    }
    let store = KeyringCredentialStore::moggyt();
    let Some(creds) = store.load_lastfm() else {
        return LastFmClient::new("", "");
    };
    if !creds.is_complete() {
        return LastFmClient::new("", "");
    }
    client_from_credentials(&creds)
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

/// Parses CLI arguments and executes single-instance URL/handoff import commands.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub(crate) fn handle_cli_args<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: &mut SlintAppState<C, A, S, L, K>,
) {
    let mut args = std::env::args().skip(1);
    let Some(first_arg) = args.next() else { return };
    if first_arg.starts_with("moggyt:") || first_arg.starts_with("handoff:") {
        if state.import_handoff(&first_arg) {
            tracing::info!("successfully imported session handoff from CLI argument");
        } else {
            tracing::warn!("failed to import session handoff from CLI argument");
        }
    } else if first_arg.contains("list=") {
        match state.import_playlist_url(&first_arg) {
            Ok(count) => tracing::info!("enqueued {count} tracks from CLI playlist URL"),
            Err(err) => tracing::warn!("failed to import playlist from CLI URL: {err:#}"),
        }
    }
}

/// Applies release info to application state without performing any I/O.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub(crate) fn apply_updater_release<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: &mut SlintAppState<C, A, S, L, K>,
    info: Option<ReleaseInfo>,
) {
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
}
