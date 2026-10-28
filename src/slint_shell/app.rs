//! Slint UI application wiring and composition root adapter.
//!
//! Hexagonal architecture invariant: this module connects injected port adapters
//! to [`MoggytWindow`] without containing direct I/O or private domain rules.

use std::any::Any;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Context;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::{
    Album, Artist, Detail, DetailKind, PlaybackQueue, Playlist, SearchQuery, SearchResult, Shelf,
    SleepTimer, ThemeId, Track, extract_playlist_id,
};
use crate::infra::{
    AudioBackend, Catalog, CoverCache, CredentialStore, DesktopPresence, DiscordPresence,
    DiskCoverCache, DiskMetaStore, DiskQueueStore, DiskSettingsStore, DiskSkipStore, GithubUpdater,
    KeyringCredentialStore, KeyringSessionStore, LastFmClient, LiveAudio, LiveCatalog,
    LrclibClient, LyricLine, LyricOverrideDir, LyricsSource, MediaKeyAction, MemoryMetaStore,
    MemoryQueueStore, MemorySettingsStore, MemorySkipStore, MetaStore, Presence, QueueStore,
    RodioBackend, Scrobbler, SessionStore, SettingsStore, SingleInstanceGuard, SkipStore,
    StubScrobbler, TrayAction, TrayHandle, Updater, client_from_credentials, decode_cover,
    decode_handoff, default_config_dir, ensure_sidecar, fetch_artwork, sanitize_cookie_input,
    should_scrobble,
};
use crate::slint_shell::detail::detail_kind_label;
use crate::slint_shell::platform::{dispatch_to_ui, rgba_to_image};
use crate::slint_shell::playback::{format_position, format_track_duration};
use crate::slint_shell::playback_worker::{
    PlayTrackRequest, PlayTrackResult, PlaybackWorkerAdapters, PlaybackWorkerCommand,
    execute_detail, execute_play_track, execute_refresh_library, execute_scrobble, execute_search,
};
use crate::slint_shell::search::format_duration;
use crate::slint_shell::theme::apply_theme;
use crate::slint_shell::{
    LibraryItemData, LyricLineData, MoggytWindow, ShelfData, SlintPage, TrackRowData,
};

/// Returns current UTC timestamp in seconds since UNIX epoch, or 0 on clock failure.
pub fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

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

/// In-memory state and collaborator wiring for the Slint UI shell.
///
/// # Thread-safety and Ownership Model
///
/// Slint requires all UI state and windows to remain on the main thread inside
/// `Rc<RefCell<SlintAppState>>`. To guarantee that blocking I/O (yt-dlp resolution,
/// stream downloading, Last.fm HTTP scrobbling, and remote lyrics/covers) never
/// freezes the Slint event loop, collaborators that perform I/O are held in thread-safe
/// reference-counted containers:
///
/// - `audio`: `Arc<Mutex<A>>`. `LiveAudio` (`RodioBackend`) is `Send`. Mutex wrapping
///   allows background playback resolution while the UI thread uses non-blocking `try_lock()`.
/// - `catalog`: `Arc<Mutex<C>>`. `LiveCatalog` is `Send + Sync`. Background searches and
///   detail requests execute without blocking UI interactions.
/// - `lyrics`: `Arc<L>`. `LrclibClient` is `Send + Sync`. Remote lookup runs on worker.
/// - `covers`: `Arc<Mutex<K>>`. Remote fetch and CPU decode run on worker; decoded RGBA bytes
///   cross as owned `CoverArtData` (`Vec<u8>`). Slint `Image` is constructed on UI thread.
/// - `scrobbler`: `Arc<Mutex<Box<dyn Scrobbler>>>`. `Scrobbler: Send`. HTTP posts run on worker.
pub struct SlintAppState<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
> {
    pub catalog: Arc<Mutex<C>>,
    pub audio: Arc<Mutex<A>>,
    pub session: S,
    pub lyrics: Arc<L>,
    pub covers: Arc<Mutex<K>>,
    pub lyric_overrides: Option<LyricOverrideDir>,
    pub tray: Option<TrayHandle>,
    pub presence: DesktopPresence,
    pub discord: DiscordPresence,
    pub scrobbler: Arc<Mutex<Box<dyn Scrobbler>>>,
    pub secondary_scrobbler: Option<Arc<Mutex<Box<dyn Scrobbler>>>>,
    pub settings_store: Box<dyn SettingsStore>,
    pub queue_store: Box<dyn QueueStore>,
    pub skip_store: Box<dyn SkipStore>,
    pub meta_store: Box<dyn MetaStore>,
    pub updater: GithubUpdater,

    pub page: SlintPage,
    pub queue: PlaybackQueue,
    pub home_shelves: Vec<Shelf>,
    pub search_query: String,
    pub search_results: Vec<Track>,
    pub library_playlists: Vec<Playlist>,
    pub library_albums: Vec<Album>,
    pub library_artists: Vec<Artist>,
    pub detail: Option<Detail>,
    pub detail_return: SlintPage,
    pub lyrics_lines: Vec<LyricLine>,
    pub lyrics_key: Option<(String, String)>,
    pub cover_key: Option<String>,
    pub current_cover_image: Option<slint::Image>,
    pub last_presence_key: Option<(String, String)>,
    pub started_at: Option<(Instant, u64)>,
    pub last_skip_target: Option<(String, u32)>,
    pub sleep_timer: SleepTimer,
    pub sleep_timer_accumulated: u64,
    pub sleep_timer_base_offset: u64,
    pub autoplay: bool,
    pub hi_res_covers: bool,
    pub signed_in: bool,
    pub theme: ThemeId,

    pub play_generation: u64,
    pub search_generation: u64,
    pub detail_generation: u64,
    pub library_generation: u64,
}

