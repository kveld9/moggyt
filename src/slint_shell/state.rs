//! Application state, collaborator containers, and core domain mutations.
//!
//! Hexagonal invariant: holds injected port adapters in thread-safe containers (`Arc<Mutex<..>>`)
//! and exposes non-blocking mutation methods for UI events.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::domain::{
    Album, Detail, DetailKind, Palette, PlaybackPhase, PlaybackQueue, PlayerBarMode, Playlist,
    RecentSearches, SearchFilter, SearchQuery, SearchResult, Shelf, SkipRanges, SleepTimer,
    ThemeId, Track, extract_playlist_id,
};
use crate::i18n::{Locale, tr};
use crate::infra::{
    AudioBackend, AudioError, Catalog, CatalogError, CoverCache, CustomThemeFile, DesktopPresence,
    DeviceIdentityMonitor, DiscordPresence, LyricLine, LyricOverrideDir, LyricsSource,
    MediaKeyAction, OutputDeviceWatch, PlayLogStore, QueueStore, ReleaseInfo, Scrobbler,
    SessionStore, SettingsStore, SkipProvider, SkipStore, TrayAction, TrayHandle, decode_cover,
    fetch_artwork,
};
use crate::slint_shell::auto_skip::{SponsorFetchOutcome, SponsorRangeState};
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::format::cycle_playback_speed_preset;
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::platform::rgba_to_image;
use crate::slint_shell::playback_control::StartedPlay;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::player_bar::PlayerBarPrefs;
use crate::slint_shell::view_phase::ViewPhase;

pub type SharedCatalog = Arc<Mutex<Box<dyn Catalog + Send>>>;
pub type SharedAudio = Arc<Mutex<Box<dyn AudioBackend + Send>>>;
pub type SharedLyrics = Arc<Box<dyn LyricsSource + Send + Sync>>;
pub type SharedCovers = Arc<Mutex<Box<dyn CoverCache + Send>>>;

/// Store whose persisted payload failed to load cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceStore {
    Settings,
    Queue,
}

/// Unresolved persistence problem that blocks writes until acknowledged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistenceError {
    pub store: PersistenceStore,
    pub health: crate::infra::StoreHealth,
}

/// In-memory state and collaborator wiring for the Slint UI shell.
pub struct SlintAppState {
    pub catalog: SharedCatalog,
    pub audio: SharedAudio,
    pub session: Box<dyn SessionStore>,
    pub lyrics: SharedLyrics,
    pub covers: SharedCovers,
    pub lyric_overrides: Option<LyricOverrideDir>,
    pub tray: Option<TrayHandle>,
    pub presence: DesktopPresence,
    pub discord: DiscordPresence,
    pub scrobbler: Arc<Mutex<Box<dyn Scrobbler>>>,
    pub secondary_scrobbler: Option<Arc<Mutex<Box<dyn Scrobbler>>>>,
    pub settings_store: Box<dyn SettingsStore>,
    pub queue_store: Box<dyn QueueStore>,
    pub skip_store: Box<dyn SkipStore>,
    pub play_log_store: Box<dyn PlayLogStore>,
    /// In-memory audio version override table applied to playback resolution.
    pub version_overrides: crate::domain::VersionOverrides,
    /// Persistence adapter for the override table.
    pub version_store: Box<dyn crate::infra::VersionOverrideStore>,
    /// Transient picker state for audio version overrides.
    pub version_fix: crate::slint_shell::version_fix::VersionFixState,
    /// Row that owns the open right-click context menu, when any.
    pub context_menu: crate::slint_shell::context_menu::ContextMenuState,
    pub updater_rx: Option<std::sync::mpsc::Receiver<Option<ReleaseInfo>>>,

