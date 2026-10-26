//! UI shell: navigation state and event delegation.
//!
//! The shell holds no business rules; every user intent is forwarded to
//! [`crate::domain`] and every side effect goes through injected adapters.

pub mod components;
pub mod pages;
pub mod theme;

use components::{render_player_bar, render_top_bar};
use pages::{render_now_playing, render_queue, render_search};

use eframe::egui;

use crate::domain::{
    Detail, DetailKind, PlaybackQueue, SearchQuery, SearchResult, Shelf, ThemeId, Track,
};
use crate::i18n::{Locale, tr};
use crate::infra::{
    AudioBackend, AudioQuality, COVER_HIRES_MAX_PX, COVER_MAX_PX, Catalog, CoverCache,
    CredentialStore, DesktopPresence, DiscordPresence, GithubUpdater, LastFmClient,
    LastFmCredentials, LyricLine, LyricsSource, MediaKeyAction, MemoryCredentialStore,
    MemorySettingsStore, Presence, Scrobbler, SessionStore, Settings, SettingsStore, StubScrobbler,
    TrayAction, TrayHandle, Updater, client_from_credentials, decode_cover, fetch_artwork,
    is_newer, sanitize_cookie_input, should_scrobble,
};

/// Navigation pages of the application shell.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Search the catalog.
    #[default]
    Search,
    /// Inspect and reorder the playback queue.
    Queue,
    /// Currently playing view.
    NowPlaying,
    /// Library view (playlists, liked tracks, saved albums).
    Library,
    /// Detailed view for an album, artist, or playlist.
    Detail,
}

/// Update check status for the desktop application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    /// Never checked in this session.
    Unknown,
    /// Installed version is up to date with upstream release.
    Current,
    /// A newer upstream release is available for download.
    Available {
        /// Upstream release version tag.
        version: String,
        /// Upstream release webpage URL.
        url: String,
    },
    /// Last check failed (e.g. offline, rate limited, timeout).
    CheckFailed,
}

