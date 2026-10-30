//! Platform initialization, directories, backend adapters, and logging bootstrap.
//!
//! Hexagonal invariant: provides platform concrete adapters to the composition root.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::infra::{
    AudioBackend, Catalog, CoverCache, CredentialStore, DiskCoverCache, GithubUpdater,
    KeyringCredentialStore, LastFmClient, LiveAudio, LyricsSource, RodioBackend, SessionStore,
    Updater, client_from_credentials, ensure_sidecar, is_newer,
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

/// Checks GitHub Releases for newer version tags and records update availability.
// NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
#[allow(clippy::cognitive_complexity)]
pub(crate) fn check_updater_status<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: &mut SlintAppState<C, A, S, L, K>,
) {
    if let Some(info) = state.updater.latest() {
        let current = GithubUpdater::current_version();
        if is_newer(current, &info.version) {
            tracing::info!(version = %info.version, url = %info.url, "update available");
            state.update_available = true;
            state.latest_version = Some(info.version);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Album, Detail, DetailKind, Playlist, SearchQuery, SearchResult, Shelf, Track,
    };
    use crate::i18n::Locale;
    use crate::infra::{AudioStub, LrclibStub, MemoryCoverCache, MemorySessionStore, Settings};
    use crate::slint_shell::test_support::LocaleTrackingCatalog;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_startup_settings_refetches_home_shelves_with_settings_locale() {
        let (catalog, locales, fetch_locales) = LocaleTrackingCatalog::new();
        let audio = AudioStub::new();
        let session = MemorySessionStore::new();
        let lyrics = LrclibStub::new();
        let covers = MemoryCoverCache::new();

        let mut state = SlintAppState::new(catalog, audio, session, lyrics, covers);
        assert_eq!(state.home_shelves[0].title, "Home English");

        let settings = Settings { locale: Locale::Spanish, ..Settings::default() };

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

        assert_eq!(*locales.lock().unwrap(), vec![Locale::English, Locale::Spanish]);
        assert_eq!(*fetch_locales.lock().unwrap(), vec![Locale::English, Locale::Spanish]);
        assert_eq!(state.home_shelves.len(), 1);
        assert_eq!(state.home_shelves[0].title, "Home Spanish");
    }

    #[test]
    fn test_startup_settings_refetch_preserves_old_shelves_on_empty_or_failure() {
        #[derive(Debug, Clone)]
        struct FailingCatalog {
            call_count: Arc<Mutex<usize>>,
        }

        impl Catalog for FailingCatalog {
            fn search(&self, _query: &str) -> Vec<Track> {
                Vec::new()
            }
            fn search_filtered(&self, _query: &SearchQuery) -> Vec<SearchResult> {
                Vec::new()
            }
            fn radio(&self, _track_id: &str) -> Vec<Track> {
                Vec::new()
            }
            fn home_shelves(&self) -> Vec<Shelf> {
                let mut count = self.call_count.lock().unwrap();
                *count += 1;
                if *count == 1 { vec![Shelf::new("Fallback Placeholder")] } else { Vec::new() }
            }
            fn playlists(&self) -> Vec<Playlist> {
                Vec::new()
            }
            fn liked_tracks(&self) -> Vec<Track> {
                Vec::new()
            }
            fn saved_albums(&self) -> Vec<Album> {
                Vec::new()
            }
            fn set_liked(&mut self, _track_id: &str, _liked: bool) {}
            fn detail(&self, _browse_id: &str, _kind: DetailKind) -> Option<Detail> {
                None
            }
        }

        let count = Arc::new(Mutex::new(0));
        let catalog = FailingCatalog { call_count: Arc::clone(&count) };
        let audio = AudioStub::new();
        let session = MemorySessionStore::new();
        let lyrics = LrclibStub::new();
        let covers = MemoryCoverCache::new();

        let mut state = SlintAppState::new(catalog, audio, session, lyrics, covers);
        assert_eq!(state.home_shelves[0].title, "Fallback Placeholder");

        let settings = Settings { locale: Locale::Spanish, ..Settings::default() };
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

        assert_eq!(state.home_shelves.len(), 1);
        assert_eq!(state.home_shelves[0].title, "Fallback Placeholder");
    }
}