    pub page: SlintPage,
    pub queue_panel_open: bool,
    pub queue: PlaybackQueue,
    pub home_shelves: Vec<Shelf>,
    pub search_query: String,
    pub search_active: bool,
    pub recent_searches: RecentSearches,
    pub search_results: Vec<Track>,
    pub library_playlists: Vec<Playlist>,
    pub library_albums: Vec<Album>,
    pub detail: Option<Detail>,
    pub detail_return: SlintPage,
    pub playback_phase: PlaybackPhase,
    pub library_phase: ViewPhase,
    pub search_phase: ViewPhase,
    pub detail_phase: ViewPhase,
    pub explore_phase: ViewPhase,
    pub category_phase: ViewPhase,
    pub lyrics_lines: Vec<LyricLine>,
    pub lyrics_key: Option<(String, String)>,
    pub cover_key: Option<String>,
    pub current_cover_image: Option<slint::Image>,
    pub last_presence_key: Option<(String, String)>,
    pub started_at: Option<StartedPlay>,
    pub last_skip_target: Option<(String, u32)>,
    pub cached_skip_ranges: Option<(String, Option<SkipRanges>)>,
    /// Lifecycle of community skip-range fetches, keyed by video id.
    pub(crate) sponsor_range_states: HashMap<String, SponsorRangeState>,
    /// Channel sender for completed community skip fetches.
    pub(crate) sponsor_tx: Option<std::sync::mpsc::Sender<(String, SponsorFetchOutcome)>>,
    /// Channel receiver drained by the app timer.
    pub(crate) sponsor_rx: Option<std::sync::mpsc::Receiver<(String, SponsorFetchOutcome)>>,
    /// Community skip provider; `None` disables automatic community skipping.
    pub skip_provider: Option<Arc<dyn SkipProvider>>,
    /// Debounced watcher of the system default output device.
    pub output_device_watch: OutputDeviceWatch,
    /// Background monitor of the system output device identity.
    pub device_monitor: Option<DeviceIdentityMonitor>,
    pub sleep_timer: SleepTimer,
    pub sleep_timer_accumulated: u64,
    pub sleep_timer_base_offset: u64,
    /// User volume captured when progressive sleep timer fade started; None when no fade is active.
    pub sleep_fade_base_volume: Option<u8>,
    pub autoplay: bool,
    pub crossfade_seconds: u32,
    pub last_crossfaded_track: Option<String>,
    pub stream_cache_limit_mb: u32,
    pub hi_res_covers: bool,
    pub songs_only: bool,
    pub signed_in: bool,
    pub theme: ThemeId,
    pub custom_theme: Option<CustomThemeFile>,
    pub custom_palette: Option<Palette>,
    pub custom_theme_status: String,
    /// Watcher for the matugen-generated palette file.
    pub matugen_theme: Option<CustomThemeFile>,
    /// Palette loaded from the matugen file, when available.
    pub matugen_palette: Option<Palette>,
    /// Human-readable status of the last matugen theme load attempt.
    pub matugen_theme_status: String,
    /// Optional accent override for the built-in preset themes.
    pub accent_override: Option<[u8; 3]>,
    pub locale: Locale,
    pub player_bar: PlayerBarPrefs,
    pub search_filter: SearchFilter,
    /// Whether the active search treats the query text as a lyric fragment.
    pub lyrics_search: bool,
    pub search_covers: HashMap<String, slint::Image>,
    pub liked_ids: HashSet<String>,
    pub now_playing_cover: Option<slint::Image>,
    pub is_shuffle: bool,
    pub unshuffled_tracks: Option<Vec<crate::domain::Track>>,
    pub update_available: bool,
    pub latest_version: Option<String>,

    pub play_generation: u64,
    pub search_generation: u64,
    pub detail_generation: u64,
    pub detail_error: Option<String>,
    pub detail_request: Option<(String, crate::domain::DetailKind)>,
    pub library_generation: u64,
    pub explore_generation: u64,
    pub explore_shelves: Vec<Shelf>,
    pub explore_loaded: bool,
    pub explore_loading: bool,
    pub category_shelves: Vec<Shelf>,
    pub category_title: String,
    pub category_loading: bool,
    pub category_generation: u64,
    pub category_return: SlintPage,
    /// Explore grid columns for the current window width.
    pub(crate) explore_columns: usize,

    pub worker_tx: Option<std::sync::mpsc::Sender<PlaybackWorkerCommand>>,
    pub persistence_error: Option<PersistenceError>,
    /// Last observed audio backend state, reused while the backend is busy.
    pub audio_snapshot: crate::slint_shell::audio_snapshot::AudioSnapshot,
}