/// Shared reference-counted wrapper for [`SlintAppState`].
pub type SharedState<C, A, S, L, K> = Rc<RefCell<SlintAppState<C, A, S, L, K>>>;

thread_local! {
    static ACTIVE_STATE: RefCell<Option<Box<dyn Any>>> = RefCell::new(None);
}

fn set_ui_state<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state_rc: &SharedState<C, A, S, L, K>,
) {
    ACTIVE_STATE.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(Rc::clone(state_rc)));
    });
}

fn with_ui_state<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
    R,
>(
    f: impl FnOnce(&mut SlintAppState<C, A, S, L, K>) -> R,
) -> Option<R> {
    ACTIVE_STATE.with(|cell| {
        let borrow = cell.borrow();
        let any = borrow.as_ref()?;
        let rc = any.downcast_ref::<SharedState<C, A, S, L, K>>()?;
        let mut st = rc.borrow_mut();
        Some(f(&mut st))
    })
}

impl<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
> SlintAppState<C, A, S, L, K>
{
    /// Creates the Slint application state with injected collaborators.
    pub fn new(mut catalog: C, audio: A, session: S, lyrics: L, covers: K) -> Self {
        let stored_session = session.load();
        let signed_in = stored_session.is_some();
        if let Some(ref cookies) = stored_session {
            catalog.set_session_cookies(Some(cookies));
        }
        let shelves = catalog.home_shelves();
        Self {
            catalog: Arc::new(Mutex::new(catalog)),
            audio: Arc::new(Mutex::new(audio)),
            session,
            lyrics: Arc::new(lyrics),
            covers: Arc::new(Mutex::new(covers)),
            lyric_overrides: None,
            tray: None,
            presence: DesktopPresence::new(),
            discord: DiscordPresence::new(),
            scrobbler: Arc::new(Mutex::new(Box::new(StubScrobbler::new()))),
            secondary_scrobbler: None,
            settings_store: Box::new(MemorySettingsStore::default()),
            queue_store: Box::new(MemoryQueueStore::default()),
            skip_store: Box::new(MemorySkipStore::new()),
            meta_store: Box::new(MemoryMetaStore::new()),
            updater: GithubUpdater::new(),
            page: SlintPage::Search,
            queue: PlaybackQueue::new(),
            home_shelves: shelves,
            search_query: String::new(),
            search_results: Vec::new(),
            library_playlists: Vec::new(),
            library_albums: Vec::new(),
            library_artists: Vec::new(),
            detail: None,
            detail_return: SlintPage::Search,
            lyrics_lines: Vec::new(),
            lyrics_key: None,
            cover_key: None,
            current_cover_image: None,
            last_presence_key: None,
            started_at: None,
            last_skip_target: None,
            sleep_timer: SleepTimer::new(),
            sleep_timer_accumulated: 0,
            sleep_timer_base_offset: 0,
            autoplay: false,
            hi_res_covers: false,
            signed_in,
            theme: ThemeId::default(),
            play_generation: 0,
            search_generation: 0,
            detail_generation: 0,
            library_generation: 0,
        }
    }

    /// Selects active navigation page and refreshes content when entering Library view.
    pub fn select_page(&mut self, page: SlintPage) {
        self.page = page;
        if page == SlintPage::Library {
            self.refresh_library();
        }
    }

    /// Refreshes playlists, saved albums, and followed artists from catalog.
    pub fn refresh_library(&mut self) {
        let (playlists, albums) = match self.catalog.lock() {
            Ok(c) => (c.playlists(), c.saved_albums()),
            Err(_) => (Vec::new(), Vec::new()),
        };
        self.library_playlists = playlists;
        self.library_albums = albums;
        self.library_artists = Vec::new();
    }

    /// Advances or retreats queue cursor in-memory to target index without triggering playback.
    pub fn play_track_at_index_in_memory(&mut self, index: usize) {
        if index >= self.queue.len() {
            return;
        }
        while self.queue.cursor() < index {
            self.queue.advance();
        }
        while self.queue.cursor() > index {
            self.queue.retreat();
        }
    }

    /// Advances or retreats queue cursor to target index and starts playback quietly.
    pub fn play_track_at_index(&mut self, index: usize) {
        self.play_track_at_index_in_memory(index);
        self.play_current_quiet();
    }

    /// Plays the track currently under the cursor without changing navigation page.
    pub fn play_current_quiet(&mut self) {
        let Some(track) = self.queue.current() else {
            return;
        };
        let id = track.id.clone();
        let artist = track.artist.clone();
        let title = track.title.clone();
        if let Ok(mut a) = self.audio.lock() {
            a.play(&id);
        }
        self.started_at = Some((Instant::now(), current_unix_secs()));
        if let Ok(mut s) = self.scrobbler.lock() {
            s.now_playing(&artist, &title);
        }
        if let Some(sec) = &self.secondary_scrobbler {
            if let Ok(mut s) = sec.lock() {
                s.now_playing(&artist, &title);
            }
        }
        self.last_skip_target = None;
        self.sync_presence_track(&artist, &title, true);
        self.refresh_lyrics();
    }

    /// Toggles playback: pauses if playing, resumes if requested, or starts head.
    pub fn toggle_play(&mut self) {
        let is_playing = self.audio.lock().map(|a| a.is_playing()).unwrap_or(false);
        let has_last = self.audio.lock().map(|a| a.last_requested().is_some()).unwrap_or(false);
        if is_playing {
            if let Ok(mut a) = self.audio.lock() {
                a.pause();
            }
            self.sync_presence_playing(false);
        } else if has_last {
            if let Ok(mut a) = self.audio.lock() {
                a.resume_play();
            }
            self.sync_presence_playing(true);
        } else {
            self.play_current_quiet();
        }
    }

    /// Advances cursor to next track, scrobbling the completed track if eligible.
    pub fn next_track(&mut self) {
        self.maybe_scrobble_current();
        self.queue.advance();
        self.play_current_quiet();
    }

    /// Retreats cursor to previous track, scrobbling the completed track if eligible.
    pub fn prev_track(&mut self) {
        self.maybe_scrobble_current();
        self.queue.retreat();
        self.play_current_quiet();
    }

    /// Adjusts position relative to current playback point, clamped at zero.
    pub fn seek_by(&mut self, delta_secs: i64) {
        if let Ok(mut a) = self.audio.lock() {
            let current = a.position_secs() as i64;
            let target = current.saturating_add(delta_secs).max(0) as u64;
            a.seek(target);
        }
    }

    /// Updates playback volume level and persists preference to disk.
    pub fn set_volume(&mut self, volume: u8) {
        if let Ok(mut a) = self.audio.lock() {
            a.set_volume(volume);
        }
        let mut settings = self.settings_store.load();
        settings.volume = volume;
        let _ = self.settings_store.save(&settings);
    }

    /// Updates playback speed factor and persists preference to disk.
    pub fn set_playback_speed(&mut self, speed: f32) {
        let actual = if let Ok(mut a) = self.audio.lock() {
            a.set_speed(speed);
            a.speed()
        } else {
            1.0
        };
        let mut settings = self.settings_store.load();
        settings.playback_speed = actual;
        let _ = self.settings_store.save(&settings);
    }

    /// Submits search query to catalog and wraps hits in a shelf.
    pub fn submit_search(&mut self, query: &str) {
        self.search_query = query.to_string();
        let Some(parsed) = SearchQuery::parse(query) else {
            return;
        };
        let hits = self.catalog.lock().map(|c| c.search(parsed.text())).unwrap_or_default();
        self.search_results = hits.clone();
        let items: Vec<SearchResult> = hits.into_iter().map(SearchResult::Track).collect();
        let mut shelf = Shelf::new("Search Results");
        shelf.items = items;
        self.home_shelves = vec![shelf];
    }

    /// Opens detail container for album, artist, or playlist identifier.
    pub fn open_detail(&mut self, browse_id: &str, kind: DetailKind) {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.page != SlintPage::Detail {
            self.detail_return = self.page;
        }
        self.detail = self.catalog.lock().map(|c| c.detail(trimmed, kind)).unwrap_or(None);
        self.page = SlintPage::Detail;
    }

    /// Enqueues all tracks from a playlist URL into the playback queue.
    pub fn import_playlist_url(&mut self, url: &str) -> anyhow::Result<usize> {
        let playlist_id = extract_playlist_id(url)
            .ok_or_else(|| anyhow::anyhow!("invalid playlist URL: {url}"))?;
        let detail = self
            .catalog
            .lock()
            .map_err(|_| anyhow::anyhow!("catalog lock poisoned"))?
            .detail(&playlist_id, DetailKind::Playlist)
            .ok_or_else(|| anyhow::anyhow!("playlist detail not found: {playlist_id}"))?;
        let count = detail.tracks.len();
        for track in detail.tracks {
            self.queue.push(track);
        }
        Ok(count)
    }

    /// Refreshes lyrics for the active track from local overrides or LRCLIB.
    pub fn refresh_lyrics(&mut self) {
        let Some(track) = self.queue.current() else {
            self.lyrics_lines.clear();
            self.lyrics_key = None;
            return;
        };
        let key = (track.artist.clone(), track.title.clone());
        if self.lyrics_key.as_ref() == Some(&key) {
            return;
        }
        if let Some(ref overrides) = self.lyric_overrides {
            if let Some(hit) = overrides.load(&track.id) {
                self.lyrics_lines = hit.lines;
                self.lyrics_key = Some(key);
                return;
            }
        }
        let duration = track.duration_secs.map(u64::from);
        self.lyrics_lines =
            self.lyrics.lookup(&key.0, &key.1, duration).map(|s| s.lines).unwrap_or_default();
        self.lyrics_key = Some(key);
    }

    /// Resolves 0-based active lyric line index for current playback timestamp.
    pub fn active_lyric_index(&self) -> i32 {
        let pos_secs = self.audio.try_lock().map(|a| a.position_secs()).unwrap_or(0);
        self.active_lyric_index_at(pos_secs as u32)
    }

    /// Resolves active lyric line index for an explicitly passed timestamp in seconds.
    pub fn active_lyric_index_at(&self, pos_secs: u32) -> i32 {
        let pos_ms = (pos_secs as u64) * 1000;
        if self.lyrics_lines.is_empty() {
            return -1;
        }
        let idx = self.lyrics_lines.partition_point(|l| l.start_ms <= pos_ms);
        if idx == 0 { -1 } else { (idx - 1) as i32 }
    }

    /// Fetches, caches, decodes, and converts active track cover art to Slint Image.
    pub fn resolve_cover_image(&mut self) -> Option<slint::Image> {
        let url = self.queue.current()?.artwork.clone()?;
        let bytes = if let Some(cached) = self.covers.lock().ok()?.get(&url) {
            cached
        } else {
            let fetched = fetch_artwork(&url, 8).ok()?;
            if let Ok(mut c) = self.covers.lock() {
                c.put(&url, fetched.clone());
            }
            fetched
        };
        let max_px = if self.hi_res_covers { 1024 } else { 512 };
        let rgba = decode_cover(&bytes, max_px)?;
        rgba_to_image(rgba.as_raw(), rgba.width(), rgba.height())
    }

    /// Drains pending system tray actions, returning true when quit was requested.
    pub fn drain_tray_actions(&mut self) -> bool {
        let Some(tray) = &self.tray else {
            return false;
        };
        let mut quit = false;
        for action in tray.drain() {
            match action {
                TrayAction::TogglePlay => self.toggle_play(),
                TrayAction::Next => self.next_track(),
                TrayAction::Previous => self.prev_track(),
                TrayAction::Show => {}
                TrayAction::Quit => quit = true,
            }
        }
        quit
    }

    /// Drains inbound MPRIS / SMTC media key actions from desktop presence.
    pub fn drain_media_keys(&mut self) {
        for action in self.presence.drain_actions() {
            match action {
                MediaKeyAction::Toggle => self.toggle_play(),
                MediaKeyAction::Next => self.next_track(),
                MediaKeyAction::Previous => self.prev_track(),
                MediaKeyAction::Stop => {
                    if let Ok(mut a) = self.audio.try_lock() {
                        if a.is_playing() {
                            a.pause();
                        }
                    }
                }
            }
        }
    }

    /// Evaluates auto-skip intervals for active track, seeking past matched range.
    pub fn check_auto_skip(&mut self) {
        let Some(track) = self.queue.current() else {
            self.last_skip_target = None;
            return;
        };
        let Ok(mut audio) = self.audio.try_lock() else {
            return;
        };
        let current_pos = u32::try_from(audio.position_secs()).unwrap_or(u32::MAX);
        if self
            .last_skip_target
            .as_ref()
            .is_some_and(|(id, t)| id != &track.id || current_pos >= *t)
        {
            self.last_skip_target = None;
        }
        let skips_map = self.skip_store.load();
        let Some(ranges) = skips_map.get(&track.id) else {
            return;
        };
        let Some(target) = ranges.jump_target(current_pos) else {
            return;
        };
        let already_skipped =
            self.last_skip_target.as_ref().is_some_and(|(id, t)| id == &track.id && *t == target);
        if target > current_pos && !already_skipped {
            self.last_skip_target = Some((track.id.clone(), target));
            audio.seek(target as u64);
        }
    }

    /// Advances queue or appends radio recommendations when active track ends.
    pub fn continue_autoplay_if_ended(&mut self) {
        let Ok(audio) = self.audio.try_lock() else {
            return;
        };
        if !self.autoplay || self.queue.is_empty() || audio.is_playing() {
            return;
        }
        let (Some(current), Some(last)) = (self.queue.current(), self.queue.tracks().last()) else {
            return;
        };
        if !std::ptr::eq(current, last) {
            return;
        }
        let Some(dur) = current.duration_secs else {
            return;
        };
        if audio.position_secs() < dur as u64 {
            return;
        }
        let current_id = current.id.clone();
        drop(audio);
        let recs = self.catalog.lock().map(|c| c.radio(&current_id)).unwrap_or_default();
        for track in recs {
            if !self.queue.tracks().iter().any(|t| t.id == track.id) {
                self.queue.push(track);
            }
        }
        self.queue.advance();
        self.play_current_quiet();
    }

    /// Submits scrobble record to active scrobbler if playback elapsed threshold was met.
    pub fn maybe_scrobble_current(&mut self) {
        let Some((started_instant, started_unix)) = self.started_at else {
            return;
        };
        let elapsed = started_instant.elapsed().as_secs();
        let Some(current) = self.queue.current() else {
            return;
        };
        let duration = current.duration_secs.map(u64::from);
        if should_scrobble(elapsed, duration) {
            if let Ok(mut s) = self.scrobbler.lock() {
                s.scrobble(&current.artist, &current.title, started_unix);
            }
            if let Some(sec) = &self.secondary_scrobbler {
                if let Ok(mut s) = sec.lock() {
                    s.scrobble(&current.artist, &current.title, started_unix);
                }
            }
        }
        self.started_at = None;
    }

    /// Synchronizes active track metadata with desktop presence and Discord presence.
    pub fn sync_presence_track(&mut self, artist: &str, title: &str, is_playing: bool) {
        let key = (artist.to_string(), title.to_string());
        if self.last_presence_key.as_ref() != Some(&key) {
            self.presence.set_track(artist, title);
            self.discord.set_track(artist, title);
            self.last_presence_key = Some(key);
        }
        self.presence.set_playing(is_playing);
    }

    /// Synchronizes playback status with desktop presence.
    pub fn sync_presence_playing(&mut self, is_playing: bool) {
        self.presence.set_playing(is_playing);
    }

    /// Sanitizes cookie input, updates keyring session store, and binds catalog client.
    pub fn sign_in_from_input(&mut self, cleaned: &str) -> bool {
        let Some(cookies) = sanitize_cookie_input(cleaned) else {
            return false;
        };
        self.session.save(&cookies);
        if let Ok(mut c) = self.catalog.lock() {
            c.set_session_cookies(Some(&cookies));
        }
        self.signed_in = true;
        true
    }

    /// Imports session from companion QR/handoff code string.
    pub fn import_handoff(&mut self, text: &str) -> bool {
        let Ok(cookies) = decode_handoff(text) else {
            return false;
        };
        self.session.save(&cookies);
        if let Ok(mut c) = self.catalog.lock() {
            c.set_session_cookies(Some(&cookies));
        }
        self.signed_in = true;
        true
    }

    /// Clears active session and resets catalog client to anonymous browsing.
    pub fn sign_out(&mut self) {
        self.session.clear();
        if let Ok(mut c) = self.catalog.lock() {
            c.set_session_cookies(None);
        }
        self.signed_in = false;
    }

    /// Evaluates sleep timer duration and pauses audio playback on expiration.
    pub fn poll_sleep_timer(&mut self) {
        let audio_arc = Arc::clone(&self.audio);
        let Ok(mut audio) = audio_arc.try_lock() else {
            return;
        };
        if !self.sleep_timer.is_armed() || !audio.is_playing() {
            return;
        }
        let elapsed = audio.position_secs();
        let effective = self
            .sleep_timer_accumulated
            .saturating_add(elapsed.saturating_sub(self.sleep_timer_base_offset));
        if self.sleep_timer.poll_elapsed(effective) {
            audio.pause();
            drop(audio);
            self.sync_presence_playing(false);
        }
    }
}

