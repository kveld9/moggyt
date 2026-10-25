//! UI shell: navigation state and event delegation.
//!
//! The shell holds no business rules; every user intent is forwarded to
//! [`crate::domain`] and every side effect goes through injected adapters.

pub mod theme;

use eframe::egui;

use crate::domain::{PlaybackQueue, SearchQuery, ThemeId};
use crate::i18n::{Locale, tr};
use crate::infra::{
    AudioBackend, AudioQuality, COVER_MAX_PX, Catalog, CoverCache, DesktopPresence,
    DiscordPresence, GithubUpdater, LyricLine, LyricsSource, MemorySettingsStore, Presence,
    Scrobbler, SessionStore, Settings, SettingsStore, StubScrobbler, TrayAction, TrayHandle,
    Updater, decode_cover, fetch_artwork, is_newer, sanitize_cookie_input, should_scrobble,
};

/// Navigation pages of the application shell.
#[derive(Debug, Default, PartialEq, Eq)]
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
}

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
    /// Creates the shell with injected collaborators.
    pub fn new(catalog: C, audio: A, session: S, lyrics: L, covers: K) -> Self {
        let signed_in = session.load().is_some();
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
            started_at: None,
            theme: ThemeId::White,
            locale: Locale::English,
            quality: AudioQuality::Standard,
            settings_store: Box::new(MemorySettingsStore::default()),
        }
    }

    /// Injects an alternative scrobbler implementation.
    pub fn set_scrobbler(&mut self, scrobbler: Box<dyn Scrobbler>) {
        self.scrobbler = scrobbler;
    }

    /// Injects an alternative settings store implementation and synchronizes state.
    pub fn set_settings_store(&mut self, store: Box<dyn SettingsStore>) {
        let loaded = store.load();
        self.theme = loaded.theme;
        self.locale = loaded.locale;
        self.quality = loaded.quality;
        self.audio.set_volume(loaded.volume);
        self.audio.set_quality(loaded.quality);
        self.settings_store = store;
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

    fn persist_settings(&self) {
        let settings = Settings {
            theme: self.theme,
            locale: self.locale,
            volume: self.audio.volume(),
            quality: self.quality,
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

    /// Synchronizes current playback state and track metadata with presence adapters.
    ///
    /// Fans out active track metadata or queue clearance to both desktop media presence
    /// (MPRIS on Linux, SMTC on Windows) and Discord Rich Presence.
    fn sync_presence(&mut self) {
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
        self.signed_in = true;
        self.cookie_text.clear();
        true
    }

    /// Clears the stored session and resets the cookie input buffer.
    pub fn sign_out(&mut self) {
        self.session.clear();
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
        if let Some(img) = decode_cover(bytes, COVER_MAX_PX) {
            let texture = ctx.load_texture("moggyt-cover", img, egui::TextureOptions::LINEAR);
            self.cover_key = Some(url.clone());
            self.cover_texture = Some((url, texture));
        } else {
            self.clear_cover_texture();
        }
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
        render_top_bar(ui, &mut self.state);
        render_player_bar(ui, &mut self.state);
        render_body(ui, &mut self.state);
    }
}

fn render_top_bar<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading("moggyt");
            ui.separator();
            ui.selectable_value(&mut state.page, Page::Search, "Search");
            ui.selectable_value(&mut state.page, Page::Queue, "Queue");
            ui.selectable_value(&mut state.page, Page::NowPlaying, "Now playing");
            ui.selectable_value(&mut state.page, Page::Library, "Library");
            render_top_bar_update_badge(ui, &state.update_status);
        });
    });
}

fn render_top_bar_update_badge(ui: &mut egui::Ui, status: &UpdateStatus) {
    let UpdateStatus::Available { version, .. } = status else {
        return;
    };
    // Keep top bar lean with just a colored notice; interactive download link lives in Library.
    let display_tag = if version.starts_with(['v', 'V']) {
        format!("Update {version}")
    } else {
        format!("Update v{version}")
    };
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.colored_label(egui::Color32::from_rgb(255, 180, 50), display_tag);
    });
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
    });
}

fn render_library<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    render_library_preferences(ui, state);
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
        ui.text_edit_singleline(&mut state.cookie_text);
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

fn render_search<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.label("Search the catalog (stub returns fixed hits):");
    let submitted = ui.text_edit_singleline(&mut state.search_text).lost_focus()
        && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if ui.button("Search").clicked() || submitted {
        state.submit_search();
    }
}

fn render_queue<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.label(format!("{} track(s) queued", state.queue.len()));
    ui.horizontal(|ui| {
        if ui.button("Previous").clicked() {
            state.queue.retreat();
        }
        if ui.button("Play head").clicked() {
            state.play_head();
        }
        if ui.button("Next").clicked() {
            state.queue.advance();
        }
    });
    for track in state.queue.tracks() {
        ui.label(track.display());
    }
}