/// Shared reference-counted wrapper for [`SlintAppState`].
pub type SharedState = Rc<RefCell<SlintAppState>>;

impl SlintAppState {
    /// Clears the reported persistence error and unblocks the affected stores.
    pub fn dismiss_persistence_error(&mut self) {
        self.persistence_error = None;
        self.settings_store.clear_health();
        self.queue_store.clear_health();
    }
    /// Updates active UI theme and persists preference to disk settings store.
    pub fn set_theme(&mut self, theme: ThemeId) {
        self.theme = theme;
        let mut settings = self.settings_store.load();
        settings.theme = theme;
        let _ = self.settings_store.save(&settings);
    }

    /// Sets or clears the preset accent override and persists it.
    pub fn set_accent_override(&mut self, accent: Option<[u8; 3]>) {
        self.accent_override = accent;
        let mut settings = self.settings_store.load();
        settings.accent_override = accent.map(crate::domain::format_hex_rgb);
        let _ = self.settings_store.save(&settings);
    }

    /// Stores the current playback phase; this is the only assignment site.
    pub(crate) fn set_playback_phase(&mut self, phase: PlaybackPhase) {
        self.playback_phase = phase;
    }

    /// Updates the crossfade duration setting and persists preference to disk settings store.
    pub fn set_crossfade_seconds(&mut self, seconds: u32) {
        let clamped = crate::infra::clamp_crossfade_seconds(seconds);
        self.crossfade_seconds = clamped;
        let mut settings = self.settings_store.load();
        settings.crossfade_seconds = clamped;
        let _ = self.settings_store.save(&settings);
    }

    /// Updates the stream cache size limit and persists preference to disk settings store.
    pub fn set_stream_cache_limit_mb(&mut self, limit_mb: u32) {
        let clamped = limit_mb.min(10240);
        self.stream_cache_limit_mb = clamped;
        let mut settings = self.settings_store.load();
        settings.stream_cache_limit_mb = clamped;
        let _ = self.settings_store.save(&settings);
    }

    /// Updates the player bar layout mode and persists to settings store.
    pub fn set_player_bar_mode(&mut self, mode: PlayerBarMode) {
        self.player_bar.mode = mode;
        let mut settings = self.settings_store.load();
        self.player_bar.apply_to_settings(&mut settings);
        let _ = self.settings_store.save(&settings);
    }

    /// Toggles an individual player bar element option by index and persists to settings store.
    pub fn toggle_player_bar_option(&mut self, index: i32) {
        self.player_bar = self.player_bar.toggled(index);
        let mut settings = self.settings_store.load();
        self.player_bar.apply_to_settings(&mut settings);
        let _ = self.settings_store.save(&settings);
    }

    /// Cycles to the next playback speed preset and persists preference.
    pub fn cycle_playback_speed(&mut self) -> f32 {
        let current = self.audio.lock().map(|a| a.speed()).unwrap_or(1.0);
        let next = cycle_playback_speed_preset(current);
        self.set_playback_speed(next);
        next
    }

    /// Toggles the queue side panel open or closed.
    pub fn toggle_queue_panel(&mut self) {
        self.queue_panel_open = !self.queue_panel_open;
    }

    /// Closes the queue side panel.
    pub fn close_queue_panel(&mut self) {
        self.queue_panel_open = false;
    }

    /// Selects active navigation page and refreshes content when entering Library or Explore view.
    pub fn select_page(&mut self, page: SlintPage) {
        self.page = page;
        if page == SlintPage::Library {
            self.refresh_library();
        } else if page == SlintPage::Explore && !self.explore_loaded {
            self.refresh_explore();
        }
    }

    /// Refreshes explore shelves from the injected catalog port.
    pub fn refresh_explore(&mut self) {
        let res = self.catalog.lock().map_or(Err(CatalogError::Transport), |c| c.explore_shelves());
        match res {
            Ok(shelves) => {
                let is_empty = shelves.is_empty();
                self.explore_shelves = shelves;
                self.explore_phase = if is_empty { ViewPhase::Empty } else { ViewPhase::Ready };
            }
            Err(_) => {
                self.explore_phase = ViewPhase::Error;
            }
        }
        self.explore_loaded = true;
    }