/// Synchronizes state with Slint window properties without performing blocking I/O.
fn sync_window<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
) {
    apply_theme(window, state.theme);
    window.set_active_page(state.page.to_index());
    window.set_header_title(state.page.title().into());
    let is_playing = state.audio.try_lock().map(|a| a.is_playing()).unwrap_or(false);
    window.set_is_playing(is_playing);
    let volume = state.audio.try_lock().map(|a| a.volume()).unwrap_or(100);
    window.set_volume(volume as i32);
    window.set_signed_in(state.signed_in);

    let (title, artist, dur_str, dur_secs) = match state.queue.current() {
        Some(t) => (
            t.title.clone(),
            t.artist.clone(),
            format_track_duration(t.duration_secs),
            t.duration_secs.unwrap_or(0),
        ),
        None => ("No track playing".to_string(), String::new(), "0:00".to_string(), 0),
    };
    window.set_active_track_title(title.into());
    window.set_active_track_artist(artist.into());
    window.set_duration_text(dur_str.into());

    let pos_secs = state.audio.try_lock().map(|a| a.position_secs()).unwrap_or(0) as u32;
    window.set_position_text(format_position(pos_secs).into());
    let progress =
        if dur_secs > 0 { (pos_secs as f32 / dur_secs as f32).clamp(0.0, 1.0) } else { 0.0 };
    window.set_playback_progress(progress);

    sync_cover_and_lyrics(window, state);
    sync_shelves_and_queue(window, state);
    sync_library_and_detail(window, state);
}