fn render_now_playing<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    let Some(track) = state.queue.current().cloned() else {
        debug_assert!(state.queue.is_empty());
        ui.label("Queue is empty. Search and enqueue a track first.");
        return;
    };
    state.refresh_covers(ui.ctx());
    if state.lyrics_key.as_ref().is_none_or(|k| k.0 != track.artist || k.1 != track.title) {
        state.refresh_lyrics();
    }
    ui.heading(track.display());
    ui.label(format!("by {}", track.artist));
    ui.separator();
    ui.label(format!("requested: {}", state.audio.last_requested().unwrap_or("none")));
    ui.separator();
    render_cover_art_section(ui, state.cover_texture.as_ref());
    ui.separator();
    render_lyrics_panel(ui, state);
}

fn render_cover_art_section(ui: &mut egui::Ui, texture: Option<&(String, egui::TextureHandle)>) {
    if let Some((_, tex)) = texture {
        ui.image((tex.id(), egui::Vec2::new(256.0, 256.0)));
    } else {
        ui.label("Cover art unavailable (offline or pending fetch)");
    }
}

fn render_lyrics_panel<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading("Lyrics");
    ui.horizontal(|ui| {
        ui.label(format!("Offset: {}ms", state.lyrics_offset_ms));
        if ui.button("-250ms").clicked() {
            state.nudge_lyrics_offset(-250);
        }
        if ui.button("+250ms").clicked() {
            state.nudge_lyrics_offset(250);
        }
    });
    let lines = state.visible_lyric_lines();
    if lines.is_empty() {
        ui.label("No synced lyrics for this track.");
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (ms, text) in lines {
            ui.label(format!("{} {text}", format_lyric_timestamp(ms)));
        }
    });
}

fn render_player_bar<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    egui::Panel::bottom("player_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            render_track_info(ui, state);
            ui.separator();
            render_playback_controls(ui, state);
            ui.separator();
            render_seek_controls(ui, state);
            ui.separator();
            render_volume_controls(ui, state);
            ui.separator();
            render_position_info(ui, state);
        });
    });
}

fn render_track_info<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S, L, K>,
) {
    let title = state.queue.current().map_or("Queue empty".to_owned(), |t| t.display());
    ui.label(title);
}

fn render_playback_controls<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    if ui.button("Previous").clicked() {
        state.prev_track();
    }
    let toggle_label = if state.audio.is_playing() { "Pause" } else { "Play" };
    if ui.button(toggle_label).clicked() {
        state.toggle_play();
    }
    if ui.button("Next").clicked() {
        state.next_track();
    }
}

fn render_seek_controls<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    if ui.button("-10s").clicked() {
        state.seek_by(-10);
    }
    if ui.button("+10s").clicked() {
        state.seek_by(10);
    }
}

fn render_volume_controls<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    let mut vol = state.audio.volume();
    if ui.add(egui::Slider::new(&mut vol, 0..=100).text("Vol")).changed() {
        state.set_volume(vol);
    }
}

fn render_position_info<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S, L, K>,
) {
    let pos = state.audio.position_secs();
    let text = match state.queue.current().and_then(|t| t.duration_secs) {
        Some(dur) => format!("{pos}s / {dur}s"),
        None => format!("{pos}s"),
    };
    ui.label(text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Track;
    use crate::infra::{
        AudioStub, CatalogStub, LrclibStub, LyricLine, MemoryCoverCache, MemorySessionStore,
        ReleaseInfo, StaticUpdater, Updater,
    };

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

        state.cookie_text = "  cookie: SID=secret123; HSID=secret456  ".to_owned();
        let ok = state.sign_in_from_input();
        assert!(ok);
        assert!(state.is_signed_in());
        assert_eq!(state.cookie_text, "");
        assert_eq!(state.session.load().as_deref(), Some("SID=secret123; HSID=secret456"));

        state.sign_out();
        assert!(!state.is_signed_in());
        assert_eq!(state.session.load(), None);
    }

    #[test]
    fn sign_in_from_input_rejects_empty_or_whitespace() {
        let mut state = wired();
        for invalid in ["", "   ", "cookie:", "COOKIE:   "] {
            state.cookie_text = invalid.to_owned();
            let ok = state.sign_in_from_input();
            assert!(!ok, "expected '{invalid}' to be rejected");
            assert!(!state.is_signed_in());
        }
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
        };
        let store = MemorySettingsStore::default();
        store.save(&custom_settings).unwrap();

        state.set_settings_store(Box::new(store));

        assert_eq!(state.theme, ThemeId::Mono);
        assert_eq!(state.locale, Locale::Spanish);
        assert_eq!(state.quality, AudioQuality::Compact);
        assert_eq!(state.audio.volume(), 64);
        assert_eq!(state.audio.applied_quality, AudioQuality::Compact);
    }
}
