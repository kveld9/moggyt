//! Application state, collaborator containers, and core domain mutations.
//!
//! Hexagonal invariant: holds injected port adapters in thread-safe containers (`Arc<Mutex<..>>`)
//! and exposes non-blocking mutation methods for UI events.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::domain::{
    Album, Artist, Detail, DetailKind, PlaybackQueue, PlayerBarMode, Playlist, SearchFilter,
    SearchQuery, SearchResult, Shelf, SleepTimer, ThemeId, Track, extract_playlist_id,
};
use crate::i18n::{Locale, tr};
use crate::infra::{
    AudioBackend, Catalog, CoverCache, DesktopPresence, DiscordPresence, GithubUpdater, LyricLine,
    LyricOverrideDir, LyricsSource, MediaKeyAction, MemoryMetaStore, MemoryQueueStore,
    MemorySettingsStore, MemorySkipStore, MetaStore, QueueStore, Scrobbler, SessionStore,
    SettingsStore, SkipStore, StubScrobbler, TrayAction, TrayHandle, decode_cover, decode_handoff,
    fetch_artwork, sanitize_cookie_input,
};
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::format::cycle_playback_speed_preset;
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::platform::rgba_to_image;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::player_bar::PlayerBarPrefs;

/// In-memory state and collaborator wiring for the Slint UI shell.
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
    pub locale: Locale,
    pub player_bar: PlayerBarPrefs,
    pub search_filter: SearchFilter,
    pub search_covers: HashMap<String, slint::Image>,
    pub liked_ids: HashSet<String>,
    pub now_playing_cover: Option<slint::Image>,
    pub is_shuffle: bool,
    pub update_available: bool,
    pub latest_version: Option<String>,

    pub play_generation: u64,
    pub search_generation: u64,
    pub detail_generation: u64,
    pub library_generation: u64,
    pub explore_generation: u64,
    pub explore_shelves: Vec<Shelf>,
    pub explore_loaded: bool,
    pub explore_loading: bool,
    /// Explore grid columns for the current window width.
    pub(crate) explore_columns: usize,

    pub worker_tx: Option<std::sync::mpsc::Sender<PlaybackWorkerCommand>>,
}