/// Synchronizes cover art texture and synced lyrics lines with Slint window from pre-decoded state.
fn sync_cover_and_lyrics<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
) {
    if let Some(img) = &state.current_cover_image {
        window.set_cover_image(img.clone());
        window.set_has_cover(true);
    } else {
        window.set_has_cover(false);
    }
    let line_data: Vec<LyricLineData> = state
        .lyrics_lines
        .iter()
        .enumerate()
        .map(|(i, l)| LyricLineData { index: i as i32, text: l.text.clone().into() })
        .collect();
    window.set_lyrics_lines(ModelRc::new(VecModel::from(line_data)));
    window.set_active_lyric_index(state.active_lyric_index());
}

/// Synchronizes catalog shelves and playback queue rows with Slint window.
fn sync_shelves_and_queue<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    window: &MoggytWindow,
    state: &SlintAppState<C, A, S, L, K>,
) {
    let mut shelf_models = Vec::new();
    for shelf in &state.home_shelves {
        let mut tracks = Vec::new();
        for (i, item) in shelf.items.iter().enumerate() {
            if let SearchResult::Track(t) = item {
                tracks.push(TrackRowData {
                    index: i as i32,
                    title: t.title.clone().into(),
                    artist: t.artist.clone().into(),
                    duration: format_duration(t.duration_secs).into(),
                });
            }
        }
        shelf_models.push(ShelfData {
            title: shelf.title.clone().into(),
            tracks: ModelRc::new(VecModel::from(tracks)),
        });
    }
    window.set_shelves(ModelRc::new(VecModel::from(shelf_models)));

    let q_tracks: Vec<TrackRowData> = state
        .queue
        .tracks()
        .iter()
        .enumerate()
        .map(|(i, t)| TrackRowData {
            index: i as i32,
            title: t.title.clone().into(),
            artist: t.artist.clone().into(),
            duration: format_duration(t.duration_secs).into(),
        })
        .collect();
    window.set_queue_tracks(ModelRc::new(VecModel::from(q_tracks)));
    window.set_queue_selected_index(state.queue.cursor() as i32);
}