    /// Refreshes playlists, saved albums, and followed artists from catalog.
    pub fn refresh_library(&mut self) {
        let (playlists, albums) = match self.catalog.lock() {
            Ok(c) => (c.playlists(), c.saved_albums()),
            Err(_) => (Err(CatalogError::Transport), Err(CatalogError::Transport)),
        };
        self.apply_library_outcome(playlists, albums);
    }

    /// Submits search query to catalog and wraps hits in a shelf.
    pub fn submit_search(&mut self, query: &str) {
        self.submit_search_with_filter(query, SearchFilter::All);
    }

    /// Submits search query with a filter to catalog and wraps hits in a shelf.
    pub fn submit_search_with_filter(&mut self, query: &str, filter: SearchFilter) {
        self.search_query = query.to_string();
        self.search_filter = filter;
        self.search_phase = ViewPhase::Loading;
        let Some(parsed) = SearchQuery::parse(query).map(|q| q.with_filter(filter)) else {
            self.search_phase = ViewPhase::Idle;
            return;
        };
        self.search_active = true;
        self.recent_searches.record(parsed.text());
        let res = self
            .catalog
            .lock()
            .map_or(Err(CatalogError::Transport), |c| c.search_filtered(&parsed));
        match res {
            Ok(items) => self.apply_search_items(items),
            Err(_) => self.search_phase = ViewPhase::Error,
        }
    }

    /// Exits search mode: clears the query and results, and returns the view to Idle.
    pub fn clear_search(&mut self) {
        self.search_generation = self.search_generation.wrapping_add(1);
        self.search_query.clear();
        self.lyrics_search = false;
        self.search_active = false;
        self.home_shelves.clear();
        self.search_results.clear();
        self.search_phase = ViewPhase::Idle;
    }

    fn apply_search_items(&mut self, mut items: Vec<SearchResult>) {
        if self.songs_only {
            items = crate::domain::retain_song_results(items);
        }
        self.search_results = items
            .iter()
            .filter_map(|it| match it {
                SearchResult::Track(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        let is_empty = items.is_empty();
        let mut shelf = Shelf::new(tr(self.locale, "Search Results"));
        shelf.items = items;
        self.home_shelves = vec![shelf];
        self.search_phase = if is_empty { ViewPhase::Empty } else { ViewPhase::Ready };
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
        self.page = SlintPage::Detail;
        self.apply_sync_detail(trimmed, kind);
    }

    fn apply_sync_detail(&mut self, browse_id: &str, kind: DetailKind) {
        let (detail, phase) = match self
            .catalog
            .lock()
            .map_or(Err(CatalogError::Transport), |c| c.detail(browse_id, kind))
        {
            Ok(Some(d)) => {
                let phase = if crate::slint_shell::detail::detail_is_empty(&d) {
                    ViewPhase::Empty
                } else {
                    ViewPhase::Ready
                };
                (Some(d), phase)
            }
            Ok(None) => (None, ViewPhase::Empty),
            Err(_) => (None, ViewPhase::Error),
        };
        self.detail = detail;
        self.detail_phase = phase;
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
            .map_err(|err| anyhow::anyhow!("failed to fetch playlist {playlist_id}: {err}"))?
            .ok_or_else(|| anyhow::anyhow!("playlist detail not found: {playlist_id}"))?;
        let count = detail.tracks.len();
        let worker_tx = self.worker_tx.clone();
        let generation = self.play_generation;
        for track in detail.tracks {
            if let Some(ref tx) = worker_tx {
                request_queue_track_cover(
                    track.artwork.as_deref(),
                    generation,
                    &self.search_covers,
                    tx,
                );
            }
            self.queue.push(track);
        }
        Ok(count)
    }

    fn cover_bytes_or_fetch(covers: &SharedCovers, url: &str) -> Option<Vec<u8>> {
        if let Some(cached) = covers.lock().ok()?.get(url) {
            return Some(cached);
        }
        let fetched = fetch_artwork(url, 8).ok()?;
        if let Ok(mut c) = covers.lock() {
            c.put(url, fetched.clone());
        }
        Some(fetched)
    }

    fn cover_max_px(&self) -> u32 {
        if self.hi_res_covers { 1024 } else { 512 }
    }

    /// Fetches, caches, decodes, and converts active track cover art to Slint Image.
    pub fn resolve_cover_image(&mut self) -> Option<slint::Image> {
        let url = self.queue.current()?.artwork.clone()?;
        let bytes = Self::cover_bytes_or_fetch(&self.covers, &url)?;
        let max_px = self.cover_max_px();
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
                    self.pause_play_ledger();
                    self.set_playback_phase(PlaybackPhase::Paused);
                }
            }
        }
    }