/// Shared reference-counted wrapper for [`SlintAppState`].
pub type SharedState<C, A, S, L, K> = Rc<RefCell<SlintAppState<C, A, S, L, K>>>;

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
        catalog.set_locale(Locale::default());
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
            locale: Locale::default(),
            player_bar: PlayerBarPrefs::default(),
            search_filter: SearchFilter::All,
            search_covers: HashMap::new(),
            liked_ids: HashSet::new(),
            now_playing_cover: None,
            is_shuffle: false,
            update_available: false,
            latest_version: None,
            play_generation: 0,
            search_generation: 0,
            detail_generation: 0,
            library_generation: 0,
            explore_generation: 0,
            explore_shelves: Vec::new(),
            explore_loaded: false,
            explore_loading: false,
            explore_columns: crate::slint_shell::sync::DEFAULT_EXPLORE_COLUMNS,
            worker_tx: None,
        }
    }

    /// Updates active UI theme and persists preference to disk settings store.
    pub fn set_theme(&mut self, theme: ThemeId) {
        self.theme = theme;
        let mut settings = self.settings_store.load();
        settings.theme = theme;
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
        let shelves = match self.catalog.lock() {
            Ok(c) => c.explore_shelves(),
            Err(_) => Vec::new(),
        };
        self.explore_shelves = shelves;
        self.explore_loaded = true;
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

    /// Submits search query to catalog and wraps hits in a shelf.
    pub fn submit_search(&mut self, query: &str) {
        self.search_query = query.to_string();
        self.search_filter = SearchFilter::All;
        self.search_covers.clear();
        let Some(parsed) = SearchQuery::parse(query) else {
            return;
        };
        let items = self.catalog.lock().map(|c| c.search_filtered(&parsed)).unwrap_or_default();
        self.search_results = items
            .iter()
            .filter_map(|it| match it {
                SearchResult::Track(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        let mut shelf = Shelf::new(tr(self.locale, "Search Results"));
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

    /// Returns from detail view to the previous page (defaults to Search).
    pub fn go_back(&mut self) {
        if self.page == SlintPage::Detail {
            self.page = self.detail_return;
        }
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

    fn cover_bytes_or_fetch(covers: &Arc<Mutex<K>>, url: &str) -> Option<Vec<u8>> {
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
                }
            }
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::{LocaleTrackingCatalog, test_wired};

    #[test]
    fn test_initial_state_defaults() {
        let state = test_wired();
        assert_eq!(state.page, SlintPage::Search);
        assert!(state.queue.is_empty());
        assert!(!state.home_shelves.is_empty());
        assert_eq!(state.active_lyric_index(), -1);
        assert!(state.liked_ids.is_empty());
        assert!(state.now_playing_cover.is_none());
        assert!(!state.is_shuffle);
        assert_eq!(state.search_query, "");
        assert_eq!(state.explore_columns, crate::slint_shell::sync::DEFAULT_EXPLORE_COLUMNS);
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
    fn test_search_submission() {
        let mut state = test_wired();
        state.submit_search("midnight");
        assert_eq!(state.search_query, "midnight");
        assert_eq!(state.home_shelves.len(), 1);
        assert_eq!(state.home_shelves[0].title, "Search Results");

        let mut state_es = test_wired();
        state_es.locale = Locale::Spanish;
        state_es.submit_search("midnight");
        assert_eq!(state_es.home_shelves.len(), 1);
        assert_eq!(state_es.home_shelves[0].title, "Resultados de búsqueda");
    }

    #[test]
    fn test_search_submission_empty_query_preserves_shelves() {
        let mut state = test_wired();
        let initial_shelves = state.home_shelves.clone();
        assert!(!initial_shelves.is_empty());
        state.submit_search("   ");
        assert_eq!(state.search_query, "   ");
        assert_eq!(state.home_shelves, initial_shelves);
    }

    #[test]
    fn test_search_query_explicit_clear() {
        let mut state = test_wired();
        state.submit_search("midnight");
        assert_eq!(state.search_query, "midnight");
        state.search_query.clear();
        assert_eq!(state.search_query, "");
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
        state.go_back();
        assert_eq!(state.page, SlintPage::Search);
    }

    #[test]
    fn test_detail_back_navigation_transitions_to_return_page() {
        let mut state = test_wired();
        assert_eq!(state.detail_return, SlintPage::Search);

        state.open_detail("album-1", DetailKind::Album);
        assert_eq!(state.page, SlintPage::Detail);
        assert_eq!(state.detail_return, SlintPage::Search);
        state.go_back();
        assert_eq!(state.page, SlintPage::Search);

        state.select_page(SlintPage::Library);
        state.open_detail("artist-1", DetailKind::Artist);
        assert_eq!(state.page, SlintPage::Detail);
        assert_eq!(state.detail_return, SlintPage::Library);
        state.go_back();
        assert_eq!(state.page, SlintPage::Library);

        state.go_back();
        assert_eq!(state.page, SlintPage::Library);
    }

    #[test]
    fn test_search_filter_lifecycle_and_reset() {
        let mut state = test_wired();
        assert_eq!(state.search_filter, SearchFilter::All);

        state.search_filter = SearchFilter::Albums;
        assert_eq!(state.search_filter, SearchFilter::Albums);

        state.submit_search("rock");
        assert_eq!(state.search_filter, SearchFilter::All);
        assert!(state.search_covers.is_empty());
    }

    #[test]
    fn test_set_theme_persists_to_settings_store() {
        let mut state = test_wired();
        assert_eq!(state.theme, ThemeId::White);

        state.set_theme(ThemeId::BlackOled);
        assert_eq!(state.theme, ThemeId::BlackOled);
        let loaded = state.settings_store.load();
        assert_eq!(loaded.theme, ThemeId::BlackOled);

        state.set_theme(ThemeId::Mono);
        assert_eq!(state.theme, ThemeId::Mono);
        let loaded2 = state.settings_store.load();
        assert_eq!(loaded2.theme, ThemeId::Mono);
    }

    #[test]
    fn test_set_player_bar_mode_persists_to_settings_store() {
        let mut state = test_wired();
        assert_eq!(state.player_bar.mode, PlayerBarMode::Classic);

        state.set_player_bar_mode(PlayerBarMode::Floating);
        assert_eq!(state.player_bar.mode, PlayerBarMode::Floating);
        let loaded = state.settings_store.load();
        assert_eq!(loaded.player_bar_mode, PlayerBarMode::Floating);

        state.set_player_bar_mode(PlayerBarMode::Classic);
        assert_eq!(state.player_bar.mode, PlayerBarMode::Classic);
        let loaded2 = state.settings_store.load();
        assert_eq!(loaded2.player_bar_mode, PlayerBarMode::Classic);
    }

    #[test]
    fn test_toggle_player_bar_option_persists_to_settings_store() {
        let mut state = test_wired();
        assert!(state.player_bar.show_artwork);
        assert!(!state.player_bar.show_skip_seconds);

        state.toggle_player_bar_option(0);
        assert!(!state.player_bar.show_artwork);
        let loaded = state.settings_store.load();
        assert!(!loaded.player_bar_show_artwork);

        state.toggle_player_bar_option(4);
        assert!(state.player_bar.show_skip_seconds);
        let loaded2 = state.settings_store.load();
        assert!(loaded2.player_bar_show_skip_seconds);
    }

    #[test]
    fn test_slint_app_state_new_fetches_home_shelves_with_default_locale() {
        let (catalog, locales, fetch_locales) = LocaleTrackingCatalog::new();
        let audio = crate::infra::AudioStub::new();
        let session = crate::infra::MemorySessionStore::new();
        let lyrics = crate::infra::LrclibStub::new();
        let covers = crate::infra::MemoryCoverCache::new();

        let state = SlintAppState::new(catalog, audio, session, lyrics, covers);

        assert_eq!(*locales.lock().unwrap(), vec![Locale::English]);
        assert_eq!(*fetch_locales.lock().unwrap(), vec![Locale::English]);
        assert_eq!(state.home_shelves.len(), 1);
        assert_eq!(state.home_shelves[0].title, "Home English");
    }

    #[test]
    fn test_cover_max_px_resolution() {
        let mut state = test_wired();
        state.hi_res_covers = false;
        assert_eq!(state.cover_max_px(), 512);
        state.hi_res_covers = true;
        assert_eq!(state.cover_max_px(), 1024);
    }

    #[test]
    fn test_cover_bytes_or_fetch_cache_hit() {
        let state = test_wired();
        state.covers.lock().unwrap().put("https://example.com/art.jpg", vec![1, 2, 3]);
        let cached = SlintAppState::<
            crate::infra::CatalogStub,
            crate::infra::AudioStub,
            crate::infra::MemorySessionStore,
            crate::infra::LrclibStub,
            crate::infra::MemoryCoverCache,
        >::cover_bytes_or_fetch(&state.covers, "https://example.com/art.jpg");
        assert_eq!(cached, Some(vec![1, 2, 3]));
    }
}