/// Synchronizes library collections and detail view models with Slint window.
fn sync_library_and_detail<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    window: &MoggytWindow,
    state: &SlintAppState<C, A, S, L, K>,
) {
    let playlists: Vec<LibraryItemData> = state
        .library_playlists
        .iter()
        .enumerate()
        .map(|(i, p)| LibraryItemData {
            index: i as i32,
            title: p.title.clone().into(),
            subtitle: format!("{} tracks", p.track_count).into(),
        })
        .collect();
    window.set_library_playlists(ModelRc::new(VecModel::from(playlists)));

    let albums: Vec<LibraryItemData> = state
        .library_albums
        .iter()
        .enumerate()
        .map(|(i, a)| LibraryItemData {
            index: i as i32,
            title: a.title.clone().into(),
            subtitle: a.artist.clone().into(),
        })
        .collect();
    window.set_library_albums(ModelRc::new(VecModel::from(albums)));

    let artists: Vec<LibraryItemData> = state
        .library_artists
        .iter()
        .enumerate()
        .map(|(i, a)| LibraryItemData {
            index: i as i32,
            title: a.name.clone().into(),
            subtitle: "Artist".into(),
        })
        .collect();
    window.set_library_artists(ModelRc::new(VecModel::from(artists)));

    if let Some(detail) = &state.detail {
        window.set_detail_has_detail(true);
        window.set_detail_title(detail.title.clone().into());
        window.set_detail_subtitle(detail.subtitle.clone().into());
        window.set_detail_kind_label(detail_kind_label(detail.kind).into());
        let d_tracks: Vec<TrackRowData> = detail
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| TrackRowData {
                index: i as i32,
                title: t.title.clone().into(),
                artist: t.artist.clone().into(),
                duration: format_duration(t.duration_secs).into(),
            })
            .collect();
        window.set_detail_tracks(ModelRc::new(VecModel::from(d_tracks)));
    } else {
        window.set_detail_has_detail(false);
    }
}

