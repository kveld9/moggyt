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
    Album, Detail, DetailKind, PlaybackQueue, PlayerBarMode, Playlist, RecentSearches,
    SearchFilter, SearchQuery, SearchResult, Shelf, SkipRanges, SleepTimer, ThemeId, Track,
    extract_playlist_id,
};
use crate::i18n::{Locale, tr};
use crate::infra::{
    AudioBackend, Catalog, CoverCache, DesktopPresence, DiscordPresence, LyricLine,
    LyricOverrideDir, LyricsSource, MediaKeyAction, QueueStore, ReleaseInfo, Scrobbler,
    SessionStore, SettingsStore, SkipStore, TrayAction, TrayHandle, decode_cover, decode_handoff,
    fetch_artwork, sanitize_cookie_input,
};
use crate::slint_shell::covers::request_queue_track_cover;
use crate::slint_shell::format::cycle_playback_speed_preset;
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::platform::rgba_to_image;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::player_bar::PlayerBarPrefs;

pub type SharedCatalog = Arc<Mutex<Box<dyn Catalog + Send>>>;
pub type SharedAudio = Arc<Mutex<Box<dyn AudioBackend + Send>>>;
pub type SharedLyrics = Arc<Box<dyn LyricsSource + Send + Sync>>;
pub type SharedCovers = Arc<Mutex<Box<dyn CoverCache + Send>>>;

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
    pub lyrics_lines: Vec<LyricLine>,
    pub lyrics_key: Option<(String, String)>,
    pub cover_key: Option<String>,
    pub current_cover_image: Option<slint::Image>,
    pub last_presence_key: Option<(String, String)>,
    pub started_at: Option<(Instant, u64)>,
    pub last_skip_target: Option<(String, u32)>,
    pub cached_skip_ranges: Option<(String, Option<SkipRanges>)>,
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
pub type SharedState = Rc<RefCell<SlintAppState>>;

impl SlintAppState {
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
    }

    /// Submits search query to catalog and wraps hits in a shelf.
    pub fn submit_search(&mut self, query: &str) {
        self.search_query = query.to_string();
        self.search_filter = SearchFilter::All;
        self.search_covers.clear();
        let Some(parsed) = SearchQuery::parse(query) else {
            return;
        };
        self.search_active = true;
        self.recent_searches.record(parsed.text());
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

impl<T: ?Sized + Catalog> Catalog for Box<T> {
    fn search(&self, q: &str) -> Vec<Track> {
        (**self).search(q)
    }
    fn search_filtered(&self, q: &SearchQuery) -> Vec<SearchResult> {
        (**self).search_filtered(q)
    }
    fn search_by_lyrics(&self, s: &crate::domain::LyricsSnippet) -> Vec<Track> {
        (**self).search_by_lyrics(s)
    }
    fn radio(&self, id: &str) -> Vec<Track> {
        (**self).radio(id)
    }
    fn home_shelves(&self) -> Vec<Shelf> {
        (**self).home_shelves()
    }
    fn explore_shelves(&self) -> Vec<Shelf> {
        (**self).explore_shelves()
    }
    fn playlists(&self) -> Vec<Playlist> {
        (**self).playlists()
    }
    fn saved_albums(&self) -> Vec<Album> {
        (**self).saved_albums()
    }
    fn detail(&self, id: &str, k: DetailKind) -> Option<Detail> {
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
    fn play(&mut self, id: &str) {
        (**self).play(id);
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
    fn last_requested(&self) -> Option<&str> {
        (**self).last_requested()
    }
    fn set_quality(&mut self, q: crate::infra::AudioQuality) {
        (**self).set_quality(q);
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
    use crate::slint_shell::test_support::test_wired;

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
        assert_eq!(state.page, SlintPage::Search);

        state.toggle_queue_panel();
        assert!(state.queue_panel_open);
        state.toggle_queue_panel();
        assert!(!state.queue_panel_open);

        assert_eq!(state.page, SlintPage::Search);

        state.close_queue_panel();
        assert!(!state.queue_panel_open);

        state.select_page(SlintPage::NowPlaying);
        assert_eq!(state.page, SlintPage::NowPlaying);
        state.toggle_queue_panel();
        assert!(state.queue_panel_open);
        assert_eq!(state.page, SlintPage::NowPlaying);

        state.close_queue_panel();
        assert!(!state.queue_panel_open);
        assert_eq!(state.page, SlintPage::NowPlaying);
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
        assert!(state.search_active);
        assert_eq!(state.recent_searches.entries(), &["midnight"]);
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
        assert!(initial_shelves.is_empty());
        state.submit_search("   ");
        assert_eq!(state.search_query, "   ");
        assert!(!state.search_active);
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
        let cached =
            SlintAppState::cover_bytes_or_fetch(&state.covers, "https://example.com/art.jpg");
        assert_eq!(cached, Some(vec![1, 2, 3]));
    }
}