/// Wiring between the UI shell and its collaborators.
pub struct AppState<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache> {
    /// Active navigation page.
    pub page: Page,
    /// Pure playback queue (no I/O).
    pub queue: PlaybackQueue,
    /// Text typed into the search box (UI state only).
    pub search_text: String,
    /// Text typed into the sign-in cookie box (UI state only).
    pub cookie_text: String,
    /// Catalog adapter (injected port).
    pub catalog: C,
    /// Audio adapter (injected port).
    pub audio: A,
    /// Session store adapter (injected port).
    pub session: S,
    /// Cached sign-in status to avoid per-frame keyring I/O.
    ///
    /// Cache coherence contract: initialized once from `session.load()` at startup
    /// and kept in sync on `sign_in_from_input()` and `sign_out()`. Out-of-band external
    /// mutations to the underlying keyring session store are not observed.
    pub signed_in: bool,
    pub lyrics: L,
    pub covers: K,
    pub tray: Option<TrayHandle>,
    pub presence: DesktopPresence,
    pub discord: DiscordPresence,
    pub last_presence_key: Option<(String, String)>,
    pub lyrics_key: Option<(String, String)>,
    pub lyrics_lines: Vec<LyricLine>,
    pub lyrics_offset_ms: i64,
    /// Artwork URL for the currently rendered cover art texture.
    pub cover_key: Option<String>,
    /// Single-entry texture cache for the currently active cover art.
    pub cover_texture: Option<(String, egui::TextureHandle)>,
    pub update_status: UpdateStatus,
    pub updater: GithubUpdater,
    /// Last.fm scrobbler port for now-playing notifications and scrobbles.
    pub scrobbler: Box<dyn Scrobbler>,
    /// Playback start monotonic instant and unix timestamp pair.
    pub started_at: Option<(std::time::Instant, u64)>,
    /// Active UI theme.
    pub theme: ThemeId,
    /// Active interface locale.
    pub locale: Locale,
    /// Preferred audio streaming quality.
    pub quality: AudioQuality,
    /// Injected settings storage adapter.
    pub settings_store: Box<dyn SettingsStore>,
    /// Whether radio autoplay queue continuation is active.
    pub autoplay: bool,
    /// Cached home shelves for the Search view.
    pub home_shelves: Vec<Shelf>,
    /// Active detail container view.
    pub detail: Option<Detail>,
    /// Page to restore when closing detail view.
    pub detail_return: Page,
    /// Whether any single-line text input field currently has focus.
    pub text_focused: bool,
    /// Whether global keyboard shortcuts are enabled.
    pub shortcuts_enabled: bool,
    /// Port for persisting and retrieving Last.fm credentials.
    pub credentials: Box<dyn CredentialStore>,
    /// Whether Last.fm live scrobbling is enabled.
    pub lastfm_enabled: bool,
    /// Whether high-resolution cover artwork decoding is enabled.
    pub hi_res_covers: bool,
    /// Last.fm API key input buffer (UI state only).
    pub lastfm_api_key_text: String,
    /// Last.fm API secret input buffer (UI state only).
    pub lastfm_secret_text: String,
    /// Last.fm session key input buffer (UI state only).
    pub lastfm_session_key_text: String,
}

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Creates the shell with injected collaborators.
    pub fn new(mut catalog: C, audio: A, session: S, lyrics: L, covers: K) -> Self {
        let stored_session = session.load();
        let signed_in = stored_session.is_some();
        if let Some(ref cookies) = stored_session {
            catalog.set_session_cookies(Some(cookies));
        }
        Self {
            page: Page::default(),
            queue: PlaybackQueue::new(),
            search_text: String::new(),
            cookie_text: String::new(),
            catalog,
            audio,
            signed_in,
            session,
            lyrics,
            covers,
            tray: None,
            presence: DesktopPresence::new(),
            discord: DiscordPresence::new(),
            last_presence_key: None,
            lyrics_key: None,
            lyrics_lines: Vec::new(),
            lyrics_offset_ms: 0,
            cover_key: None,
            cover_texture: None,
            update_status: UpdateStatus::Unknown,
            updater: GithubUpdater::new(),
            scrobbler: Box::new(StubScrobbler::new()),
            credentials: Box::new(MemoryCredentialStore::new()),
            started_at: None,
            theme: ThemeId::White,
            locale: Locale::English,
            quality: AudioQuality::Standard,
            settings_store: Box::new(MemorySettingsStore::default()),
            autoplay: false,
            home_shelves: Vec::new(),
            detail: None,
            detail_return: Page::Search,
            text_focused: false,
            shortcuts_enabled: true,
            lastfm_enabled: false,
            hi_res_covers: false,
            lastfm_api_key_text: String::new(),
            lastfm_secret_text: String::new(),
            lastfm_session_key_text: String::new(),
        }
    }

    /// Injects an alternative scrobbler implementation.
    pub fn set_scrobbler(&mut self, scrobbler: Box<dyn Scrobbler>) {
        self.scrobbler = scrobbler;
    }

    /// Injects an alternative Last.fm credential store implementation.
    pub fn set_credential_store(&mut self, store: Box<dyn CredentialStore>) {
        self.credentials = store;
        self.sync_lastfm_scrobbler();
    }

    /// Injects an alternative settings store implementation and synchronizes state.
    pub fn set_settings_store(&mut self, store: Box<dyn SettingsStore>) {
        let loaded = store.load();
        self.theme = loaded.theme;
        self.locale = loaded.locale;
        self.quality = loaded.quality;
        self.shortcuts_enabled = loaded.keyboard_shortcuts;
        self.lastfm_enabled = loaded.lastfm_enabled;
        self.hi_res_covers = loaded.hi_res_covers;
        self.audio.set_volume(loaded.volume);
        self.audio.set_quality(loaded.quality);
        self.settings_store = store;
        self.sync_lastfm_scrobbler();
    }

    /// Sets whether Last.fm live scrobbling is enabled and persists the preference.
    ///
    /// Disabling scrobbling switches to a dormant client immediately to prevent
    /// outgoing network requests, but preserves stored credentials in the vault
    /// unless explicitly cleared.
    pub fn set_lastfm_enabled(&mut self, enabled: bool) {
        self.lastfm_enabled = enabled;
        self.persist_settings();
        self.sync_lastfm_scrobbler();
    }

    fn sync_lastfm_scrobbler(&mut self) {
        if !self.lastfm_enabled {
            self.scrobbler = Box::new(LastFmClient::new("", ""));
            return;
        }
        let Some(creds) = self.credentials.load_lastfm() else {
            self.scrobbler = Box::new(LastFmClient::new("", ""));
            return;
        };
        if !creds.is_complete() {
            self.scrobbler = Box::new(LastFmClient::new("", ""));
            return;
        }
        self.scrobbler = Box::new(client_from_credentials(&creds));
    }

    /// Saves Last.fm credentials to the vault and synchronizes scrobbler state.
    pub fn save_lastfm_credentials(&mut self, creds: &LastFmCredentials) {
        self.credentials.save_lastfm(creds);
        self.sync_lastfm_scrobbler();
    }

    /// Clears persisted Last.fm credentials from the vault, input buffers, and resets scrobbler to dormant.
    pub fn clear_lastfm_credentials(&mut self) {
        self.credentials.clear_lastfm();
        self.lastfm_api_key_text.clear();
        self.lastfm_secret_text.clear();
        self.lastfm_session_key_text.clear();
        self.sync_lastfm_scrobbler();
    }

    /// Saves credentials from UI input buffers and clears buffers on completion.
    pub fn save_lastfm_credentials_from_input(&mut self) -> bool {
        let creds = LastFmCredentials::new(
            &self.lastfm_api_key_text,
            &self.lastfm_secret_text,
            &self.lastfm_session_key_text,
        );
        let complete = creds.is_complete();
        self.save_lastfm_credentials(&creds);
        self.lastfm_api_key_text.clear();
        self.lastfm_secret_text.clear();
        self.lastfm_session_key_text.clear();
        complete
    }

    /// Returns true if the active scrobbler has a live authenticated session.
    pub fn lastfm_is_live(&self) -> bool {
        self.scrobbler.as_any().downcast_ref::<LastFmClient>().is_some_and(LastFmClient::is_live)
    }

    /// Sets whether high-resolution cover artwork is enabled and persists the preference.
    pub fn set_hi_res_covers(&mut self, enabled: bool) {
        if self.hi_res_covers == enabled {
            return;
        }
        self.hi_res_covers = enabled;
        self.clear_cover_texture();
        self.persist_settings();
    }

    /// Resolves the maximum pixel bound for decoded cover art based on current settings.
    pub fn cover_max_px(&self) -> u32 {
        if self.hi_res_covers { COVER_HIRES_MAX_PX } else { COVER_MAX_PX }
    }

    /// Decodes raw cover art bytes into a color image bounded by the active setting.
    pub fn decode_cover_image(&self, bytes: &[u8]) -> Option<egui::ColorImage> {
        decode_cover(bytes, self.cover_max_px())
    }

    /// Sets the active UI theme and persists the change.
    pub fn set_theme(&mut self, theme: ThemeId) {
        self.theme = theme;
        self.persist_settings();
    }

    /// Sets the active interface locale and persists the change.
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
        self.persist_settings();
    }

    /// Sets the preferred audio streaming quality, forwards to audio backend, and persists the change.
    pub fn set_quality(&mut self, quality: AudioQuality) {
        self.quality = quality;
        self.audio.set_quality(quality);
        self.persist_settings();
    }

    /// Sets whether keyboard shortcuts are enabled and persists the preference.
    pub fn set_shortcuts_enabled(&mut self, enabled: bool) {
        self.shortcuts_enabled = enabled;
        self.persist_settings();
    }

    /// Returns whether keyboard shortcuts are enabled.
    pub fn keyboard_shortcuts_enabled(&self) -> bool {
        self.shortcuts_enabled
    }

    fn persist_settings(&self) {
        let settings = Settings {
            theme: self.theme,
            locale: self.locale,
            volume: self.audio.volume(),
            quality: self.quality,
            keyboard_shortcuts: self.shortcuts_enabled,
            lastfm_enabled: self.lastfm_enabled,
            hi_res_covers: self.hi_res_covers,
        };
        if let Err(err) = self.settings_store.save(&settings) {
            tracing::warn!("failed to persist settings: {err:#}");
        }
    }

    /// Returns the current update check status.
    pub fn update_status(&self) -> &UpdateStatus {
        &self.update_status
    }

    /// Checks for updates against an injected updater implementation.
    pub fn check_with(&mut self, updater: &impl Updater) {
        let Some(info) = updater.latest() else {
            self.update_status = UpdateStatus::CheckFailed;
            return;
        };
        let current = GithubUpdater::current_version();
        if is_newer(current, &info.version) {
            self.update_status = UpdateStatus::Available { version: info.version, url: info.url };
        } else {
            self.update_status = UpdateStatus::Current;
        }
    }

    /// Initiates a manual update check using the configured updater.
    pub fn refresh_update(&mut self) {
        let updater = self.updater.clone();
        self.check_with(&updater);
    }

    /// Dismisses the update banner, treating the app as up-to-date for this session.
    pub fn dismiss_update(&mut self) {
        self.update_status = UpdateStatus::Current;
    }

    /// Attaches an initialized system tray handle.
    pub fn set_tray(&mut self, handle: TrayHandle) {
        self.tray = Some(handle);
    }

    /// Attaches an initialized desktop media presence handle.
    pub fn set_presence(&mut self, presence: DesktopPresence) {
        self.presence = presence;
    }

    /// Attaches an initialized Discord Rich Presence handle.
    pub fn set_discord(&mut self, discord: DiscordPresence) {
        self.discord = discord;
    }

    /// Dispatches a single tray action, returning true if quit was requested.
    fn handle_action(&mut self, action: TrayAction) -> bool {
        match action {
            TrayAction::TogglePlay => self.toggle_play(),
            TrayAction::Next => self.next_track(),
            TrayAction::Previous => self.prev_track(),
            TrayAction::Show => {}
            TrayAction::Quit => return true,
        }
        false
    }

    /// Applies a batch of tray actions to application state.
    pub fn apply_actions(&mut self, actions: &[TrayAction]) -> bool {
        let mut quit_requested = false;
        for action in actions {
            if self.handle_action(*action) {
                quit_requested = true;
            }
        }
        quit_requested
    }

    /// Drains pending system tray events and applies them to state.
    ///
    /// Returns true when a Quit action was received.
    pub fn drain_tray(&mut self) -> bool {
        let Some(tray) = &self.tray else {
            return false;
        };
        let actions = tray.drain();
        self.apply_actions(&actions)
    }

    /// Dispatches a single media key action to application state.
    fn handle_media_key(&mut self, action: MediaKeyAction) {
        match action {
            MediaKeyAction::Toggle => self.toggle_play(),
            MediaKeyAction::Next => self.next_track(),
            MediaKeyAction::Previous => self.prev_track(),
            MediaKeyAction::Stop => self.stop_playback(),
        }
    }

    fn stop_playback(&mut self) {
        if self.audio.is_playing() {
            self.audio.pause();
        }
    }

    /// Applies a batch of media key actions to application state.
    fn apply_media_keys(&mut self, actions: &[MediaKeyAction]) {
        for action in actions {
            self.handle_media_key(*action);
        }
    }

    /// Drains pending inbound media key actions from desktop presence and applies them.
    fn drain_media_keys(&mut self) {
        let actions = self.presence.drain_actions();
        self.apply_media_keys(&actions);
    }

    /// Synchronizes current playback state and track metadata with presence adapters.
    ///
    /// Drains inbound media key actions first, then fans out active track metadata or queue
    /// clearance to both desktop media presence (MPRIS on Linux, SMTC on Windows) and Discord Rich Presence.
    fn sync_presence(&mut self) {
        self.drain_media_keys();
        let is_playing = self.audio.is_playing();
        let track_info = self.queue.current().map(|t| (t.artist.clone(), t.title.clone()));
        match track_info {
            Some((artist, title)) => self.sync_active_track(&artist, &title, is_playing),
            None => self.sync_empty_track(),
        }
    }

    fn sync_active_track(&mut self, artist: &str, title: &str, is_playing: bool) {
        let key = (artist.to_string(), title.to_string());
        if self.last_presence_key.as_ref() != Some(&key) {
            self.presence.set_track(artist, title);
            self.discord.set_track(artist, title);
            self.last_presence_key = Some(key);
        }
        self.presence.set_playing(is_playing);
    }

    fn sync_empty_track(&mut self) {
        if self.last_presence_key.is_some() || self.presence.last_published().is_some() {
            self.presence.clear();
            self.discord.clear();
            self.last_presence_key = None;
        }
        self.presence.set_playing(false);
    }

    /// Fetches home shelves from the catalog if not already populated.
    pub fn ensure_home(&mut self) {
        if self.home_shelves.is_empty() {
            self.home_shelves = self.catalog.home_shelves();
        }
    }

    /// Clears and reloads home shelves from the catalog.
    pub fn reload_home(&mut self) {
        self.home_shelves.clear();
        self.ensure_home();
    }

    /// Opens the detail view for `browse_id` and `kind`. Blank identifiers are ignored.
    pub fn open_detail(&mut self, browse_id: &str, kind: DetailKind) {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.page != Page::Detail {
            self.detail_return = self.page;
        }
        self.detail = self.catalog.detail(trimmed, kind);
        self.page = Page::Detail;
    }

    /// Closes the detail view and returns to the previous page.
    pub fn close_detail(&mut self) {
        self.page = self.detail_return;
        self.detail = None;
    }

    /// Handles a search submission: parses the raw input at the boundary,
    /// queries the catalog, and enqueues the first hit.
    pub fn submit_search(&mut self) {
        let Some(query) = SearchQuery::parse(&self.search_text) else {
            return;
        };
        let hits = self.catalog.search(query.text());
        if let Some(track) = hits.into_iter().next() {
            self.queue.push(track);
            self.page = Page::Queue;
        }
    }

    /// Starts playback of the track at the head of the queue.
    pub fn play_head(&mut self) {
        if self.queue.current().is_some() {
            self.play_current_quiet();
            self.page = Page::NowPlaying;
        }
    }

    /// Seeds an autoplay radio session with `seed`, clears queue, and starts playback.
    /// Blank seed identifiers are ignored.
    pub fn start_radio(&mut self, seed: Track) {
        let seed_id = seed.id.trim();
        if seed_id.is_empty() {
            return;
        }
        let radio_tracks = self.catalog.radio(seed_id);
        self.queue.clear();
        let seed_key = seed.id.clone();
        self.queue.push(seed);
        for track in radio_tracks {
            let trimmed = track.id.trim();
            if !trimmed.is_empty() && trimmed != seed_key {
                self.queue.push(track);
            }
        }
        self.autoplay = true;
        self.play_current_quiet();
        self.page = Page::NowPlaying;
    }

    /// Evaluates autoplay queue continuation when the active track ends.
    pub fn continue_autoplay_if_ended(&mut self) {
        if !self.autoplay {
            return;
        }
        if self.queue.is_empty() {
            return;
        }
        let (Some(current), Some(last)) = (self.queue.current(), self.queue.tracks().last()) else {
            return;
        };
        if !std::ptr::eq(current, last) {
            return;
        }
        if self.audio.is_playing() {
            return;
        }
        if self.audio.last_requested().is_none() {
            return;
        }
        let Some(duration) = current.duration_secs else {
            return;
        };
        if self.audio.position_secs() < duration as u64 {
            return;
        }
        let current_id = current.id.clone();
        self.append_autoplay_and_advance(&current_id);
    }

    fn append_autoplay_and_advance(&mut self, current_id: &str) {
        let recommendations = self.catalog.radio(current_id);
        let mut appended = false;
        for track in recommendations {
            let trimmed = track.id.trim();
            if trimmed.is_empty() || self.queue.tracks().iter().any(|t| t.id == trimmed) {
                continue;
            }
            self.queue.push(track);
            appended = true;
        }
        if appended {
            self.queue.advance();
            self.play_current_quiet();
        }
    }

    /// Plays the track currently under the queue cursor without modifying active page.
    pub fn play_current_quiet(&mut self) {
        let Some(track) = self.queue.current() else {
            return;
        };
        let id = track.id.clone();
        let artist = track.artist.clone();
        let title = track.title.clone();
        self.audio.play(&id);
        self.started_at = Some((std::time::Instant::now(), current_unix_secs()));
        self.scrobbler.now_playing(&artist, &title);
    }

    /// Toggles playback: pauses if currently playing, resumes if previously requested,
    /// or starts playback of current head quietly.
    pub fn toggle_play(&mut self) {
        if self.audio.is_playing() {
            self.audio.pause();
        } else if self.audio.last_requested().is_some() {
            self.audio.resume_play();
        } else {
            self.play_current_quiet();
        }
    }

    fn maybe_scrobble_current(&mut self) {
        let elapsed = self.started_at.map_or(0, |(inst, _)| inst.elapsed().as_secs());
        self.maybe_scrobble_with_elapsed(elapsed);
    }

    pub(crate) fn maybe_scrobble_with_elapsed(&mut self, elapsed_secs: u64) {
        let Some(track) = self.queue.current() else {
            return;
        };
        let duration = track.duration_secs.map(|d| d as u64);
        if !should_scrobble(elapsed_secs, duration) {
            return;
        }
        let timestamp = self.started_at.map_or(0, |(_, ts)| ts);
        let artist = track.artist.clone();
        let title = track.title.clone();
        self.scrobbler.scrobble(&artist, &title, timestamp);
    }

    /// Advances the queue cursor and deterministically starts playback of the new head.
    pub fn next_track(&mut self) {
        self.maybe_scrobble_current();
        self.queue.advance();
        self.play_current_quiet();
    }

    /// Retreats the queue cursor and deterministically starts playback of the new head.
    pub fn prev_track(&mut self) {
        self.maybe_scrobble_current();
        self.queue.retreat();
        self.play_current_quiet();
    }

    /// Adjusts playback position by `delta_secs` relative to current position, clamped at zero.
    pub fn seek_by(&mut self, delta_secs: i64) {
        let current = self.audio.position_secs() as i64;
        let target = current.saturating_add(delta_secs).max(0) as u64;
        self.audio.seek(target);
    }

    /// Sets playback volume level (0..=100, clamped by audio adapter) and persists the change.
    pub fn set_volume(&mut self, volume: u8) {
        self.audio.set_volume(volume);
        self.persist_settings();
    }

    /// Returns true if an active session is currently stored.
    pub fn is_signed_in(&self) -> bool {
        self.signed_in
    }

    /// Attempts to parse and save the current cookie input.
    ///
    /// Returns true and clears the buffer if valid, otherwise returns false.
    pub fn sign_in_from_input(&mut self) -> bool {
        let Some(cleaned) = sanitize_cookie_input(&self.cookie_text) else {
            return false;
        };
        self.session.save(&cleaned);
        self.catalog.set_session_cookies(Some(&cleaned));
        self.signed_in = true;
        self.cookie_text.clear();
        true
    }

    /// Clears the stored session and resets the cookie input buffer.
    pub fn sign_out(&mut self) {
        self.session.clear();
        self.catalog.set_session_cookies(None);
        self.signed_in = false;
        self.cookie_text.clear();
    }

    /// Returns true if the track id is present in the user's liked collection.
    pub fn is_liked(&self, track_id: &str) -> bool {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            return false;
        }
        self.catalog.liked_tracks().iter().any(|track| track.id == trimmed)
    }

    /// Toggles the liked status of a track in the catalog. Blank identifiers are ignored.
    pub fn toggle_like(&mut self, track_id: &str) {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            return;
        }
        let next_liked = !self.is_liked(trimmed);
        self.catalog.set_liked(trimmed, next_liked);
    }

    /// Handles a global keyboard shortcut event.
    ///
    /// Returns true if the key triggered an action, false otherwise.
    pub fn apply_key(&mut self, key: egui::Key) -> bool {
        if !self.keyboard_shortcuts_enabled() {
            return false;
        }
        self.apply_playback_shortcut(key)
            || self.apply_volume_shortcut(key)
            || self.apply_queue_shortcut(key)
    }

    fn apply_playback_shortcut(&mut self, key: egui::Key) -> bool {
        match key {
            egui::Key::Space => {
                self.toggle_play();
                true
            }
            egui::Key::ArrowLeft => {
                self.seek_by(-10);
                true
            }
            egui::Key::ArrowRight => {
                self.seek_by(10);
                true
            }
            _ => false,
        }
    }

    fn apply_volume_shortcut(&mut self, key: egui::Key) -> bool {
        match key {
            egui::Key::ArrowUp => {
                let vol = self.audio.volume().saturating_add(5).min(100);
                self.set_volume(vol);
                true
            }
            egui::Key::ArrowDown => {
                let vol = self.audio.volume().saturating_sub(5);
                self.set_volume(vol);
                true
            }
            _ => false,
        }
    }

    fn apply_queue_shortcut(&mut self, key: egui::Key) -> bool {
        match key {
            egui::Key::N => self.step_next_shortcut(),
            egui::Key::P => self.step_prev_shortcut(),
            egui::Key::L => self.toggle_like_shortcut(),
            _ => false,
        }
    }

    fn step_next_shortcut(&mut self) -> bool {
        if self.queue.is_empty() || self.queue.current().is_none() {
            return false;
        }
        self.next_track();
        true
    }

    fn step_prev_shortcut(&mut self) -> bool {
        if self.queue.is_empty() || self.queue.current().is_none() {
            return false;
        }
        self.prev_track();
        true
    }

    fn toggle_like_shortcut(&mut self) -> bool {
        let Some(track) = self.queue.current() else {
            return false;
        };
        let id = track.id.clone();
        self.toggle_like(&id);
        true
    }

    /// Fetches and caches synced lyrics for the currently playing track.
    ///
    /// No-op if lyrics for the current (artist, title) pair have already been resolved.
    pub fn refresh_lyrics(&mut self) {
        let Some(track) = self.queue.current() else {
            if self.lyrics_key.is_some() {
                self.lyrics_key = None;
                self.lyrics_lines.clear();
            }
            return;
        };
        let key = (track.artist.clone(), track.title.clone());
        if self.lyrics_key.as_ref() == Some(&key) {
            return;
        }
        let lines = self.lyrics.lookup(&key.0, &key.1).map(|s| s.lines).unwrap_or_default();
        self.lyrics_lines = lines;
        self.lyrics_key = Some(key);
    }

    /// Adjusts the user lyrics timing offset by `delta_ms` with saturating arithmetic.
    pub fn nudge_lyrics_offset(&mut self, delta_ms: i64) {
        self.lyrics_offset_ms = self.lyrics_offset_ms.saturating_add(delta_ms);
    }

    /// Computes user-visible lyrics timestamps by applying current offset clamped at zero.
    pub fn visible_lyric_lines(&self) -> Vec<(u64, String)> {
        self.lyrics_lines
            .iter()
            .map(|line| {
                let shifted = (line.start_ms as i64).saturating_add(self.lyrics_offset_ms);
                let visible_ms = shifted.max(0) as u64;
                (visible_ms, line.text.clone())
            })
            .collect()
    }

    pub(crate) fn clear_cover_texture(&mut self) {
        self.cover_key = None;
        self.cover_texture = None;
    }

    fn current_artwork_url(&self) -> Option<String> {
        self.queue.current()?.artwork.clone()
    }

    fn load_cover_bytes(&mut self, url: &str) -> Option<Vec<u8>> {
        if let Some(cached) = self.covers.get(url) {
            return Some(cached);
        }
        match fetch_artwork(url, 8) {
            Ok(fetched) => {
                self.covers.put(url, fetched.clone());
                Some(fetched)
            }
            Err(err) => {
                tracing::warn!(url = %url, error = %err, "failed to fetch cover art");
                None
            }
        }
    }

    pub(crate) fn resolve_cover_bytes(&mut self) -> Option<(String, Vec<u8>)> {
        let Some(url) = self.current_artwork_url() else {
            self.clear_cover_texture();
            return None;
        };
        if self.cover_key.as_deref() == Some(&url) {
            return None;
        }
        let Some(bytes) = self.load_cover_bytes(&url) else {
            self.clear_cover_texture();
            return None;
        };
        Some((url, bytes))
    }

    fn store_cover_texture(&mut self, ctx: &egui::Context, url: String, bytes: &[u8]) {
        let Some(img) = self.decode_cover_image(bytes) else {
            self.clear_cover_texture();
            return;
        };
        let texture = ctx.load_texture("moggyt-cover", img, egui::TextureOptions::LINEAR);
        self.cover_key = Some(url.clone());
        self.cover_texture = Some((url, texture));
    }

    /// Fetches, caches, decodes, and loads cover art for the currently playing track.
    pub fn refresh_covers(&mut self, ctx: &egui::Context) {
        if let Some((url, bytes)) = self.resolve_cover_bytes() {
            self.store_cover_texture(ctx, url, &bytes);
        }
    }
}