/// Periodic playback tick syncing elapsed timestamps, progress, and lyrics line without blocking.
fn sync_playback_tick<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
) {
    let Ok(audio) = state.audio.try_lock() else {
        return;
    };
    let pos_secs = audio.position_secs() as u32;
    let is_playing = audio.is_playing();
    drop(audio);

    window.set_position_text(format_position(pos_secs).into());
    let dur_secs = state.queue.current().and_then(|t| t.duration_secs).unwrap_or(0);
    let progress =
        if dur_secs > 0 { (pos_secs as f32 / dur_secs as f32).clamp(0.0, 1.0) } else { 0.0 };
    window.set_playback_progress(progress);
    window.set_active_lyric_index(state.active_lyric_index_at(pos_secs));
    window.set_is_playing(is_playing);
}

/// Triggers asynchronous track playback on the background worker thread.
fn trigger_playback<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let Some(track) = state.queue.current().cloned() else {
        return;
    };
    state.play_generation = state.play_generation.wrapping_add(1);
    let generation = state.play_generation;
    state.maybe_scrobble_current();

    state.current_cover_image = None;
    state.lyrics_lines.clear();
    state.lyrics_key = None;

    window.set_is_playing(true);
    window.set_active_track_title(track.title.clone().into());
    window.set_active_track_artist(track.artist.clone().into());
    window.set_duration_text(format_track_duration(track.duration_secs).into());
    window.set_position_text("0:00".into());
    window.set_playback_progress(0.0);
    window.set_has_cover(false);
    window.set_lyrics_lines(ModelRc::new(VecModel::default()));
    window.set_active_lyric_index(-1);

    let req = PlayTrackRequest {
        generation,
        track_id: track.id.clone(),
        title: track.title.clone(),
        artist: track.artist.clone(),
        artwork_url: track.artwork.clone(),
        duration_secs: track.duration_secs,
        hi_res_covers: state.hi_res_covers,
        lyric_overrides: state.lyric_overrides.clone(),
    };
    let _ = worker_tx.send(PlaybackWorkerCommand::Play(req));
}

/// Applies completed playback worker results back to the Slint window on the event loop.
pub fn apply_play_track_completion<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
    res: PlayTrackResult,
) {
    if !res.success {
        window.set_active_track_title("Failed to play track".into());
        window.set_is_playing(false);
        return;
    }
    window.set_is_playing(true);
    state.lyrics_lines = res.lyrics_lines;
    state.lyrics_key = Some((res.artist.clone(), res.title.clone()));

    let line_data: Vec<LyricLineData> = state
        .lyrics_lines
        .iter()
        .enumerate()
        .map(|(i, l)| LyricLineData { index: i as i32, text: l.text.clone().into() })
        .collect();
    window.set_lyrics_lines(ModelRc::new(VecModel::from(line_data)));
    window.set_active_lyric_index(state.active_lyric_index());

    if let Some(cover) = res.cover_data {
        if let Some(img) = rgba_to_image(&cover.rgba, cover.width, cover.height) {
            state.current_cover_image = Some(img.clone());
            window.set_cover_image(img);
            window.set_has_cover(true);
        } else {
            state.current_cover_image = None;
            window.set_has_cover(false);
        }
    } else {
        state.current_cover_image = None;
        window.set_has_cover(false);
    }

    state.sync_presence_track(&res.artist, &res.title, true);
    state.started_at = Some((Instant::now(), current_unix_secs()));
    state.last_skip_target = None;
}

/// Dispatches asynchronous search query off the UI thread.
fn submit_search_async<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
    query: &str,
) {
    state.search_query = query.to_string();
    state.search_generation = state.search_generation.wrapping_add(1);
    let generation = state.search_generation;

    state.home_shelves = vec![Shelf::new("Searching...")];
    sync_shelves_and_queue(window, state);

    let cat = Arc::clone(&state.catalog);
    let q = query.to_string();
    let weak = window.as_weak();

    std::thread::spawn(move || {
        let (g, q_res, shelves) = execute_search(generation, &q, &cat);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else { return };
            with_ui_state::<C, A, S, L, K, _>(|st| {
                if st.search_generation == g {
                    st.search_query = q_res;
                    st.home_shelves = shelves;
                    sync_shelves_and_queue(&win, st);
                }
            });
        });
    });
}

/// Dispatches asynchronous container detail lookup off the UI thread.
fn open_detail_async<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
    browse_id: &str,
    kind: DetailKind,
) {
    let trimmed = browse_id.trim();
    if trimmed.is_empty() {
        return;
    }
    if state.page != SlintPage::Detail {
        state.detail_return = state.page;
    }
    state.page = SlintPage::Detail;
    state.detail = Some(Detail::new(kind, "Loading...", "", Vec::new()));
    sync_window(window, state);

    state.detail_generation = state.detail_generation.wrapping_add(1);
    let generation = state.detail_generation;
    let id = trimmed.to_string();
    let cat = Arc::clone(&state.catalog);
    let weak = window.as_weak();

    std::thread::spawn(move || {
        let (g, detail) = execute_detail(generation, &id, kind, &cat);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else { return };
            with_ui_state::<C, A, S, L, K, _>(|st| {
                if st.detail_generation == g {
                    st.detail = detail;
                    sync_library_and_detail(&win, st);
                }
            });
        });
    });
}

/// Dispatches asynchronous library refresh off the UI thread.
fn refresh_library_async<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
) {
    state.library_generation = state.library_generation.wrapping_add(1);
    let generation = state.library_generation;
    let cat = Arc::clone(&state.catalog);
    let weak = window.as_weak();

    std::thread::spawn(move || {
        let (g, playlists, albums) = execute_refresh_library(generation, &cat);
        let _ = dispatch_to_ui(move || {
            let Some(win) = weak.upgrade() else { return };
            with_ui_state::<C, A, S, L, K, _>(|st| {
                if st.library_generation == g {
                    st.library_playlists = playlists;
                    st.library_albums = albums;
                    st.library_artists = Vec::new();
                    sync_library_and_detail(&win, st);
                }
            });
        });
    });
}