    /// Sets active custom theme by name, updates handle, and persists preference.
    pub fn set_custom_theme_name(&mut self, name: Option<&str>) {
        if let Some(ref mut custom) = self.custom_theme {
            custom.select_theme(name);
        }
        let mut settings = self.settings_store.load();
        settings.custom_theme_name = name.map(ToString::to_string);
        let _ = self.settings_store.save(&settings);
    }
}

impl<T: ?Sized + Catalog> Catalog for Box<T> {
    fn search(&self, q: &str) -> Result<Vec<Track>, CatalogError> {
        (**self).search(q)
    }
    fn search_filtered(&self, q: &SearchQuery) -> Result<Vec<SearchResult>, CatalogError> {
        (**self).search_filtered(q)
    }
    fn search_by_lyrics(
        &self,
        s: &crate::domain::LyricsSnippet,
    ) -> Result<Vec<Track>, CatalogError> {
        (**self).search_by_lyrics(s)
    }
    fn radio(&self, id: &str) -> Result<Vec<Track>, CatalogError> {
        (**self).radio(id)
    }
    fn home_shelves(&self) -> Vec<Shelf> {
        (**self).home_shelves()
    }
    fn explore_shelves(&self) -> Result<Vec<Shelf>, CatalogError> {
        (**self).explore_shelves()
    }
    fn category_shelves(&self, id: &str, p: Option<&str>) -> Result<Vec<Shelf>, CatalogError> {
        (**self).category_shelves(id, p)
    }
    fn playlists(&self) -> Result<Vec<Playlist>, CatalogError> {
        (**self).playlists()
    }
    fn saved_albums(&self) -> Result<Vec<Album>, CatalogError> {
        (**self).saved_albums()
    }
    fn detail(&self, id: &str, k: DetailKind) -> Result<Option<Detail>, CatalogError> {
        (**self).detail(id, k)
    }
    fn set_session_cookies(&mut self, c: Option<&str>) {
        (**self).set_session_cookies(c);
    }
    fn set_locale(&mut self, l: Locale) {
        (**self).set_locale(l);
    }
}

impl<T: ?Sized + AudioBackend> AudioBackend for Box<T> {
    fn play(&mut self, id: &str) -> Result<(), AudioError> {
        (**self).play(id)
    }
    fn pause(&mut self) {
        (**self).pause();
    }
    fn resume_play(&mut self) {
        (**self).resume_play();
    }
    fn seek(&mut self, pos: u64) {
        (**self).seek(pos);
    }
    fn set_volume(&mut self, v: u8) {
        (**self).set_volume(v);
    }
    fn volume(&self) -> u8 {
        (**self).volume()
    }
    fn position_secs(&self) -> u64 {
        (**self).position_secs()
    }
    fn is_playing(&self) -> bool {
        (**self).is_playing()
    }
    fn is_paused(&self) -> bool {
        (**self).is_paused()
    }
    fn is_empty(&self) -> bool {
        (**self).is_empty()
    }
    fn duration_secs(&self) -> u64 {
        (**self).duration_secs()
    }
    fn last_requested(&self) -> Option<&str> {
        (**self).last_requested()
    }
    fn set_quality(&mut self, q: crate::infra::AudioQuality) {
        (**self).set_quality(q);
    }
    fn quality(&self) -> crate::infra::AudioQuality {
        (**self).quality()
    }
    fn set_skip_silence(&mut self, on: bool) {
        (**self).set_skip_silence(on);
    }
    fn set_loudness_normalization(&mut self, on: bool) {
        (**self).set_loudness_normalization(on);
    }
    fn set_speed(&mut self, s: f32) {
        (**self).set_speed(s);
    }
    fn speed(&self) -> f32 {
        (**self).speed()
    }
}