/// Formats a millisecond duration as zero-padded "m:ss" (e.g. 65000 -> "1:05").
pub fn format_lyric_timestamp(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins}:{secs:02}")
}

/// Returns current UTC timestamp in seconds since UNIX epoch, or 0 on clock failure.
fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// eframe application wrapper (thin view over [`AppState`]).
pub struct MoggytApp<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache> {
    state: AppState<C, A, S, L, K>,
}

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    MoggytApp<C, A, S, L, K>
{
    /// Wraps an already-wired state for eframe.
    pub fn new(state: AppState<C, A, S, L, K>) -> Self {
        Self { state }
    }
}

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache> eframe::App
    for MoggytApp<C, A, S, L, K>
{
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.state.drain_tray() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.state.sync_presence();
        self.state.continue_autoplay_if_ended();
        if !self.state.text_focused {
            for key in [
                egui::Key::Space,
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
                egui::Key::N,
                egui::Key::P,
                egui::Key::L,
            ] {
                if ui.input(|i| i.key_pressed(key)) {
                    self.state.apply_key(key);
                    break;
                }
            }
        }
        render_top_bar(ui, &mut self.state);
        render_player_bar(ui, &mut self.state);
        render_body(ui, &mut self.state);
    }
}

fn render_body<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    egui::CentralPanel::default().show(ui, |ui| match state.page {
        Page::Search => render_search(ui, state),
        Page::Queue => render_queue(ui, state),
        Page::NowPlaying => render_now_playing(ui, state),
        Page::Library => render_library(ui, state),
        Page::Detail => render_detail(ui, state),
    });
}