/// Non-blocking handler for playback toggle (pause, resume, or start head track).
fn handle_toggle_play<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state: &mut SlintAppState<C, A, S, L, K>,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let audio_arc = Arc::clone(&state.audio);
    let Ok(mut audio) = audio_arc.try_lock() else {
        return;
    };
    if audio.is_playing() {
        audio.pause();
        drop(audio);
        state.sync_presence_playing(false);
        window.set_is_playing(false);
        return;
    }
    if audio.last_requested().is_some() {
        audio.resume_play();
        drop(audio);
        state.sync_presence_playing(true);
        window.set_is_playing(true);
        return;
    }
    drop(audio);
    trigger_playback(window, state, worker_tx);
}

/// Spawns the dedicated playback worker thread processing play and scrobble commands.
fn spawn_playback_worker<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    adapters: PlaybackWorkerAdapters<A, L, K>,
    rx: std::sync::mpsc::Receiver<PlaybackWorkerCommand>,
) {
    let weak = window.as_weak();

    std::thread::Builder::new()
        .name("moggyt-playback".into())
        .spawn(move || {
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    PlaybackWorkerCommand::Play(req) => {
                        let res = execute_play_track(req, &adapters);
                        let w = weak.clone();
                        let _ = dispatch_to_ui(move || {
                            let Some(win) = w.upgrade() else { return };
                            with_ui_state::<C, A, S, L, K, _>(|st| {
                                if st.play_generation == res.generation {
                                    apply_play_track_completion(&win, st, res);
                                }
                            });
                        });
                    }
                    PlaybackWorkerCommand::Scrobble { artist, title, started_unix } => {
                        execute_scrobble(
                            &artist,
                            &title,
                            started_unix,
                            &adapters.scrobbler,
                            &adapters.secondary_scrobbler,
                        );
                    }
                }
            }
        })
        .expect("failed to spawn playback worker thread");
}

/// Builds and configures MoggytWindow wired to SlintAppState with callbacks and background timer.
pub fn build_window<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: SlintAppState<C, A, S, L, K>,
) -> anyhow::Result<(MoggytWindow, slint::Timer)> {
    let window = MoggytWindow::new().context("failed to instantiate MoggytWindow")?;
    let state_rc = Rc::new(RefCell::new(state));
    set_ui_state(&state_rc);

    let adapters = PlaybackWorkerAdapters::new(
        Arc::clone(&state_rc.borrow().audio),
        Arc::clone(&state_rc.borrow().scrobbler),
        state_rc.borrow().secondary_scrobbler.clone(),
        Arc::clone(&state_rc.borrow().lyrics),
        Arc::clone(&state_rc.borrow().covers),
    );

    let (worker_tx, worker_rx) = std::sync::mpsc::channel::<PlaybackWorkerCommand>();
    spawn_playback_worker::<C, A, S, L, K>(&window, adapters, worker_rx);

    sync_window(&window, &mut state_rc.borrow_mut());
    wire_window_callbacks(&window, Rc::clone(&state_rc), worker_tx);
    let timer = setup_app_timer(&window, Rc::clone(&state_rc));

    Ok((window, timer))
}