impl<T: ?Sized + LyricsSource> LyricsSource for Box<T> {
    fn lookup(
        &self,
        artist: &str,
        title: &str,
        duration_secs: Option<u64>,
    ) -> Option<crate::infra::SyncedLyrics> {
        (**self).lookup(artist, title, duration_secs)
    }
}

impl<T: ?Sized + CoverCache> CoverCache for Box<T> {
    fn get(&self, url: &str) -> Option<Vec<u8>> {
        (**self).get(url)
    }
    fn put(&mut self, url: &str, bytes: Vec<u8>) {
        (**self).put(url, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::{MockLibraryCatalog, test_wired, wrap_mock_catalog};

    #[test]
    fn test_initial_state_defaults() {
        let state = test_wired();
        assert_eq!(state.page, SlintPage::Search);
        assert!(state.queue.is_empty());
        assert!(state.home_shelves.is_empty());
        assert!(state.recent_searches.is_empty());
        assert!(!state.search_active);
        assert_eq!(state.active_lyric_index(), -1);
        assert!(state.liked_ids.is_empty());
        assert!(state.now_playing_cover.is_none());
        assert!(!state.is_shuffle);
        assert_eq!(state.search_query, "");
        assert_eq!(state.explore_columns, crate::slint_shell::sync::DEFAULT_EXPLORE_COLUMNS);
        assert!(!state.queue_panel_open);
    }

    #[test]
    fn test_queue_panel_open_close_and_toggle_lifecycle() {
        let mut state = test_wired();
        assert!(!state.queue_panel_open);
        state.toggle_queue_panel();
        assert!(state.queue_panel_open);
        state.close_queue_panel();
        assert!(!state.queue_panel_open);
        state.select_page(SlintPage::NowPlaying);
        state.toggle_queue_panel();
        assert!(state.queue_panel_open);
        state.toggle_queue_panel();
        assert!(!state.queue_panel_open);
    }

    #[test]
    fn test_page_navigation_and_library_refresh() {
        let mut state = test_wired();
        assert_eq!(state.page, SlintPage::Search);
        state.select_page(SlintPage::Library);
        assert_eq!(state.page, SlintPage::Library);
        assert!(!state.library_playlists.is_empty());
        assert!(!state.library_albums.is_empty());
        state.open_detail("album-1", DetailKind::Album);
        assert_eq!(state.page, SlintPage::Detail);
        assert!(state.detail.is_some());
        state.go_back();
        assert_eq!(state.page, SlintPage::Library);
    }

    #[test]
    fn test_sign_in_and_out_lifecycle() {
        let mut state = test_wired();
        assert!(!state.signed_in);
        assert!(!state.sign_in_from_input("   "));
        assert!(state.sign_in_from_input("SID=valid_cookie_123"));
        assert!(state.signed_in);
        assert_eq!(state.session.load().as_deref(), Some("SID=valid_cookie_123"));
        state.sign_out();
        assert!(!state.signed_in);
        assert_eq!(state.session.load(), None);
    }

    #[test]
    fn state_search_errors_and_empty_phases() {
        let mut state = test_wired();
        let err_cat =
            MockLibraryCatalog::default().with_search_filtered(Err(CatalogError::Transport));
        state.catalog = wrap_mock_catalog(err_cat);
        state.submit_search("rock");
        assert_eq!(state.search_phase, ViewPhase::Error);
        state.submit_search_with_filter("rock", SearchFilter::Albums);
        assert_eq!(state.search_phase, ViewPhase::Error);
        let empty_cat = MockLibraryCatalog::default().with_search_filtered(Ok(Vec::new()));
        state.catalog = wrap_mock_catalog(empty_cat);
        state.submit_search("rock");
        assert_eq!(state.search_phase, ViewPhase::Empty);
        state.submit_search_with_filter("rock", SearchFilter::Albums);
        assert_eq!(state.search_phase, ViewPhase::Empty);
        let hit = SearchResult::Track(Track::new("t1", "Title", "Artist"));
        let ok_cat = MockLibraryCatalog::default().with_search_filtered(Ok(vec![hit]));
        state.catalog = wrap_mock_catalog(ok_cat);
        state.submit_search("rock");
        assert_eq!(state.search_phase, ViewPhase::Ready);
    }

    #[test]
    fn state_detail_errors_and_empty_phases() {
        let mut state = test_wired();
        let err_cat = MockLibraryCatalog::default().with_detail(Err(CatalogError::Transport));
        state.catalog = wrap_mock_catalog(err_cat);
        state.open_detail("album-1", DetailKind::Album);
        assert_eq!(state.detail_phase, ViewPhase::Error);
        assert!(state.detail.is_none());
        let empty_cat = MockLibraryCatalog::default().with_detail(Ok(None));
        state.catalog = wrap_mock_catalog(empty_cat);
        state.open_detail("album-1", DetailKind::Album);
        assert_eq!(state.detail_phase, ViewPhase::Empty);
        assert!(state.detail.is_none());
        let d = Detail::new(DetailKind::Album, "T", "A", vec![Track::new("t1", "T", "A")]);
        let ok_cat = MockLibraryCatalog::default().with_detail(Ok(Some(d)));
        state.catalog = wrap_mock_catalog(ok_cat);
        state.open_detail("album-1", DetailKind::Album);
        assert_eq!(state.detail_phase, ViewPhase::Ready);
        assert!(state.detail.is_some());
    }

    #[test]
    fn test_settings_persist_to_store_and_state() {
        let mut state = test_wired();
        assert_eq!(state.theme, ThemeId::White);
        state.set_theme(ThemeId::BlackOled);
        assert_eq!(state.settings_store.load().theme, ThemeId::BlackOled);
        state.set_theme(ThemeId::Mono);
        assert_eq!(state.settings_store.load().theme, ThemeId::Mono);
        assert_eq!(state.player_bar.mode, PlayerBarMode::Classic);
        state.set_player_bar_mode(PlayerBarMode::Floating);
        assert_eq!(state.settings_store.load().player_bar_mode, PlayerBarMode::Floating);
        state.set_player_bar_mode(PlayerBarMode::Classic);
        assert_eq!(state.settings_store.load().player_bar_mode, PlayerBarMode::Classic);
        assert!(state.player_bar.show_artwork && !state.player_bar.show_skip_seconds);
        state.toggle_player_bar_option(0);
        assert!(!state.settings_store.load().player_bar_show_artwork);
        state.toggle_player_bar_option(4);
        assert!(state.settings_store.load().player_bar_show_skip_seconds);
        state.hi_res_covers = false;
        assert_eq!(state.cover_max_px(), 512);
        state.hi_res_covers = true;
        assert_eq!(state.cover_max_px(), 1024);
        state.set_crossfade_seconds(5);
        assert_eq!(state.settings_store.load().crossfade_seconds, 5);
        state.set_crossfade_seconds(20);
        assert_eq!(state.settings_store.load().crossfade_seconds, 15);
        state.set_stream_cache_limit_mb(1024);
        assert_eq!(state.settings_store.load().stream_cache_limit_mb, 1024);
        state.set_stream_cache_limit_mb(20000);
        assert_eq!(state.settings_store.load().stream_cache_limit_mb, 10240);
    }

    #[test]
    fn test_cover_bytes_or_fetch_cache_hit() {
        let state = test_wired();
        state.covers.lock().unwrap().put("https://example.com/art.jpg", vec![1, 2, 3]);
        let cached =
            SlintAppState::cover_bytes_or_fetch(&state.covers, "https://example.com/art.jpg");
        assert_eq!(cached, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_accent_override_persists_to_store_and_state() {
        let mut state = test_wired();
        state.set_accent_override(Some([0x12, 0x34, 0x56]));
        assert_eq!(state.accent_override, Some([0x12, 0x34, 0x56]));
        assert_eq!(state.settings_store.load().accent_override.as_deref(), Some("#123456"));
        state.set_accent_override(None);
        assert_eq!(state.accent_override, None);
        assert_eq!(state.settings_store.load().accent_override, None);
    }
}