fn render_library<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    render_library_preferences(ui, state);
    ui.separator();
    render_library_lastfm(ui, state);
    ui.separator();
    render_library_sign_in(ui, state);
    ui.separator();
    render_library_updates(ui, state);
    ui.separator();
    render_library_playlists(ui, state);
    ui.separator();
    render_library_liked(ui, state);
    ui.separator();
    render_library_albums(ui, state);
}

fn render_library_preferences<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading(tr(state.locale, "Theme"));
    ui.horizontal(|ui| {
        render_theme_button(ui, state, ThemeId::White, "White");
        render_theme_button(ui, state, ThemeId::BlackOled, "Black OLED");
        render_theme_button(ui, state, ThemeId::Mono, "Mono");
    });
    ui.add_space(4.0);
    ui.heading(tr(state.locale, "Language"));
    ui.horizontal(|ui| {
        render_locale_button(ui, state, Locale::English, "English");
        render_locale_button(ui, state, Locale::Spanish, "Spanish");
    });
    ui.add_space(4.0);
    let mut shortcuts = state.shortcuts_enabled;
    if ui.checkbox(&mut shortcuts, "Keyboard shortcuts").changed() {
        state.set_shortcuts_enabled(shortcuts);
    }
    let mut hi_res = state.hi_res_covers;
    if ui.checkbox(&mut hi_res, "High-resolution covers").changed() {
        state.set_hi_res_covers(hi_res);
    }
}

fn render_library_lastfm<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading("Last.fm");
    let mut enabled = state.lastfm_enabled;
    if ui.checkbox(&mut enabled, "Enable scrobbling").changed() {
        state.set_lastfm_enabled(enabled);
    }
    let status = if state.lastfm_is_live() { "Scrobbler: Live" } else { "Scrobbler: Dormant" };
    ui.label(status);
    render_lastfm_inputs(ui, state);
}

fn render_lastfm_inputs<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    render_lastfm_field(ui, &mut state.lastfm_api_key_text, &mut state.text_focused, "API Key");
    render_lastfm_field(ui, &mut state.lastfm_secret_text, &mut state.text_focused, "Secret");
    render_lastfm_field(
        ui,
        &mut state.lastfm_session_key_text,
        &mut state.text_focused,
        "Session Key",
    );
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            state.save_lastfm_credentials_from_input();
        }
        if ui.button("Clear").clicked() {
            state.clear_lastfm_credentials();
        }
    });
}

fn render_lastfm_field(ui: &mut egui::Ui, text: &mut String, focused: &mut bool, label: &str) {
    ui.horizontal(|ui| {
        ui.label(format!("{label}:"));
        let response = ui.add(egui::TextEdit::singleline(text).password(true));
        if response.has_focus() {
            *focused = true;
        }
    });
}

fn render_theme_button<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
    theme: ThemeId,
    key: &str,
) {
    let label = tr(state.locale, key);
    let is_selected = state.theme == theme;
    if ui.selectable_label(is_selected, label).clicked() && !is_selected {
        state.set_theme(theme);
        theme::apply(ui.ctx(), &theme.palette());
    }
}

fn render_locale_button<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
    locale: Locale,
    key: &str,
) {
    let label = tr(state.locale, key);
    let is_selected = state.locale == locale;
    if ui.selectable_label(is_selected, label).clicked() && !is_selected {
        state.set_locale(locale);
    }
}

fn render_library_updates<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading("Updates");
    if render_update_status_body(ui, &state.update_status) {
        state.dismiss_update();
    }
    if ui.button("Check for updates").clicked() {
        state.refresh_update();
    }
}

fn render_update_status_body(ui: &mut egui::Ui, status: &UpdateStatus) -> bool {
    match status {
        UpdateStatus::Unknown => {
            ui.label("Never checked");
        }
        UpdateStatus::Current => {
            ui.label(format!("moggyt is up to date (v{})", GithubUpdater::current_version()));
        }
        UpdateStatus::Available { version, url } => {
            return render_update_available(ui, version, url);
        }
        UpdateStatus::CheckFailed => {
            ui.label("Update check failed (offline?)");
        }
    }
    false
}

fn render_update_available(ui: &mut egui::Ui, version: &str, url: &str) -> bool {
    ui.label(format!("Update available: {version}"));
    let mut dismissed = false;
    ui.horizontal(|ui| {
        ui.hyperlink_to("Download page", url);
        if ui.button("Dismiss").clicked() {
            dismissed = true;
        }
    });
    dismissed
}

fn render_library_sign_in<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    let status = if state.is_signed_in() {
        "Signed in — library unlocked"
    } else {
        "Anonymous — paste cookies to unlock library"
    };
    ui.label(status);
    ui.horizontal(|ui| {
        let response = ui.text_edit_singleline(&mut state.cookie_text);
        state.text_focused = response.has_focus();
        if ui.button("Save").clicked() {
            state.sign_in_from_input();
        }
        if ui.button("Clear").clicked() {
            state.sign_out();
        }
    });
}

fn render_library_playlists<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S, L, K>,
) {
    ui.heading("Playlists");
    let playlists = state.catalog.playlists();
    if playlists.is_empty() {
        ui.label("No playlists");
        return;
    }
    for playlist in playlists {
        ui.label(format!("{} ({} tracks)", playlist.title, playlist.track_count));
    }
}

fn render_library_liked<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading("Liked songs");
    let liked = state.catalog.liked_tracks();
    if liked.is_empty() {
        ui.label("No liked tracks");
        return;
    }
    let mut to_toggle = None;
    for track in &liked {
        ui.horizontal(|ui| {
            ui.label(track.display());
            if ui.button("Unlike").clicked() {
                to_toggle = Some(track.id.clone());
            }
        });
    }
    if let Some(id) = to_toggle {
        state.toggle_like(&id);
    }
}

fn render_library_albums<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S, L, K>,
) {
    ui.heading("Saved albums");
    let albums = state.catalog.saved_albums();
    if albums.is_empty() {
        ui.label("No saved albums");
        return;
    }
    for album in albums {
        ui.label(format!("{} - {}", album.title, album.artist));
    }
}

fn shelf_item_display(item: &SearchResult) -> String {
    match item {
        SearchResult::Track(track) => format!("{} - {}", track.title, track.artist),
        SearchResult::Album(album) => {
            let subtitle = if album.artist.is_empty() { "Album" } else { &album.artist };
            format!("{} - {subtitle}", album.title)
        }
        SearchResult::Artist(artist) => format!("{} - Artist", artist.name),
        SearchResult::Playlist(playlist) => format!("{} - Playlist", playlist.title),
    }
}

fn item_detail_target(item: &SearchResult) -> Option<(String, DetailKind)> {
    match item {
        SearchResult::Album(album) => Some((album.id.clone(), DetailKind::Album)),
        SearchResult::Artist(artist) => Some((artist.id.clone(), DetailKind::Artist)),
        SearchResult::Playlist(playlist) => Some((playlist.id.clone(), DetailKind::Playlist)),
        SearchResult::Track(_) => None,
    }
}

fn render_home_shelves<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.horizontal(|ui| {
        ui.heading("Home");
        if ui.button("Refresh").clicked() {
            state.reload_home();
        }
    });
    let mut to_open = None;
    for shelf in &state.home_shelves {
        ui.heading(&shelf.title);
        for item in &shelf.items {
            ui.horizontal(|ui| {
                ui.label(shelf_item_display(item));
                if let Some((id, kind)) = item_detail_target(item) {
                    if ui.button("Open").clicked() {
                        to_open = Some((id, kind));
                    }
                }
            });
        }
    }
    if let Some((id, kind)) = to_open {
        state.open_detail(&id, kind);
    }
}

fn render_detail<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    if ui.button("Back").clicked() {
        state.close_detail();
        return;
    }
    let Some(detail) = state.detail.clone() else {
        ui.label("No detail available.");
        return;
    };
    ui.heading(&detail.title);
    if !detail.subtitle.is_empty() {
        ui.label(&detail.subtitle);
    }
    ui.separator();
    render_detail_tracks(ui, state, &detail.tracks);
}