/// Wires user event callbacks between MoggytWindow and the application state without blocking.
fn wire_window_callbacks<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state_rc: SharedState<C, A, S, L, K>,
    worker_tx: std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    window.on_page_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let page = SlintPage::from_index(idx);
        st.page = page;
        if page == SlintPage::Library {
            refresh_library_async(&win, &mut st);
        }
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    window.on_query_submitted(move |q| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        submit_search_async(&win, &mut st, q.as_str());
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_track_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let track_opt = st
            .home_shelves
            .iter()
            .flat_map(|sh| {
                sh.items.iter().filter_map(|it| match it {
                    SearchResult::Track(t) => Some(t.clone()),
                    _ => None,
                })
            })
            .nth(idx as usize);
        if let Some(track) = track_opt {
            st.queue.push(track);
            let last_idx = st.queue.len().saturating_sub(1);
            st.play_track_at_index_in_memory(last_idx);
            st.page = SlintPage::NowPlaying;
            trigger_playback(&win, &mut st, &tx);
        }
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_queue_track_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.play_track_at_index_in_memory(idx as usize);
        trigger_playback(&win, &mut st, &tx);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    window.on_playlist_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(p) = st.library_playlists.get(idx as usize).cloned() {
            open_detail_async(&win, &mut st, &p.id, DetailKind::Playlist);
        }
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    window.on_album_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(a) = st.library_albums.get(idx as usize).cloned() {
            open_detail_async(&win, &mut st, &a.id, DetailKind::Album);
        }
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    window.on_artist_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(a) = st.library_artists.get(idx as usize).cloned() {
            open_detail_async(&win, &mut st, &a.id, DetailKind::Artist);
        }
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_detail_track_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if let Some(detail) = st.detail.clone() {
            if let Some(track) = detail.tracks.get(idx as usize).cloned() {
                st.queue.push(track);
                let last = st.queue.len().saturating_sub(1);
                st.play_track_at_index_in_memory(last);
                trigger_playback(&win, &mut st, &tx);
            }
        }
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_toggle_play(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        handle_toggle_play(&win, &mut st, &tx);
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_prev_track(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.queue.retreat();
        trigger_playback(&win, &mut st, &tx);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    let tx = worker_tx;
    window.on_next_track(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.queue.advance();
        trigger_playback(&win, &mut st, &tx);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    window.on_seek_by(move |delta| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.seek_by(delta as i64);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    window.on_set_volume(move |vol| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.set_volume(vol.clamp(0, 100) as u8);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    window.on_sign_in_submitted(move |cookies| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if st.sign_in_from_input(cookies.as_str()) {
            win.set_cookie_text("".into());
            refresh_library_async(&win, &mut st);
        }
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(&state_rc);
    let w = window.as_weak();
    window.on_sign_out(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.sign_out();
        refresh_library_async(&win, &mut st);
        sync_window(&win, &mut st);
    });
}

/// Sets up periodic background timer for tray events, media keys, auto-skip, and tick sync.
fn setup_app_timer<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    window: &MoggytWindow,
    state_rc: SharedState<C, A, S, L, K>,
) -> slint::Timer {
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(250), move || {
        let Some(win) = weak.upgrade() else { return };
        let mut state = state_rc.borrow_mut();
        if state.drain_tray_actions() {
            let _ = state.queue_store.save(&state.queue);
            slint::quit_event_loop().ok();
            return;
        }
        state.drain_media_keys();
        state.check_auto_skip();
        state.continue_autoplay_if_ended();
        state.poll_sleep_timer();
        sync_playback_tick(&win, &mut state);
    });
    timer
}

/// Parses CLI arguments and executes single-instance URL/handoff import commands.
fn handle_cli_args<
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
///
/// TODO Gap: Update banner badge: GithubUpdater checks for updates against GitHub
/// Releases on startup, but ui/shell.slint lacks an update banner badge.
fn check_updater_status<
    C: Catalog + Send + 'static,
    A: AudioBackend + Send + 'static,
    S: SessionStore + 'static,
    L: LyricsSource + Send + Sync + 'static,
    K: CoverCache + Send + 'static,
>(
    state: &mut SlintAppState<C, A, S, L, K>,
) {
    if let Some(info) = state.updater.latest() {
        tracing::info!(version = %info.version, url = %info.url, "update available");
    }
}

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
    use crate::domain::SkipSegment;
    use crate::infra::{AudioStub, CatalogStub, LrclibStub, MemoryCoverCache, MemorySessionStore};

    fn test_wired()
    -> SlintAppState<CatalogStub, AudioStub, MemorySessionStore, LrclibStub, MemoryCoverCache> {
        let catalog = CatalogStub::new();
        let audio = AudioStub::new();
        let session = MemorySessionStore::new();
        let lyrics = LrclibStub::new();
        let covers = MemoryCoverCache::new();
        SlintAppState::new(catalog, audio, session, lyrics, covers)
    }

    #[test]
    fn test_initial_state_defaults() {
        let state = test_wired();
        assert_eq!(state.page, SlintPage::Search);
        assert!(state.queue.is_empty());
        assert!(!state.home_shelves.is_empty());
        assert_eq!(state.active_lyric_index(), -1);
    }

    #[test]
    fn test_page_navigation_and_library_refresh() {
        let mut state = test_wired();
        assert_eq!(state.page, SlintPage::Search);
        state.select_page(SlintPage::Library);
        assert_eq!(state.page, SlintPage::Library);
        assert!(!state.library_playlists.is_empty());
        assert!(!state.library_albums.is_empty());
    }

    #[test]
    fn test_playback_controls_toggle_and_queue_navigation() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.queue.push(Track::new("t2", "Track 2", "Artist 2").with_duration(200));

        state.play_current_quiet();
        assert!(state.audio.lock().unwrap().is_playing());
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));

        state.toggle_play();
        assert!(!state.audio.lock().unwrap().is_playing());

        state.toggle_play();
        assert!(state.audio.lock().unwrap().is_playing());

        state.next_track();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t2"));

        state.prev_track();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("t1"));
    }

    #[test]
    fn test_seek_and_volume_controls() {
        let mut state = test_wired();
        state.set_volume(75);
        assert_eq!(state.audio.lock().unwrap().volume(), 75);

        state.seek_by(15);
        assert_eq!(state.audio.lock().unwrap().position_secs(), 15);

        state.seek_by(-50);
        assert_eq!(state.audio.lock().unwrap().position_secs(), 0);
    }

    #[test]
    fn test_search_submission() {
        let mut state = test_wired();
        state.submit_search("midnight");
        assert_eq!(state.search_query, "midnight");
        assert_eq!(state.home_shelves.len(), 1);
        assert_eq!(state.home_shelves[0].title, "Search Results");
    }

    #[test]
    fn test_sign_in_and_out_lifecycle() {
        let mut state = test_wired();
        assert!(!state.signed_in);

        let ok = state.sign_in_from_input("SID=valid_cookie_123");
        assert!(ok);
        assert!(state.signed_in);
        assert_eq!(state.session.load().as_deref(), Some("SID=valid_cookie_123"));

        state.sign_out();
        assert!(!state.signed_in);
        assert_eq!(state.session.load(), None);
    }

    #[test]
    fn test_sign_in_empty_input_rejected() {
        let mut state = test_wired();
        assert!(!state.signed_in);

        let ok = state.sign_in_from_input("   ");
        assert!(!ok);
        assert!(!state.signed_in);
        assert_eq!(state.session.load(), None);
    }

    #[test]
    fn test_detail_view_open_and_close() {
        let mut state = test_wired();
        state.open_detail("album-1", DetailKind::Album);
        assert_eq!(state.page, SlintPage::Detail);
        assert!(state.detail.is_some());
    }

    #[test]
    fn test_auto_skip_evaluation() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        let mut ranges = crate::domain::SkipRanges::new();
        ranges.insert(SkipSegment::new(10, 20).unwrap());
        let mut map = std::collections::HashMap::new();
        map.insert("t1".to_string(), ranges);
        let _ = state.skip_store.save(&map);

        state.seek_by(12);
        state.check_auto_skip();
        assert_eq!(state.audio.lock().unwrap().position_secs(), 20);
    }

    #[test]
    fn test_sleep_timer_expiry() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1").with_duration(180));
        state.play_current_quiet();
        state.sleep_timer.set_preset(crate::domain::TimerPreset::Minutes15);
        assert!(state.sleep_timer.is_armed());

        state.seek_by(900);
        state.poll_sleep_timer();
        assert!(!state.audio.lock().unwrap().is_playing());
    }
}