fn render_detail_tracks<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
    tracks: &[Track],
) {
    if tracks.is_empty() {
        ui.label("No tracks available.");
        return;
    }
    let mut to_play = None;
    for track in tracks {
        ui.horizontal(|ui| {
            let dur_text = track.duration_secs.map_or_else(String::new, |d| format!(" ({d}s)"));
            ui.label(format!("{} - {}{dur_text}", track.title, track.artist));
            if ui.button("Play").clicked() {
                to_play = Some(track.clone());
            }
        });
    }
    if let Some(track) = to_play {
        state.queue.push(track);
        state.play_head();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Album, Artist, Detail, DetailKind, Playlist, SearchQuery, SearchResult, Shelf, Track,
    };
    use crate::infra::{
        AudioStub, CatalogStub, LrclibStub, LyricLine, MemoryCoverCache, MemorySessionStore,
        ReleaseInfo, StaticUpdater, Updater,
    };

    #[derive(Default)]
    struct CountingCatalog {
        inner: CatalogStub,
        home_calls: std::sync::atomic::AtomicUsize,
    }

    impl Catalog for CountingCatalog {
        fn search(&self, query: &str) -> Vec<Track> {
            self.inner.search(query)
        }
        fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult> {
            self.inner.search_filtered(query)
        }
        fn radio(&self, seed_track_id: &str) -> Vec<Track> {
            self.inner.radio(seed_track_id)
        }
        fn home_shelves(&self) -> Vec<Shelf> {
            self.home_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.home_shelves()
        }
        fn playlists(&self) -> Vec<Playlist> {
            self.inner.playlists()
        }
        fn liked_tracks(&self) -> Vec<Track> {
            self.inner.liked_tracks()
        }
        fn saved_albums(&self) -> Vec<Album> {
            self.inner.saved_albums()
        }
        fn set_liked(&mut self, track_id: &str, liked: bool) {
            self.inner.set_liked(track_id, liked);
        }
        fn detail(&self, browse_id: &str, kind: DetailKind) -> Option<Detail> {
            self.inner.detail(browse_id, kind)
        }
        fn set_session_cookies(&mut self, cookies: Option<&str>) {
            self.inner.set_session_cookies(cookies);
        }
    }

    struct NoUpdates;

    impl Updater for NoUpdates {
        fn latest(&self) -> Option<ReleaseInfo> {
            None
        }
    }

    struct CustomReleaseUpdater(String);

    impl Updater for CustomReleaseUpdater {
        fn latest(&self) -> Option<ReleaseInfo> {
            Some(ReleaseInfo { version: self.0.clone(), url: "https://example.com/release".into() })
        }
    }

    fn wired() -> AppState<CatalogStub, AudioStub, MemorySessionStore, LrclibStub, MemoryCoverCache>
    {
        AppState::new(
            CatalogStub::new(),
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        )
    }

    #[test]
    fn sign_in_and_out_lifecycle_with_sanitized_input() {
        let mut state = wired();
        assert!(!state.is_signed_in());
        assert_eq!(state.catalog.last_session_cookies(), None);

        state.cookie_text = "  cookie: SID=secret123; HSID=secret456  ".to_owned();
        let ok = state.sign_in_from_input();
        assert!(ok);
        assert!(state.is_signed_in());
        assert_eq!(state.cookie_text, "");
        assert_eq!(state.session.load().as_deref(), Some("SID=secret123; HSID=secret456"));
        assert_eq!(state.catalog.last_session_cookies(), Some("SID=secret123; HSID=secret456"));

        state.sign_out();
        assert!(!state.is_signed_in());
        assert_eq!(state.session.load(), None);
        assert_eq!(state.catalog.last_session_cookies(), None);
    }

    #[test]
    fn sign_in_from_input_rejects_empty_or_whitespace() {
        let mut state = wired();
        for invalid in ["", "   ", "cookie:", "COOKIE:   "] {
            state.cookie_text = invalid.to_owned();
            let ok = state.sign_in_from_input();
            assert!(!ok, "expected '{invalid}' to be rejected");
            assert!(!state.is_signed_in());
            assert_eq!(state.catalog.last_session_cookies(), None);
        }
    }

    #[test]
    fn startup_with_preloaded_session_store_forwards_cookies() {
        let mut session = MemorySessionStore::new();
        session.save("PRELOADED=session_cookie_123");
        let state = AppState::new(
            CatalogStub::new(),
            AudioStub::new(),
            session,
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );
        assert!(state.is_signed_in());
        assert_eq!(state.catalog.last_session_cookies(), Some("PRELOADED=session_cookie_123"));
    }

    #[test]
    fn toggle_like_flips_state_and_ignores_blank() {
        let mut state = wired();

        // Stub catalog defaults to "stub-1" liked.
        assert!(state.is_liked("stub-1"));
        assert!(!state.is_liked("stub-2"));

        // Toggle existing liked track off.
        state.toggle_like("stub-1");
        assert!(!state.is_liked("stub-1"));

        // Toggle back on.
        state.toggle_like("stub-1");
        assert!(state.is_liked("stub-1"));

        // Toggle unliked track on.
        state.toggle_like("stub-2");
        assert!(state.is_liked("stub-2"));

        // Blank and whitespace ids are ignored.
        state.toggle_like("");
        state.toggle_like("   ");
        assert!(state.is_liked("stub-1"));
        assert!(state.is_liked("stub-2"));
        assert!(!state.is_liked(""));
        assert!(!state.is_liked("   "));
    }

    #[test]
    fn library_page_selection_and_state() {
        let mut state = wired();
        assert_eq!(state.page, Page::Search);
        state.page = Page::Library;
        assert_eq!(state.page, Page::Library);
        assert!(!state.catalog.playlists().is_empty());
        assert!(!state.catalog.liked_tracks().is_empty());
        assert!(!state.catalog.saved_albums().is_empty());
    }

    #[test]
    fn empty_query_keeps_page_and_queue() {
        let mut state = wired();
        state.search_text = "   ".to_owned();
        state.submit_search();
        assert_eq!(state.page, Page::Search);
        assert_eq!(state.queue.len(), 0);
    }

    #[test]
    fn search_hit_enqueues_and_navigates_to_queue() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        assert_eq!(state.page, Page::Queue);
        assert_eq!(state.queue.len(), 1);
    }

    #[test]
    fn play_head_navigates_to_now_playing() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.play_head();
        assert_eq!(state.page, Page::NowPlaying);
    }

    #[test]
    fn toggle_on_empty_queue_does_nothing() {
        let mut state = wired();
        state.toggle_play();
        assert!(!state.audio.is_playing());
        assert_eq!(state.audio.last_requested(), None);
    }

    #[test]
    fn search_then_toggle_plays_head() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.toggle_play();
        assert!(state.audio.is_playing());
        assert_eq!(state.audio.last_requested(), Some("stub-1"));
        assert_eq!(state.page, Page::Queue);
    }

    #[test]
    fn next_and_prev_move_current_and_play() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.search_text = "sunrise".to_owned();
        state.submit_search();
        assert_eq!(state.queue.len(), 2);

        state.next_track();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("sunrise"));

        state.prev_track();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));
    }

    #[test]
    fn seek_by_clamps_at_zero_and_advances() {
        let mut state = wired();
        state.seek_by(-10);
        assert_eq!(state.audio.position_secs(), 0);

        state.seek_by(15);
        assert_eq!(state.audio.position_secs(), 15);

        state.seek_by(-5);
        assert_eq!(state.audio.position_secs(), 10);

        state.seek_by(-20);
        assert_eq!(state.audio.position_secs(), 0);
    }

    #[test]
    fn set_volume_clamps_via_stub() {
        let mut state = wired();
        state.set_volume(50);
        assert_eq!(state.audio.volume(), 50);

        state.set_volume(150);
        assert_eq!(state.audio.volume(), 100);
    }

    #[test]
    fn format_lyric_timestamp_table() {
        for (ms, exp) in [(0, "0:00"), (5_000, "0:05"), (65_000, "1:05"), (612_000, "10:12")] {
            assert_eq!(format_lyric_timestamp(ms), exp);
        }
    }

    #[test]
    fn lyrics_offset_and_visible_lines_clamping_table() {
        let mut state = wired();
        state.lyrics_lines = vec![
            LyricLine { start_ms: 0, text: "intro".into() },
            LyricLine { start_ms: 5_000, text: "verse".into() },
        ];
        for (delta, off, fst, snd) in [
            (0, 0, 0, 5_000),
            (250, 250, 250, 5_250),
            (-500, -250, 0, 4_750),
            (-5_000, -5_250, 0, 0),
        ] {
            state.nudge_lyrics_offset(delta);
            assert_eq!(state.lyrics_offset_ms, off);
            assert_eq!(
                state.visible_lyric_lines(),
                vec![(fst, "intro".into()), (snd, "verse".into())]
            );
        }
        state.nudge_lyrics_offset(i64::MAX);
        assert_eq!(state.lyrics_offset_ms, i64::MAX - 5_250);
        state.nudge_lyrics_offset(10_000);
        assert_eq!(state.lyrics_offset_ms, i64::MAX);
    }

    #[test]
    fn refresh_lyrics_lifecycle_tracks_active_song_and_refills_on_change() {
        let mut state = wired();
        state.refresh_lyrics();
        assert!(state.lyrics_key.is_none() && state.lyrics_lines.is_empty());

        state.search_text = "midnight".into();
        state.submit_search();
        state.refresh_lyrics();
        assert_eq!(state.lyrics_key, Some(("Stub Artist".into(), "midnight".into())));
        assert_eq!(state.lyrics_lines.len(), 2);
        assert_eq!(state.lyrics_lines[0].text, "midnight - Stub Artist");

        // Skip on same key
        state.refresh_lyrics();
        assert_eq!(state.lyrics_lines.len(), 2);

        // Refill on track change via stub (2 lines)
        state.queue.push(Track::new("s2", "sunrise", "Sunrise Band"));
        state.queue.advance();
        state.refresh_lyrics();
        assert_eq!(state.lyrics_key, Some(("Sunrise Band".into(), "sunrise".into())));
        assert_eq!(state.lyrics_lines.len(), 2);
        assert_eq!(state.lyrics_lines[0].text, "sunrise - Sunrise Band");
    }

    #[test]
    fn drain_tray_with_none_returns_false_and_preserves_state() {
        let mut state = wired();
        assert!(state.tray.is_none());
        let quit = state.drain_tray();
        assert!(!quit);
        assert!(!state.audio.is_playing());
    }

    #[test]
    fn apply_actions_empty_and_show_return_false() {
        let mut state = wired();
        assert!(!state.apply_actions(&[]));
        assert!(!state.apply_actions(&[TrayAction::Show]));
        assert!(!state.audio.is_playing());
    }

    #[test]
    fn apply_actions_toggle_next_prev_and_quit() {
        let mut state = wired();
        state.search_text = "midnight".into();
        state.submit_search();
        state.search_text = "sunrise".into();
        state.submit_search();
        assert_eq!(state.queue.len(), 2);

        // Toggle starts playback of head track
        let quit = state.apply_actions(&[TrayAction::TogglePlay]);
        assert!(!quit);
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));

        // Next advances to sunrise and plays
        let quit = state.apply_actions(&[TrayAction::Next]);
        assert!(!quit);
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("sunrise"));

        // Previous retreats to midnight and plays
        let quit = state.apply_actions(&[TrayAction::Previous]);
        assert!(!quit);
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));

        // Quit returns true
        assert!(state.apply_actions(&[TrayAction::Quit]));

        // Compound batch: TogglePlay then Quit toggles (pauses) and returns true
        let quit = state.apply_actions(&[TrayAction::TogglePlay, TrayAction::Quit]);
        assert!(quit);
        assert!(!state.audio.is_playing());
    }

    #[test]
    fn inbound_media_keys_dispatch_via_presence_queue() {
        let mut state = wired();
        state.search_text = "midnight".into();
        state.submit_search();
        state.search_text = "sunrise".into();
        state.submit_search();
        assert_eq!(state.queue.len(), 2);
        assert!(!state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));

        // Queued Toggle flips playback state via existing fakes (starts playing head)
        state.presence.enqueue_action(MediaKeyAction::Toggle);
        state.sync_presence();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));

        // Queued Next advances the queue head and plays
        state.presence.enqueue_action(MediaKeyAction::Next);
        state.sync_presence();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("sunrise"));

        // Queued Toggle flips playback state back to paused
        state.presence.enqueue_action(MediaKeyAction::Toggle);
        state.sync_presence();
        assert!(!state.audio.is_playing());

        // Queued Stop on paused playback remains paused
        state.presence.enqueue_action(MediaKeyAction::Stop);
        state.sync_presence();
        assert!(!state.audio.is_playing());

        // Resume playback with Toggle, then Stop pauses
        state.presence.enqueue_action(MediaKeyAction::Toggle);
        state.sync_presence();
        assert!(state.audio.is_playing());

        state.presence.enqueue_action(MediaKeyAction::Stop);
        state.sync_presence();
        assert!(!state.audio.is_playing());

        // Queued Previous retreats the queue head and plays
        state.presence.enqueue_action(MediaKeyAction::Previous);
        state.sync_presence();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));
    }

    #[test]
    fn sync_presence_with_empty_queue_clears_and_tracks_status() {
        let mut state = wired();
        state.sync_presence();
        assert_eq!(state.presence.last_published(), None);
        assert_eq!(state.presence.last_status(), Some(false));
    }

    #[test]
    fn sync_presence_with_queued_track_publishes_and_deduplicates_key() {
        let mut state = wired();
        let track = Track {
            id: "vid-1".into(),
            title: "Paranoid Android".into(),
            artist: "Radiohead".into(),
            album: None,
            duration_secs: Some(383),
            artwork: None,
        };
        state.queue.push(track);
        state.audio.play("vid-1");

        state.sync_presence();
        assert_eq!(
            state.presence.last_published(),
            Some(("Radiohead".into(), "Paranoid Android".into()))
        );
        assert_eq!(state.presence.last_status(), Some(true));
        assert_eq!(state.last_presence_key, Some(("Radiohead".into(), "Paranoid Android".into())));

        // Pause: set_track skipped because key unchanged, but set_playing updated to false
        state.audio.pause();
        state.sync_presence();
        assert_eq!(state.presence.last_status(), Some(false));
        assert_eq!(state.last_presence_key, Some(("Radiohead".into(), "Paranoid Android".into())));

        // Clear queue: sync clears presence metadata and resets key
        state.queue.clear();
        state.sync_presence();
        assert_eq!(state.presence.last_published(), None);
        assert_eq!(state.presence.last_status(), Some(false));
        assert_eq!(state.last_presence_key, None);
    }

    #[test]
    fn sync_presence_fans_out_to_discord_presence_and_clears_on_empty() {
        let mut state = wired();
        assert_eq!(state.discord.last_published(), None);

        let track = Track {
            id: "vid-1".into(),
            title: "Paranoid Android".into(),
            artist: "Radiohead".into(),
            album: None,
            duration_secs: Some(383),
            artwork: None,
        };
        state.queue.push(track);
        state.audio.play("vid-1");

        state.sync_presence();
        assert_eq!(
            state.discord.last_published(),
            Some(("Paranoid Android".into(), "Radiohead".into()))
        );

        state.queue.clear();
        state.sync_presence();
        assert_eq!(state.discord.last_published(), None);
    }

    #[test]
    fn update_status_lifecycle_and_dismiss() {
        let mut state = wired();
        assert_eq!(*state.update_status(), UpdateStatus::Unknown);

        state.check_with(&StaticUpdater::new());
        assert_eq!(
            *state.update_status(),
            UpdateStatus::Available {
                version: "v9.9.9-test".into(),
                url: "https://github.com/example/moggyt/releases/tag/v9.9.9-test".into(),
            }
        );

        state.dismiss_update();
        assert_eq!(*state.update_status(), UpdateStatus::Current);
    }

    #[test]
    fn scrobbler_now_playing_recorded_on_playback_funnel() {
        let mut state = wired();
        state.search_text = "midnight".to_owned();
        state.submit_search();
        state.toggle_play();

        let stub = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().expect("stub");
        assert_eq!(
            stub.now_playing_calls,
            vec![("Stub Artist".to_string(), "midnight".to_string())]
        );
    }

    #[test]
    fn scrobbler_advance_threshold_and_recording_lifecycle() {
        let mut state = wired();
        let track = Track::new_full("vid-1", "Karma Police", "Radiohead", None, Some(240));
        state.queue.push(track);
        state.play_current_quiet();

        // Advance immediately with ~0s elapsed -> no scrobble
        state.next_track();
        let stub = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().expect("stub");
        assert!(stub.scrobble_calls.is_empty());

        // With sufficient elapsed playtime, scrobble is recorded
        state.maybe_scrobble_with_elapsed(120);
        let stub = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().expect("stub");
        assert_eq!(stub.scrobble_calls.len(), 1);
        assert_eq!(stub.scrobble_calls[0].0, "Radiohead");
        assert_eq!(stub.scrobble_calls[0].1, "Karma Police");
    }

    #[test]
    fn update_check_failure_and_up_to_date_cases() {
        let mut state = wired();

        state.check_with(&NoUpdates);
        assert_eq!(*state.update_status(), UpdateStatus::CheckFailed);

        state.check_with(&CustomReleaseUpdater("0.1.0".into()));
        assert_eq!(*state.update_status(), UpdateStatus::Current);

        state.check_with(&CustomReleaseUpdater("0.0.1".into()));
        assert_eq!(*state.update_status(), UpdateStatus::Current);
    }

    #[test]
    fn resolve_cover_bytes_empty_queue_clears_and_returns_none() {
        let mut state = wired();
        state.cover_key = Some("https://example.com/art.png".to_string());
        assert_eq!(state.resolve_cover_bytes(), None);
        assert_eq!(state.cover_key, None);
        assert!(state.cover_texture.is_none());
    }

    #[test]
    fn resolve_cover_bytes_track_without_artwork_clears_and_returns_none() {
        let mut state = wired();
        let track = Track::new("t1", "Song", "Artist");
        state.queue.push(track);
        state.cover_key = Some("https://example.com/art.png".to_string());
        assert_eq!(state.resolve_cover_bytes(), None);
        assert_eq!(state.cover_key, None);
        assert!(state.cover_texture.is_none());
    }

    #[test]
    fn resolve_cover_bytes_cache_hit_returns_url_and_bytes() {
        let mut state = wired();
        let url = "https://example.com/art.png";
        let track = Track::new("t1", "Song", "Artist").with_artwork(url);
        state.queue.push(track);
        let sample_bytes = vec![1, 2, 3, 4];
        state.covers.put(url, sample_bytes.clone());

        let resolved = state.resolve_cover_bytes();
        assert_eq!(resolved, Some((url.to_string(), sample_bytes)));
    }

    #[test]
    fn resolve_cover_bytes_deduplicates_when_already_matching_key() {
        let mut state = wired();
        let url = "https://example.com/art.png";
        let track = Track::new("t1", "Song", "Artist").with_artwork(url);
        state.queue.push(track);
        state.covers.put(url, vec![1, 2, 3]);
        state.cover_key = Some(url.to_string());

        assert_eq!(state.resolve_cover_bytes(), None);
        assert_eq!(state.cover_key.as_deref(), Some(url));
    }

    #[test]
    fn resolve_cover_bytes_advancing_track_resolves_new_artwork() {
        let mut state = wired();
        let url1 = "https://example.com/art1.png";
        let url2 = "https://example.com/art2.png";
        state.queue.push(Track::new("t1", "Song 1", "Artist").with_artwork(url1));
        state.queue.push(Track::new("t2", "Song 2", "Artist").with_artwork(url2));
        state.covers.put(url1, vec![10, 20]);
        state.covers.put(url2, vec![30, 40]);

        // First track
        assert_eq!(state.resolve_cover_bytes(), Some((url1.to_string(), vec![10, 20])));
        state.cover_key = Some(url1.to_string());

        // Advance to second track
        state.queue.advance();
        assert_eq!(state.resolve_cover_bytes(), Some((url2.to_string(), vec![30, 40])));
    }

    #[test]
    fn theme_and_locale_setters_mutate_state_and_persist() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        assert_eq!(state.theme, ThemeId::White);
        assert_eq!(state.locale, Locale::English);
        assert_eq!(state.quality, AudioQuality::Standard);
        assert_eq!(state.audio.volume(), 100);

        state.set_theme(ThemeId::BlackOled);
        assert_eq!(state.theme, ThemeId::BlackOled);
        assert_eq!(store.load().theme, ThemeId::BlackOled);

        state.set_locale(Locale::Spanish);
        assert_eq!(state.locale, Locale::Spanish);
        assert_eq!(store.load().locale, Locale::Spanish);
    }

    #[test]
    fn set_volume_persists_to_settings_store_with_clamping() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        state.set_volume(42);
        assert_eq!(state.audio.volume(), 42);
        assert_eq!(store.load().volume, 42);

        state.set_volume(150);
        assert_eq!(state.audio.volume(), 100);
        assert_eq!(store.load().volume, 100);
    }

    #[test]
    fn set_quality_persists_and_forwards_to_audio_backend() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        state.set_quality(AudioQuality::High);
        assert_eq!(state.quality, AudioQuality::High);
        assert_eq!(store.load().quality, AudioQuality::High);

        state.set_quality(AudioQuality::Compact);
        assert_eq!(state.quality, AudioQuality::Compact);
        assert_eq!(store.load().quality, AudioQuality::Compact);
    }

    #[derive(Debug, Default)]
    struct QualityTrackingAudio {
        stub: AudioStub,
        applied_quality: AudioQuality,
    }

    impl AudioBackend for QualityTrackingAudio {
        fn play(&mut self, track_id: &str) {
            self.stub.play(track_id);
        }
        fn pause(&mut self) {
            self.stub.pause();
        }
        fn resume_play(&mut self) {
            self.stub.resume_play();
        }
        fn seek(&mut self, position_secs: u64) {
            self.stub.seek(position_secs);
        }
        fn set_volume(&mut self, volume: u8) {
            self.stub.set_volume(volume);
        }
        fn volume(&self) -> u8 {
            self.stub.volume()
        }
        fn position_secs(&self) -> u64 {
            self.stub.position_secs()
        }
        fn is_playing(&self) -> bool {
            self.stub.is_playing()
        }
        fn last_requested(&self) -> Option<&str> {
            self.stub.last_requested()
        }
        fn set_quality(&mut self, q: AudioQuality) {
            self.applied_quality = q;
        }
    }

    #[test]
    fn set_settings_store_applies_volume_and_quality_to_stub_backend() {
        let mut state = AppState::new(
            CatalogStub::new(),
            QualityTrackingAudio::default(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );

        let custom_settings = Settings {
            theme: ThemeId::Mono,
            locale: Locale::Spanish,
            volume: 64,
            quality: AudioQuality::Compact,
            keyboard_shortcuts: false,
            lastfm_enabled: false,
            hi_res_covers: false,
        };
        let store = MemorySettingsStore::default();
        store.save(&custom_settings).unwrap();

        state.set_settings_store(Box::new(store));

        assert_eq!(state.theme, ThemeId::Mono);
        assert_eq!(state.locale, Locale::Spanish);
        assert_eq!(state.quality, AudioQuality::Compact);
        assert!(!state.shortcuts_enabled);
        assert_eq!(state.audio.volume(), 64);
        assert_eq!(state.audio.applied_quality, AudioQuality::Compact);
    }

    #[test]
    fn start_radio_seeds_plays_and_navigates() {
        let mut state = wired();
        let seed = Track::new("seed-1", "Seed Song", "Seed Artist").with_duration(180);
        state.start_radio(seed);
        assert_eq!(state.queue.len(), 3);
        assert_eq!(state.queue.tracks()[0].id, "seed-1");
        assert_eq!(state.queue.tracks()[1].id, "seed-1-radio-1");
        assert_eq!(state.queue.tracks()[2].id, "seed-1-radio-2");
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("seed-1"));
        assert!(state.autoplay && state.page == Page::NowPlaying && state.audio.is_playing());
        assert_eq!(state.audio.last_requested(), Some("seed-1"));
    }

    #[test]
    fn start_radio_blank_id_is_noop() {
        let mut state = wired();
        state.queue.push(Track::new("kept", "Kept Song", "Artist"));
        state.start_radio(Track::new("", "Empty", "Artist"));
        assert_eq!(state.queue.len(), 1);
        assert!(!state.autoplay && state.page == Page::Search);
        state.start_radio(Track::new("   ", "Spaces", "Artist"));
        assert_eq!(state.queue.len(), 1);
        assert!(!state.autoplay && state.page == Page::Search);
    }

    #[test]
    fn continue_autoplay_if_ended_appends_and_advances_at_end() {
        let mut state = wired();
        state.start_radio(Track::new("seed-1", "Seed", "Artist").with_duration(180));
        state.next_track();
        state.next_track();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("seed-1-radio-2"));

        state.audio.seek(210);
        state.audio.pause();
        state.continue_autoplay_if_ended();

        assert_eq!(state.queue.len(), 5);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("seed-1-radio-2-radio-1"));
        assert!(state.audio.is_playing());
        assert_eq!(state.audio.last_requested(), Some("seed-1-radio-2-radio-1"));
    }

    #[test]
    fn continue_autoplay_if_ended_noops() {
        let mut state = wired();
        state.start_radio(Track::new("seed-1", "Seed", "Artist").with_duration(180));

        // Mid-queue: not final track
        state.audio.seek(180);
        state.audio.pause();
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 3);

        // Move to last track
        state.next_track();
        state.next_track();

        // While playing
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 3);

        // Paused mid-track
        state.audio.seek(100);
        state.audio.pause();
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 3);

        // Autoplay off
        state.audio.seek(210);
        state.autoplay = false;
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 3);

        // Unknown duration
        state.autoplay = true;
        state.queue.push(Track::new("unknown-dur", "Unknown", "Artist"));
        state.next_track();
        state.audio.seek(500);
        state.audio.pause();
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), 4);
    }

    #[test]
    fn continue_autoplay_deduplicates_and_skips_existing_tracks() {
        let mut state = wired();
        state.start_radio(Track::new("seed-1", "Seed", "Artist").with_duration(180));
        state.next_track();
        state.next_track();
        state.queue.push(Track::new("seed-1-radio-2-radio-1", "R1", "Stub Artist"));
        state.queue.push(Track::new("seed-1-radio-2-radio-2", "R2", "Stub Artist"));
        state.next_track();
        state.next_track();
        let len_before = state.queue.len();

        state.audio.seek(210);
        state.audio.pause();
        state.continue_autoplay_if_ended();
        assert_eq!(state.queue.len(), len_before);
    }

    #[test]
    fn ensure_home_fetches_once_and_deduplicates() {
        let catalog = CountingCatalog::default();
        let mut state = AppState::new(
            catalog,
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );

        assert!(state.home_shelves.is_empty());
        assert_eq!(state.catalog.home_calls.load(std::sync::atomic::Ordering::SeqCst), 0);

        // First call fetches
        state.ensure_home();
        assert_eq!(state.home_shelves.len(), 2);
        assert_eq!(state.catalog.home_calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Second call deduplicates (does not re-query)
        state.ensure_home();
        assert_eq!(state.home_shelves.len(), 2);
        assert_eq!(state.catalog.home_calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Reload refetches
        state.reload_home();
        assert_eq!(state.home_shelves.len(), 2);
        assert_eq!(state.catalog.home_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn shelf_item_display_formatting() {
        assert_eq!(
            shelf_item_display(&SearchResult::Track(Track::new("t1", "TrackTitle", "TrackArtist"))),
            "TrackTitle - TrackArtist"
        );
        assert_eq!(
            shelf_item_display(&SearchResult::Album(Album::new("a1", "AlbumTitle", "AlbumArtist"))),
            "AlbumTitle - AlbumArtist"
        );
        assert_eq!(
            shelf_item_display(&SearchResult::Album(Album::new("a2", "AlbumTitle", ""))),
            "AlbumTitle - Album"
        );
        assert_eq!(
            shelf_item_display(&SearchResult::Artist(Artist::new("ar1", "ArtistName"))),
            "ArtistName - Artist"
        );
        assert_eq!(
            shelf_item_display(&SearchResult::Playlist(Playlist::new("p1", "PlaylistTitle", 10))),
            "PlaylistTitle - Playlist"
        );
    }

    #[test]
    fn open_detail_sets_page_detail_and_return_page() {
        let mut state = wired();
        assert_eq!(state.page, Page::Search);
        state.open_detail("stub-album-1", DetailKind::Album);
        assert_eq!(state.page, Page::Detail);
        assert_eq!(state.detail_return, Page::Search);
        let detail = state.detail.as_ref().expect("detail should be loaded");
        assert_eq!(detail.kind, DetailKind::Album);
        assert_eq!(detail.title, "stub-album-1 Title");
        assert_eq!(detail.tracks.len(), 2);
    }

    #[test]
    fn open_detail_blank_id_is_noop() {
        let mut state = wired();
        state.open_detail("", DetailKind::Album);
        assert_eq!(state.page, Page::Search);
        assert!(state.detail.is_none());

        state.open_detail("   ", DetailKind::Artist);
        assert_eq!(state.page, Page::Search);
        assert!(state.detail.is_none());
    }

    #[test]
    fn open_detail_preserves_initial_return_page_when_already_detail() {
        let mut state = wired();
        state.page = Page::Queue;
        state.open_detail("alb-1", DetailKind::Album);
        assert_eq!(state.page, Page::Detail);
        assert_eq!(state.detail_return, Page::Queue);

        state.open_detail("art-1", DetailKind::Artist);
        assert_eq!(state.page, Page::Detail);
        assert_eq!(state.detail_return, Page::Queue);
    }

    #[test]
    fn close_detail_restores_return_page_and_clears_detail() {
        let mut state = wired();
        state.page = Page::Library;
        state.open_detail("alb-1", DetailKind::Album);
        assert_eq!(state.page, Page::Detail);
        assert!(state.detail.is_some());

        state.close_detail();
        assert_eq!(state.page, Page::Library);
        assert!(state.detail.is_none());
    }

    #[test]
    fn item_detail_target_mapping() {
        assert_eq!(
            item_detail_target(&SearchResult::Album(Album::new("a1", "Alb", "Art"))),
            Some(("a1".into(), DetailKind::Album))
        );
        assert_eq!(
            item_detail_target(&SearchResult::Artist(Artist::new("ar1", "Art"))),
            Some(("ar1".into(), DetailKind::Artist))
        );
        assert_eq!(
            item_detail_target(&SearchResult::Playlist(Playlist::new("p1", "Play", 5))),
            Some(("p1".into(), DetailKind::Playlist))
        );
        assert_eq!(
            item_detail_target(&SearchResult::Track(Track::new("t1", "Track", "Art"))),
            None
        );
    }

    #[test]
    fn test_apply_key_handling_matrix() {
        let mut state = wired();
        state.search_text = "midnight".into();
        state.submit_search();
        state.search_text = "sunrise".into();
        state.submit_search();
        assert_eq!(state.queue.len(), 2);

        // Space toggles playback
        assert!(state.apply_key(egui::Key::Space));
        assert!(state.audio.is_playing());
        assert!(state.apply_key(egui::Key::Space));
        assert!(!state.audio.is_playing());

        // Seek forward and backward
        assert!(state.apply_key(egui::Key::ArrowRight));
        assert_eq!(state.audio.position_secs(), 10);
        assert!(state.apply_key(egui::Key::ArrowLeft));
        assert_eq!(state.audio.position_secs(), 0);

        // Volume adjustments
        state.set_volume(50);
        assert!(state.apply_key(egui::Key::ArrowUp));
        assert_eq!(state.audio.volume(), 55);
        assert!(state.apply_key(egui::Key::ArrowDown));
        assert_eq!(state.audio.volume(), 50);

        // Track progression and regression
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));
        assert!(state.apply_key(egui::Key::N));
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("sunrise"));
        assert!(state.apply_key(egui::Key::P));
        assert_eq!(state.queue.current().map(|t| t.title.as_str()), Some("midnight"));

        // Toggle like status for current track
        let curr_id = state.queue.current().unwrap().id.clone();
        let initial_liked = state.is_liked(&curr_id);
        assert!(state.apply_key(egui::Key::L));
        assert_eq!(state.is_liked(&curr_id), !initial_liked);
        assert!(state.apply_key(egui::Key::L));
        assert_eq!(state.is_liked(&curr_id), initial_liked);

        // Unhandled keys return false
        assert!(!state.apply_key(egui::Key::A));
        assert!(!state.apply_key(egui::Key::Enter));
        assert!(!state.apply_key(egui::Key::Escape));
    }

    #[test]
    fn test_apply_key_when_shortcuts_disabled_returns_false_and_does_not_mutate() {
        let mut state = wired();
        state.search_text = "midnight".into();
        state.submit_search();
        state.set_shortcuts_enabled(false);

        let initial_pos = state.audio.position_secs();
        let initial_vol = state.audio.volume();
        let initial_playing = state.audio.is_playing();
        let initial_title = state.queue.current().map(|t| t.title.clone());

        for key in [
            egui::Key::Space,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::N,
            egui::Key::P,
            egui::Key::L,
            egui::Key::A,
        ] {
            assert!(!state.apply_key(key));
        }

        assert_eq!(state.audio.position_secs(), initial_pos);
        assert_eq!(state.audio.volume(), initial_vol);
        assert_eq!(state.audio.is_playing(), initial_playing);
        assert_eq!(state.queue.current().map(|t| t.title.clone()), initial_title);
    }

    #[test]
    fn test_apply_key_queue_dependent_keys_noop_on_empty_queue() {
        let mut state = wired();
        assert!(state.queue.is_empty());

        assert!(!state.apply_key(egui::Key::N));
        assert!(!state.apply_key(egui::Key::P));
        assert!(!state.apply_key(egui::Key::L));
        assert_eq!(state.audio.last_requested(), None);
    }

    #[test]
    fn test_apply_key_volume_clamping_at_edges() {
        let mut state = wired();

        // Upper boundary
        state.set_volume(98);
        assert!(state.apply_key(egui::Key::ArrowUp));
        assert_eq!(state.audio.volume(), 100);
        assert!(state.apply_key(egui::Key::ArrowUp));
        assert_eq!(state.audio.volume(), 100);

        // Lower boundary
        state.set_volume(2);
        assert!(state.apply_key(egui::Key::ArrowDown));
        assert_eq!(state.audio.volume(), 0);
        assert!(state.apply_key(egui::Key::ArrowDown));
        assert_eq!(state.audio.volume(), 0);
    }

    #[test]
    fn test_set_shortcuts_enabled_persists_flag() {
        let mut state = wired();
        assert!(state.shortcuts_enabled);
        assert!(state.keyboard_shortcuts_enabled());

        state.set_shortcuts_enabled(false);
        assert!(!state.shortcuts_enabled);
        assert!(!state.keyboard_shortcuts_enabled());
        assert!(!state.settings_store.load().keyboard_shortcuts);

        state.set_shortcuts_enabled(true);
        assert!(state.shortcuts_enabled);
        assert!(state.keyboard_shortcuts_enabled());
        assert!(state.settings_store.load().keyboard_shortcuts);
    }

    #[test]
    fn test_set_settings_store_syncs_shortcuts_flag() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        store
            .save(&Settings {
                theme: ThemeId::White,
                locale: Locale::English,
                volume: 80,
                quality: AudioQuality::Standard,
                keyboard_shortcuts: false,
                lastfm_enabled: false,
                hi_res_covers: false,
            })
            .unwrap();

        state.set_settings_store(Box::new(store));
        assert!(!state.shortcuts_enabled);
    }

    #[test]
    fn test_enabling_lastfm_with_complete_creds_yields_live_client() {
        let mut state = wired();
        let creds = LastFmCredentials::new("api-key", "api-secret", "session-key");
        state.save_lastfm_credentials(&creds);
        assert!(!state.lastfm_is_live());

        state.set_lastfm_enabled(true);
        assert!(state.lastfm_enabled);
        assert!(state.lastfm_is_live());
        assert!(state.settings_store.load().lastfm_enabled);
    }

    #[test]
    fn test_disabling_lastfm_yields_dormant_client_and_preserves_stored_creds() {
        let mut state = wired();
        let creds = LastFmCredentials::new("api-key", "api-secret", "session-key");
        state.save_lastfm_credentials(&creds);
        state.set_lastfm_enabled(true);
        assert!(state.lastfm_is_live());

        state.set_lastfm_enabled(false);
        assert!(!state.lastfm_enabled);
        assert!(!state.lastfm_is_live());
        assert!(!state.settings_store.load().lastfm_enabled);
        // Disabling scrobbler clears the session client but keeps stored credentials in vault
        assert_eq!(state.credentials.load_lastfm(), Some(creds));
    }

    #[test]
    fn test_incomplete_creds_never_go_live() {
        let mut state = wired();
        state.set_lastfm_enabled(true);

        for (k, s, sk) in [
            ("", "secret", "sk"),
            ("key", "", "sk"),
            ("key", "secret", ""),
            ("   ", "secret", "sk"),
            ("key", "   ", "sk"),
            ("key", "secret", "   "),
        ] {
            let incomplete = LastFmCredentials::new(k, s, sk);
            state.save_lastfm_credentials(&incomplete);
            assert!(
                !state.lastfm_is_live(),
                "expected incomplete creds ({k}, {s}, {sk}) to remain dormant"
            );
        }
    }

    #[test]
    fn test_lastfm_credentials_save_clear_round_trip() {
        let mut state = wired();
        assert_eq!(state.credentials.load_lastfm(), None);

        let creds = LastFmCredentials::new("my-key", "my-secret", "my-session");
        state.save_lastfm_credentials(&creds);
        assert_eq!(state.credentials.load_lastfm(), Some(creds));

        state.clear_lastfm_credentials();
        assert_eq!(state.credentials.load_lastfm(), None);
        assert!(!state.lastfm_is_live());
    }

    #[test]
    fn test_lastfm_credentials_from_input_buffers() {
        let mut state = wired();
        state.lastfm_api_key_text = "test-key".into();
        state.lastfm_secret_text = "test-secret".into();
        state.lastfm_session_key_text = "test-session".into();
        state.set_lastfm_enabled(true);

        let ok = state.save_lastfm_credentials_from_input();
        assert!(ok);
        assert!(state.lastfm_is_live());
        assert!(state.lastfm_api_key_text.is_empty());
        assert!(state.lastfm_secret_text.is_empty());
        assert!(state.lastfm_session_key_text.is_empty());

        state.clear_lastfm_credentials();
        assert!(!state.lastfm_is_live());
        assert_eq!(state.credentials.load_lastfm(), None);
    }

    #[test]
    fn test_hi_res_covers_setting_toggles_decode_bound_and_persists() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        assert!(!state.hi_res_covers);
        assert_eq!(state.cover_max_px(), COVER_MAX_PX);

        state.set_hi_res_covers(true);
        assert!(state.hi_res_covers);
        assert_eq!(state.cover_max_px(), COVER_HIRES_MAX_PX);
        assert!(store.load().hi_res_covers);

        state.set_hi_res_covers(false);
        assert!(!state.hi_res_covers);
        assert_eq!(state.cover_max_px(), COVER_MAX_PX);
        assert!(!store.load().hi_res_covers);
    }

    #[test]
    fn test_decode_cover_image_honors_hires_flag() {
        let mut state = wired();
        let img = image::RgbaImage::from_pixel(1200, 600, image::Rgba([10, 20, 30, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("encode test PNG");

        // With default (flag off), capped at COVER_MAX_PX (512)
        let decoded_default = state.decode_cover_image(&buf).expect("decode default");
        assert_eq!(decoded_default.size, [512, 256]);

        // With flag on, capped at COVER_HIRES_MAX_PX (1024)
        state.set_hi_res_covers(true);
        let decoded_hires = state.decode_cover_image(&buf).expect("decode hires");
        assert_eq!(decoded_hires.size, [1024, 512]);
    }
}
