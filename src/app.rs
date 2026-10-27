//! UI shell: navigation state and event delegation.
//!
//! The shell holds no business rules; every user intent is forwarded to
//! [`crate::domain`] and every side effect goes through injected adapters.

pub mod components;
pub mod pages;
pub mod theme;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use components::{render_player_bar, render_top_bar};
use pages::{render_detail, render_library, render_now_playing, render_queue, render_search};

use eframe::egui;

use crate::domain::{
    Detail, DetailKind, LyricsSnippet, LyricsSnippetError, MetadataOverride, PlaybackQueue,
    SearchQuery, SearchResult, Shelf, SkipRanges, SkipSegment, SleepTimer, ThemeId, TimerPreset,
    Track, extract_playlist_id,
};
use crate::i18n::Locale;
use crate::infra::{
    AudioBackend, AudioQuality, BackupSummary, COVER_HIRES_MAX_PX, COVER_MAX_PX, Catalog,
    CoverCache, CredentialStore, DesktopPresence, DiscordPresence, GithubUpdater, LastFmClient,
    LastFmCredentials, LyricLine, LyricOverrideDir, LyricsSource, MediaKeyAction,
    MemoryCredentialStore, MemoryMetaStore, MemoryQueueStore, MemorySettingsStore, MemorySkipStore,
    MetaStore, Presence, QueueStore, Scrobbler, SessionStore, Settings, SettingsStore, SkipStore,
    StubScrobbler, TrayAction, TrayHandle, Updater, apply_backup, client_from_credentials,
    decode_cover, decode_handoff, default_config_dir, export_backup, fetch_artwork, is_newer,
    read_backup_file, sanitize_cookie_input, should_scrobble, write_backup_file,
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

/// Maximum entries retained in the home shelves detail prefetch cache.
pub const DETAIL_PREFETCH_CAP: usize = 32;

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
    /// Text typed into the companion handoff box (UI state only).
    pub handoff_text: String,
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
    /// Optional directory for local lyric override files (*.lrc keyed by videoId).
    pub lyric_overrides: Option<LyricOverrideDir>,
    /// Whether the currently displayed lyrics were resolved from a local override file.
    pub lyrics_from_override: bool,
    /// Artwork URL for the currently rendered cover art texture.
    pub cover_key: Option<String>,
    /// Single-entry texture cache for the currently active cover art.
    pub cover_texture: Option<(String, egui::TextureHandle)>,
    pub update_status: UpdateStatus,
    pub updater: GithubUpdater,
    /// Last.fm scrobbler port for now-playing notifications and scrobbles.
    pub scrobbler: Box<dyn Scrobbler>,
    /// Optional secondary scrobbler port receiving the same playback events.
    pub secondary_scrobbler: Option<Box<dyn Scrobbler>>,
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
    /// Injected playback queue storage adapter.
    pub queue_store: Box<dyn QueueStore>,
    /// Whether radio autoplay queue continuation is active.
    pub autoplay: bool,
    /// Cached home shelves for the Search view.
    pub home_shelves: Vec<Shelf>,
    /// Active detail container view.
    pub detail: Option<Detail>,
    /// Page to restore when closing detail view.
    pub detail_return: Page,
    /// Bounded prefetch cache for shelf item details.
    pub detail_prefetch: HashMap<(String, DetailKind), Detail>,
    /// Insertion-order queue tracking keys for oldest-first eviction in `detail_prefetch`.
    pub detail_prefetch_order: VecDeque<(String, DetailKind)>,
    /// Whether any single-line text input field currently has focus.
    pub text_focused: bool,
    /// Whether global keyboard shortcuts are enabled.
    pub shortcuts_enabled: bool,
    /// Port for persisting and retrieving Last.fm credentials.
    pub credentials: Box<dyn CredentialStore>,
    /// Port for persisting and retrieving secondary Last.fm credentials.
    pub secondary_credentials: Box<dyn CredentialStore>,
    /// Whether Last.fm live scrobbling is enabled.
    pub lastfm_enabled: bool,
    /// Whether secondary Last.fm live scrobbling is enabled.
    pub secondary_lastfm_enabled: bool,
    /// Whether high-resolution cover artwork decoding is enabled.
    pub hi_res_covers: bool,
    /// Whether leading/trailing silence skipping is enabled.
    pub skip_silence: bool,
    /// Whether loudness normalization to -14 LUFS is enabled.
    pub loudness_normalization: bool,
    /// Last.fm API key input buffer (UI state only).
    pub lastfm_api_key_text: String,
    /// Last.fm API secret input buffer (UI state only).
    pub lastfm_secret_text: String,
    /// Last.fm session key input buffer (UI state only).
    pub lastfm_session_key_text: String,
    /// Secondary Last.fm API key input buffer (UI state only).
    pub secondary_lastfm_api_key_text: String,
    /// Secondary Last.fm API secret input buffer (UI state only).
    pub secondary_lastfm_secret_text: String,
    /// Secondary Last.fm session key input buffer (UI state only).
    pub secondary_lastfm_session_key_text: String,
    /// Pure sleep timer domain model.
    pub sleep_timer: SleepTimer,
    /// Last sleep timer fire marker.
    pub last_sleep_timer_fire: Option<u64>,
    /// Accumulated playback seconds from prior tracks while sleep timer is armed.
    pub sleep_timer_accumulated: u64,
    /// Elapsed seconds of active track at the moment sleep timer was armed.
    pub sleep_timer_base_offset: u64,
    /// Path entered for library backup import (UI state only).
    pub backup_path_text: String,
    /// Result or error status message from backup export or import (UI state only).
    pub backup_status_text: String,
    /// Optional backup directory override for deterministic testing.
    pub backup_dir: Option<PathBuf>,
    /// Whether lyrics search lookup mode is active.
    pub lyrics_mode: bool,
    /// Last search validation error message to display in UI.
    pub search_error: Option<String>,
    /// Last lyrics search validation error message to display in UI.
    pub lyrics_error: Option<String>,
    /// Result hits populated by the last lyrics search.
    pub lyrics_results: Vec<Track>,
    /// Result hits populated by the last search.
    pub search_results: Vec<Track>,
    /// Injected skip ranges storage adapter.
    pub skip_store: Box<dyn SkipStore>,
    /// In-memory cache of skip ranges keyed by video id.
    pub skips: HashMap<String, SkipRanges>,
    /// Pending marked segment start boundary in seconds.
    pub marked_start_secs: Option<u32>,
    /// User-facing skip validation or operation error message.
    pub skip_error: Option<String>,
    /// Last target position skipped to, preventing re-trigger loops on the same range.
    pub last_skip_target: Option<(String, u32)>,
    /// Text typed into the playlist import URL box (UI state only).
    pub playlist_url_text: String,
    /// Status message from the last playlist URL import attempt (UI state only).
    pub playlist_import_status: Option<String>,
    /// Injected metadata override storage adapter.
    pub meta_store: Box<dyn MetaStore>,
    /// In-memory cache of metadata overrides keyed by catalog track id.
    pub meta_overrides: HashMap<String, MetadataOverride>,
    /// Catalog track id currently bound to metadata edit buffers.
    pub meta_edit_track_id: Option<String>,
    /// Metadata override title input buffer (UI state only).
    pub meta_edit_title: String,
    /// Metadata override artist input buffer (UI state only).
    pub meta_edit_artist: String,
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
            handoff_text: String::new(),
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
            lyric_overrides: None,
            lyrics_from_override: false,
            cover_key: None,
            cover_texture: None,
            update_status: UpdateStatus::Unknown,
            updater: GithubUpdater::new(),
            scrobbler: Box::new(StubScrobbler::new()),
            secondary_scrobbler: None,
            credentials: Box::new(MemoryCredentialStore::new()),
            secondary_credentials: Box::new(MemoryCredentialStore::with_namespace("secondary")),
            started_at: None,
            theme: ThemeId::White,
            locale: Locale::English,
            quality: AudioQuality::Standard,
            settings_store: Box::new(MemorySettingsStore::default()),
            queue_store: Box::new(MemoryQueueStore::default()),
            autoplay: false,
            home_shelves: Vec::new(),
            detail: None,
            detail_return: Page::Search,
            detail_prefetch: HashMap::new(),
            detail_prefetch_order: VecDeque::new(),
            text_focused: false,
            shortcuts_enabled: true,
            lastfm_enabled: false,
            secondary_lastfm_enabled: false,
            hi_res_covers: false,
            skip_silence: false,
            loudness_normalization: false,
            lastfm_api_key_text: String::new(),
            lastfm_secret_text: String::new(),
            lastfm_session_key_text: String::new(),
            secondary_lastfm_api_key_text: String::new(),
            secondary_lastfm_secret_text: String::new(),
            secondary_lastfm_session_key_text: String::new(),
            sleep_timer: SleepTimer::new(),
            last_sleep_timer_fire: None,
            sleep_timer_accumulated: 0,
            sleep_timer_base_offset: 0,
            backup_path_text: String::new(),
            backup_status_text: String::new(),
            backup_dir: None,
            lyrics_mode: false,
            search_error: None,
            lyrics_error: None,
            lyrics_results: Vec::new(),
            search_results: Vec::new(),
            skip_store: Box::new(MemorySkipStore::new()),
            skips: HashMap::new(),
            marked_start_secs: None,
            skip_error: None,
            last_skip_target: None,
            playlist_url_text: String::new(),
            playlist_import_status: None,
            meta_store: Box::new(MemoryMetaStore::new()),
            meta_overrides: HashMap::new(),
            meta_edit_track_id: None,
            meta_edit_title: String::new(),
            meta_edit_artist: String::new(),
        }
    }

    /// Injects a local lyric overrides directory.
    pub fn set_lyric_overrides(&mut self, overrides: LyricOverrideDir) {
        self.lyric_overrides = Some(overrides);
    }

    /// Injects an alternative scrobbler implementation.
    pub fn set_scrobbler(&mut self, scrobbler: Box<dyn Scrobbler>) {
        self.scrobbler = scrobbler;
    }

    /// Injects an alternative secondary scrobbler implementation.
    pub fn set_secondary_scrobbler(&mut self, scrobbler: Option<Box<dyn Scrobbler>>) {
        self.secondary_scrobbler = scrobbler;
    }

    /// Injects an alternative Last.fm credential store implementation.
    pub fn set_credential_store(&mut self, store: Box<dyn CredentialStore>) {
        self.credentials = store;
        self.sync_lastfm_scrobbler();
    }

    /// Injects an alternative secondary Last.fm credential store implementation.
    pub fn set_secondary_credential_store(&mut self, store: Box<dyn CredentialStore>) {
        self.secondary_credentials = store;
        self.sync_secondary_lastfm_scrobbler();
    }

    /// Injects an alternative settings store implementation and synchronizes state.
    pub fn set_settings_store(&mut self, store: Box<dyn SettingsStore>) {
        let loaded = store.load();
        self.theme = loaded.theme;
        self.locale = loaded.locale;
        self.quality = loaded.quality;
        self.shortcuts_enabled = loaded.keyboard_shortcuts;
        self.lastfm_enabled = loaded.lastfm_enabled;
        self.secondary_lastfm_enabled = loaded.secondary_lastfm_enabled;
        self.hi_res_covers = loaded.hi_res_covers;
        self.skip_silence = loaded.skip_silence;
        self.loudness_normalization = loaded.loudness_normalization;
        self.audio.set_volume(loaded.volume);
        self.audio.set_quality(loaded.quality);
        self.audio.set_skip_silence(loaded.skip_silence);
        self.audio.set_loudness_normalization(loaded.loudness_normalization);
        self.settings_store = store;
        self.sync_lastfm_scrobbler();
        self.sync_secondary_lastfm_scrobbler();
    }

    /// Injects an alternative skip store implementation and loads persisted ranges into state.
    pub fn set_skip_store(&mut self, store: Box<dyn SkipStore>) {
        self.skips = store.load();
        self.skip_store = store;
    }

    fn persist_skips(&self) {
        if let Err(err) = self.skip_store.save(&self.skips) {
            tracing::warn!("failed to persist skips: {err:#}");
        }
    }

    /// Injects an alternative metadata store implementation and synchronizes state.
    pub fn set_meta_store(&mut self, store: Box<dyn MetaStore>) {
        self.meta_overrides = store.load();
        self.meta_store = store;
    }

    /// Saves a metadata override patch for `track_id` and persists to storage.
    pub fn save_meta_override(
        &mut self,
        track_id: &str,
        patch: MetadataOverride,
    ) -> anyhow::Result<()> {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            anyhow::bail!("track id cannot be blank");
        }
        self.meta_overrides.insert(trimmed.to_owned(), patch);
        if let Err(err) = self.meta_store.save(&self.meta_overrides) {
            tracing::warn!("failed to persist metadata override: {err:#}");
            return Err(err);
        }
        Ok(())
    }

    /// Clears any metadata override for `track_id` and persists clearance to storage.
    pub fn clear_meta_override(&mut self, track_id: &str) {
        let trimmed = track_id.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.meta_overrides.remove(trimmed).is_some() {
            if let Err(err) = self.meta_store.save(&self.meta_overrides) {
                tracing::warn!("failed to persist metadata override clearance: {err:#}");
            }
        }
    }

    /// Resolves track title and artist for DISPLAY ONLY.
    ///
    /// Scope: now-playing current-track header and queue rows.
    /// All identity paths (likes, lyrics key + lookup args, scrobble args, desktop/discord
    /// presence, queue persistence, backup, and handoff) MUST keep canonical `track.artist`
    /// and `track.title` and must NOT route through metadata overrides.
    pub fn resolved_track_text(&self, track: &Track) -> (String, String) {
        let Some(meta) = self.meta_overrides.get(&track.id) else {
            return (track.title.clone(), track.artist.clone());
        };
        let (title, artist) = meta.resolve(track);
        (title.into_owned(), artist.into_owned())
    }

    /// Synchronizes UI edit buffers with resolved text for `track`.
    pub fn sync_meta_edit_buffers(&mut self, track: &Track) {
        if self.meta_edit_track_id.as_deref() != Some(&track.id) {
            let (title, artist) = self.resolved_track_text(track);
            self.meta_edit_title = title;
            self.meta_edit_artist = artist;
            self.meta_edit_track_id = Some(track.id.clone());
        }
    }

    /// Saves metadata override from UI edit buffers for `track`.
    ///
    /// Blank input fields mean "keep canonical for that field".
    /// If both fields are blank, any existing override is cleared.
    pub fn save_meta_override_from_input(&mut self, track: &Track) {
        let title_opt = sanitize_input_field(&self.meta_edit_title);
        let artist_opt = sanitize_input_field(&self.meta_edit_artist);
        if title_opt.is_none() && artist_opt.is_none() {
            self.clear_meta_override_and_reset(track);
            return;
        }
        let Ok(patch) = MetadataOverride::new(title_opt.as_deref(), artist_opt.as_deref()) else {
            return;
        };
        let _ = self.save_meta_override(&track.id, patch);
        let (title, artist) = self.resolved_track_text(track);
        self.meta_edit_title = title;
        self.meta_edit_artist = artist;
    }

    /// Clears metadata override for `track` and resets UI edit buffers to canonical text.
    pub fn clear_meta_override_and_reset(&mut self, track: &Track) {
        self.clear_meta_override(&track.id);
        let (title, artist) = self.resolved_track_text(track);
        self.meta_edit_title = title;
        self.meta_edit_artist = artist;
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

    /// Sets whether secondary Last.fm live scrobbling is enabled and persists the preference.
    pub fn set_secondary_lastfm_enabled(&mut self, enabled: bool) {
        self.secondary_lastfm_enabled = enabled;
        self.persist_settings();
        self.sync_secondary_lastfm_scrobbler();
    }

    fn sync_secondary_lastfm_scrobbler(&mut self) {
        if !self.secondary_lastfm_enabled {
            self.secondary_scrobbler = None;
            return;
        }
        let Some(creds) = self.secondary_credentials.load_lastfm() else {
            self.secondary_scrobbler = None;
            return;
        };
        if !creds.is_complete() {
            self.secondary_scrobbler = None;
            return;
        }
        self.secondary_scrobbler = Some(Box::new(client_from_credentials(&creds)));
    }

    /// Saves secondary Last.fm credentials to the vault and synchronizes secondary scrobbler state.
    pub fn save_secondary_lastfm_credentials(&mut self, creds: &LastFmCredentials) {
        self.secondary_credentials.save_lastfm(creds);
        self.sync_secondary_lastfm_scrobbler();
    }

    /// Clears persisted secondary Last.fm credentials from the vault, input buffers, and resets secondary scrobbler to None.
    pub fn clear_secondary_lastfm_credentials(&mut self) {
        self.secondary_credentials.clear_lastfm();
        self.secondary_lastfm_api_key_text.clear();
        self.secondary_lastfm_secret_text.clear();
        self.secondary_lastfm_session_key_text.clear();
        self.sync_secondary_lastfm_scrobbler();
    }

    /// Saves secondary credentials from UI input buffers and clears buffers on completion.
    pub fn save_secondary_lastfm_credentials_from_input(&mut self) -> bool {
        let creds = LastFmCredentials::new(
            &self.secondary_lastfm_api_key_text,
            &self.secondary_lastfm_secret_text,
            &self.secondary_lastfm_session_key_text,
        );
        let complete = creds.is_complete();
        self.save_secondary_lastfm_credentials(&creds);
        self.secondary_lastfm_api_key_text.clear();
        self.secondary_lastfm_secret_text.clear();
        self.secondary_lastfm_session_key_text.clear();
        complete
    }

    /// Returns true if the secondary scrobbler has a live authenticated session.
    pub fn secondary_lastfm_is_live(&self) -> bool {
        self.secondary_scrobbler
            .as_ref()
            .and_then(|s| s.as_any().downcast_ref::<LastFmClient>())
            .is_some_and(LastFmClient::is_live)
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

    /// Injects an alternative queue store implementation and restores the queue.
    pub fn set_queue_store(&mut self, store: Box<dyn QueueStore>) {
        self.queue_store = store;
        self.restore_queue();
    }

    /// Restores the playback queue from storage, clamping cursor to valid bounds.
    pub fn restore_queue(&mut self) {
        let mut queue = self.queue_store.load();
        queue.clamp_cursor();
        self.queue = queue;
    }

    /// Persists the active playback queue to storage, logging errors without panicking.
    pub fn save_queue(&self) {
        if let Err(err) = self.queue_store.save(&self.queue) {
            tracing::warn!("failed to persist queue: {err:#}");
        }
    }

    /// Mutates the playback queue in-place and persists the updated queue.
    ///
    /// Persistence policy (SDD FR-13):
    /// All queue mutations pass through `mutate_queue`, which immediately persists the queue
    /// to `queue_store` (via atomic tmp+rename writes on disk). This ensures crash resilience
    /// without relying on graceful exit hooks or risking lost updates during unexpected termination.
    pub fn mutate_queue<R>(&mut self, f: impl FnOnce(&mut PlaybackQueue) -> R) -> R {
        let result = f(&mut self.queue);
        self.save_queue();
        result
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

    /// Sets whether skip-silence processing is enabled, forwards to audio backend, and persists.
    pub fn set_skip_silence(&mut self, enabled: bool) {
        self.skip_silence = enabled;
        self.audio.set_skip_silence(enabled);
        self.persist_settings();
    }

    /// Sets whether loudness normalization is enabled, forwards to audio backend, and persists.
    pub fn set_loudness_normalization(&mut self, enabled: bool) {
        self.loudness_normalization = enabled;
        self.audio.set_loudness_normalization(enabled);
        self.persist_settings();
    }

    fn persist_settings(&self) {
        let settings = Settings {
            theme: self.theme,
            locale: self.locale,
            volume: self.audio.volume(),
            quality: self.quality,
            keyboard_shortcuts: self.shortcuts_enabled,
            lastfm_enabled: self.lastfm_enabled,
            secondary_lastfm_enabled: self.secondary_lastfm_enabled,
            hi_res_covers: self.hi_res_covers,
            skip_silence: self.skip_silence,
            loudness_normalization: self.loudness_normalization,
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
    /// Hits are served from `detail_prefetch` without invoking the catalog adapter.
    pub fn open_detail(&mut self, browse_id: &str, kind: DetailKind) {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.page != Page::Detail {
            self.detail_return = self.page;
        }
        let key = (trimmed.to_owned(), kind);
        if let Some(cached) = self.detail_prefetch.get(&key) {
            self.detail = Some(cached.clone());
            self.page = Page::Detail;
            return;
        }
        self.detail = self.catalog.detail(trimmed, kind);
        self.page = Page::Detail;
    }

    /// Prefetches detail content into the bounded prefetch cache if not already present.
    /// Blank identifiers and failed fetches are ignored and not cached.
    pub fn prefetch_detail(&mut self, browse_id: &str, kind: DetailKind) {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return;
        }
        let key = (trimmed.to_owned(), kind);
        if self.detail_prefetch.contains_key(&key) {
            return;
        }
        let Some(detail) = self.catalog.detail(trimmed, kind) else {
            return;
        };
        self.insert_prefetched_detail(key, detail);
    }

    fn insert_prefetched_detail(&mut self, key: (String, DetailKind), detail: Detail) {
        while self.detail_prefetch.len() >= DETAIL_PREFETCH_CAP {
            let Some(oldest) = self.detail_prefetch_order.pop_front() else {
                break;
            };
            self.detail_prefetch.remove(&oldest);
        }
        self.detail_prefetch_order.push_back(key.clone());
        self.detail_prefetch.insert(key, detail);
    }

    /// Closes the detail view and returns to the previous page.
    pub fn close_detail(&mut self) {
        self.page = self.detail_return;
        self.detail = None;
    }

    /// Sets whether lyrics search lookup mode is enabled.
    pub fn set_lyrics_mode(&mut self, enabled: bool) {
        self.lyrics_mode = enabled;
        self.search_error = None;
        self.lyrics_error = None;
    }

    /// Returns whether lyrics search lookup mode is enabled.
    pub fn is_lyrics_mode(&self) -> bool {
        self.lyrics_mode
    }

    fn record_lyrics_error(&mut self, err: LyricsSnippetError) {
        let msg = err.to_string();
        self.search_error = Some(msg.clone());
        self.lyrics_error = Some(msg);
        self.lyrics_results.clear();
        self.search_results.clear();
    }

    /// Handles a search submission: routes to lyrics search when lyrics mode is active,
    /// otherwise parses standard query, queries the catalog, and enqueues the first hit.
    /// Enqueues a single track at the tail of the playback queue.
    pub fn enqueue_track(&mut self, track: Track) {
        self.queue.push(track);
    }

    /// Imports a playlist from a URL and enqueues all tracks to the playback queue.
    ///
    /// Resolves the canonical playlist identifier, fetches playlist detail from the
    /// catalog, and enqueues tracks through the queue-mutation choke point. Returns
    /// the number of enqueued tracks. Empty-track playlists enqueue zero tracks and
    /// report zero without error. Parse and lookup failures return loud errors.
    pub fn import_playlist_url(&mut self, url: &str) -> anyhow::Result<usize> {
        let playlist_id = extract_playlist_id(url)
            .ok_or_else(|| anyhow::anyhow!("invalid or unsupported playlist URL: '{url}'"))?;
        let detail = self
            .catalog
            .detail(&playlist_id, DetailKind::Playlist)
            .ok_or_else(|| anyhow::anyhow!("playlist detail not found: '{playlist_id}'"))?;
        let count = detail.tracks.len();
        for track in detail.tracks {
            self.enqueue_track(track);
        }
        Ok(count)
    }

    pub fn submit_search(&mut self) {
        if self.lyrics_mode {
            self.submit_lyrics_search();
            return;
        }
        self.search_error = None;
        self.lyrics_error = None;
        let Some(query) = SearchQuery::parse(&self.search_text) else {
            return;
        };
        let hits = self.catalog.search(query.text());
        self.search_results = hits.clone();
        if let Some(track) = hits.into_iter().next() {
            self.queue.push(track);
            self.page = Page::Queue;
        }
    }

    /// Submits a lyrics-based search query using `search_text`.
    /// Surfaces validation errors on blank or overlong input without panicking.
    pub fn submit_lyrics_search(&mut self) {
        let snippet = match LyricsSnippet::parse(&self.search_text) {
            Ok(s) => s,
            Err(err) => return self.record_lyrics_error(err),
        };
        self.search_error = None;
        self.lyrics_error = None;
        let hits = self.catalog.search_by_lyrics(&snippet);
        self.lyrics_results = hits.clone();
        self.search_results = hits.clone();
        if let Some(track) = hits.into_iter().next() {
            self.mutate_queue(|q| q.push(track));
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
        let seed_key = seed.id.clone();
        self.mutate_queue(|q| {
            q.clear();
            q.push(seed);
            for track in radio_tracks {
                let trimmed = track.id.trim();
                if !trimmed.is_empty() && trimmed != seed_key {
                    q.push(track);
                }
            }
        });
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
        self.mutate_queue(|q| {
            for track in recommendations {
                let trimmed = track.id.trim();
                if trimmed.is_empty() || q.tracks().iter().any(|t| t.id == trimmed) {
                    continue;
                }
                q.push(track);
                appended = true;
            }
            if appended {
                q.advance();
            }
        });
        if appended {
            self.accumulate_sleep_timer_on_advance();
            self.play_current_quiet();
            self.notify_sleep_timer_track_changed();
        }
    }

    fn forward_now_playing(&mut self, artist: &str, title: &str) {
        self.scrobbler.now_playing(artist, title);
        if let Some(sec) = self.secondary_scrobbler.as_mut() {
            sec.now_playing(artist, title);
        }
    }

    fn forward_scrobble(&mut self, artist: &str, title: &str, timestamp: u64) {
        self.scrobbler.scrobble(artist, title, timestamp);
        if let Some(sec) = self.secondary_scrobbler.as_mut() {
            sec.scrobble(artist, title, timestamp);
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
        self.forward_now_playing(&artist, &title);
        self.last_skip_target = None;
    }

    /// Evaluates per-frame auto-skip, advancing audio position when strictly inside a range.
    pub fn check_auto_skip(&mut self) {
        let Some(track) = self.queue.current() else {
            self.last_skip_target = None;
            return;
        };
        let current_pos = u32::try_from(self.audio.position_secs()).unwrap_or(u32::MAX);
        if self
            .last_skip_target
            .as_ref()
            .is_some_and(|(id, t)| id != &track.id || current_pos >= *t)
        {
            self.last_skip_target = None;
        }
        let Some(ranges) = self.skips.get(&track.id) else {
            return;
        };
        let Some(target) = ranges.jump_target(current_pos) else {
            return;
        };
        let already_skipped =
            self.last_skip_target.as_ref().is_some_and(|(id, t)| id == &track.id && *t == target);
        if target > current_pos && !already_skipped {
            self.last_skip_target = Some((track.id.clone(), target));
            self.audio.seek(target as u64);
        }
    }

    /// Captures the current audio playback position as the segment start.
    pub fn mark_segment_start(&mut self) {
        let pos = u32::try_from(self.audio.position_secs()).unwrap_or(u32::MAX);
        self.mark_segment_start_at(pos);
    }

    /// Sets the segment start boundary explicitly (for testing and exact positioning).
    pub fn mark_segment_start_at(&mut self, pos_secs: u32) {
        self.marked_start_secs = Some(pos_secs);
        self.skip_error = None;
    }

    /// Captures current playback position as segment end and stores the range when valid.
    pub fn mark_segment_end(&mut self) -> bool {
        let pos = u32::try_from(self.audio.position_secs()).unwrap_or(u32::MAX);
        self.mark_segment_end_at(pos)
    }

    /// Sets segment end boundary explicitly and stores range when valid.
    pub fn mark_segment_end_at(&mut self, pos_secs: u32) -> bool {
        let Some(track) = self.queue.current() else {
            self.skip_error = Some("No active track".to_string());
            return false;
        };
        let Some(start) = self.marked_start_secs else {
            self.skip_error = Some("Mark start first".to_string());
            return false;
        };
        let Ok(segment) = SkipSegment::new(start, pos_secs) else {
            self.skip_error = Some("End must be greater than start".to_string());
            return false;
        };
        let track_id = track.id.clone();
        self.skips.entry(track_id).or_default().insert(segment);
        self.persist_skips();
        self.marked_start_secs = None;
        self.skip_error = None;
        true
    }

    /// Deletes a stored range at `index` for `track_id`, persisting changes.
    pub fn delete_segment(&mut self, track_id: &str, index: usize) -> bool {
        let Some(ranges) = self.skips.get_mut(track_id) else {
            return false;
        };
        let removed = ranges.remove(index).is_some();
        if removed {
            self.persist_skips();
        }
        removed
    }

    /// Returns stored skip ranges for the currently playing track.
    pub fn current_skip_ranges(&self) -> Option<&SkipRanges> {
        let track = self.queue.current()?;
        self.skips.get(&track.id)
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
        self.forward_scrobble(&artist, &title, timestamp);
    }

    /// Configures the active sleep timer preset, resetting accumulated timer elapsed.
    pub fn set_sleep_timer_preset(&mut self, preset: TimerPreset) {
        let current_track_id = self.queue.current().map(|t| t.id.as_str());
        self.sleep_timer.set_preset_with_track(preset, current_track_id);
        self.sleep_timer_accumulated = 0;
        self.sleep_timer_base_offset =
            self.started_at.map_or(0, |(inst, _)| inst.elapsed().as_secs());
        self.last_sleep_timer_fire = None;
    }

    /// Disarms the active sleep timer and clears timer bookkeeping.
    pub fn cancel_sleep_timer(&mut self) {
        self.sleep_timer.cancel();
        self.sleep_timer_accumulated = 0;
        self.sleep_timer_base_offset = 0;
        self.last_sleep_timer_fire = None;
    }

    /// Evaluates the active sleep timer against playback elapsed time.
    pub fn poll_sleep_timer(&mut self) {
        if !self.sleep_timer.is_armed() {
            return;
        }
        let track_elapsed = self.started_at.map_or(0, |(inst, _)| inst.elapsed().as_secs());
        let current = track_elapsed.saturating_sub(self.sleep_timer_base_offset);
        let total = self.sleep_timer_accumulated.saturating_add(current);
        self.poll_sleep_timer_with_elapsed(total);
    }

    pub(crate) fn poll_sleep_timer_with_elapsed(&mut self, elapsed_secs: u64) {
        if !self.sleep_timer.poll_elapsed(elapsed_secs) {
            return;
        }
        self.last_sleep_timer_fire = Some(elapsed_secs);
        self.audio.pause();
    }

    fn accumulate_sleep_timer_on_advance(&mut self) {
        let track_elapsed = self.started_at.map_or(0, |(inst, _)| inst.elapsed().as_secs());
        let current = track_elapsed.saturating_sub(self.sleep_timer_base_offset);
        self.sleep_timer_accumulated = self.sleep_timer_accumulated.saturating_add(current);
        self.sleep_timer_base_offset = 0;
    }

    fn notify_sleep_timer_track_changed(&mut self) {
        let Some(track) = self.queue.current() else {
            return;
        };
        let id = track.id.clone();
        if !self.sleep_timer.notify_track_changed(&id) {
            return;
        }
        self.last_sleep_timer_fire = Some(self.audio.position_secs());
        self.audio.pause();
    }

    /// Advances the queue cursor and deterministically starts playback of the new head.
    pub fn next_track(&mut self) {
        self.maybe_scrobble_current();
        self.accumulate_sleep_timer_on_advance();
        self.mutate_queue(|q| q.advance());
        self.play_current_quiet();
        self.notify_sleep_timer_track_changed();
    }

    /// Retreats the queue cursor and deterministically starts playback of the new head.
    pub fn prev_track(&mut self) {
        self.maybe_scrobble_current();
        self.accumulate_sleep_timer_on_advance();
        self.mutate_queue(|q| q.retreat());
        self.play_current_quiet();
        self.notify_sleep_timer_track_changed();
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

    fn apply_session(&mut self, cleaned: &str) {
        self.session.save(cleaned);
        self.catalog.set_session_cookies(Some(cleaned));
        self.signed_in = true;
    }

    /// Attempts to parse and save the current cookie input.
    ///
    /// Returns true and clears the buffer if valid, otherwise returns false.
    pub fn sign_in_from_input(&mut self) -> bool {
        let Some(cleaned) = sanitize_cookie_input(&self.cookie_text) else {
            return false;
        };
        self.apply_session(&cleaned);
        self.cookie_text.clear();
        true
    }

    /// Attempts to decode and import a companion handoff code.
    ///
    /// Returns true and clears the buffer if valid, otherwise returns false.
    pub fn import_handoff(&mut self) -> bool {
        let Ok(cleaned) = decode_handoff(&self.handoff_text) else {
            return false;
        };
        self.apply_session(&cleaned);
        self.handoff_text.clear();
        true
    }

    /// Clears the stored session and resets cookie and handoff input buffers.
    pub fn sign_out(&mut self) {
        self.session.clear();
        self.catalog.set_session_cookies(None);
        self.signed_in = false;
        self.cookie_text.clear();
        self.handoff_text.clear();
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

    /// Exports the current library into a JSON backup file in the backup directory.
    pub fn export_library_backup(&mut self) -> anyhow::Result<PathBuf> {
        let dir = self.backup_dir.clone().unwrap_or_else(default_config_dir);
        self.export_library_backup_to(&dir)
    }

    /// Exports the current library into `dir` using caller-supplied directory.
    pub fn export_library_backup_to(&mut self, dir: &Path) -> anyhow::Result<PathBuf> {
        let timestamp = current_unix_secs();
        let backup = export_backup(&self.catalog, timestamp);
        let res = write_backup_file(dir, &backup);
        self.record_export_status(&res);
        res
    }

    fn record_export_status(&mut self, res: &anyhow::Result<PathBuf>) {
        match res {
            Ok(path) => self.backup_status_text = format!("Exported to: {}", path.display()),
            Err(err) => self.backup_status_text = format!("Export error: {err:#}"),
        }
    }

    /// Imports a library backup from the given filesystem `path`.
    pub fn import_library_backup(&mut self, path: &str) -> anyhow::Result<BackupSummary> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            let err = anyhow::anyhow!("backup path cannot be blank");
            self.backup_status_text = format!("Import error: {err}");
            return Err(err);
        }
        let res = self.execute_backup_import(trimmed);
        self.record_import_status(&res);
        res
    }

    fn execute_backup_import(&mut self, path: &str) -> anyhow::Result<BackupSummary> {
        let backup = read_backup_file(Path::new(path))?;
        let summary = apply_backup(&mut self.catalog, &backup);
        Ok(summary)
    }

    fn record_import_status(&mut self, res: &anyhow::Result<BackupSummary>) {
        match res {
            Ok(s) => {
                self.backup_status_text = format!(
                    "Imported: {} liked, {} playlists, {} albums",
                    s.liked_applied, s.playlists_recorded, s.albums_recorded
                );
            }
            Err(err) => {
                self.backup_status_text = format!("Import error: {err:#}");
            }
        }
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
    /// Override content refreshes on track change like provider content (no file watcher).
    pub fn refresh_lyrics(&mut self) {
        let Some(track) = self.queue.current() else {
            if self.lyrics_key.is_some() {
                self.lyrics_key = None;
                self.lyrics_lines.clear();
                self.lyrics_from_override = false;
            }
            return;
        };
        let key = (track.artist.clone(), track.title.clone());
        if self.lyrics_key.as_ref() == Some(&key) {
            return;
        }
        let override_hit = self.lyric_overrides.as_ref().and_then(|dir| dir.load(&track.id));
        if let Some(override_lyrics) = override_hit {
            self.lyrics_lines = override_lyrics.lines;
            self.lyrics_from_override = true;
        } else {
            let duration = track.duration_secs.map(u64::from);
            self.lyrics_lines =
                self.lyrics.lookup(&key.0, &key.1, duration).map(|s| s.lines).unwrap_or_default();
            self.lyrics_from_override = false;
        }
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

fn sanitize_input_field(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
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
            self.state.save_queue();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.state.sync_presence();
        self.state.continue_autoplay_if_ended();
        self.state.poll_sleep_timer();
        self.state.check_auto_skip();
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

pub(crate) fn item_detail_target(item: &SearchResult) -> Option<(String, DetailKind)> {
    match item {
        SearchResult::Album(album) => Some((album.id.clone(), DetailKind::Album)),
        SearchResult::Artist(artist) => Some((artist.id.clone(), DetailKind::Artist)),
        SearchResult::Playlist(playlist) => Some((playlist.id.clone(), DetailKind::Playlist)),
        SearchResult::Track(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::DiskMetaStore;

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
    use crate::domain::{
        Album, Artist, Detail, DetailKind, Playlist, RepeatMode, SearchQuery, SearchResult, Shelf,
        Track,
    };
    use crate::infra::{
        AudioStub, CatalogStub, LrclibStub, LyricLine, MemoryCoverCache, MemorySessionStore,
        ReleaseInfo, StaticUpdater, SyncedLyrics, Updater, encode_handoff,
    };

    #[derive(Default)]
    struct CountingCatalog {
        inner: CatalogStub,
        home_calls: std::sync::atomic::AtomicUsize,
        detail_calls: std::sync::atomic::AtomicUsize,
        fail_detail: std::sync::atomic::AtomicBool,
    }

    impl Catalog for CountingCatalog {
        fn search(&self, query: &str) -> Vec<Track> {
            self.inner.search(query)
        }
        fn search_filtered(&self, query: &SearchQuery) -> Vec<SearchResult> {
            self.inner.search_filtered(query)
        }
        fn search_by_lyrics(&self, snippet: &LyricsSnippet) -> Vec<Track> {
            self.inner.search_by_lyrics(snippet)
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
            self.detail_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.fail_detail.load(std::sync::atomic::Ordering::SeqCst) {
                return None;
            }
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
        state.handoff_text = "leftover_handoff".to_owned();
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
        assert_eq!(state.handoff_text, "");
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
    fn import_handoff_success_applies_session_and_clears_buffer() {
        let mut state = wired();
        assert!(!state.is_signed_in());
        assert_eq!(state.catalog.last_session_cookies(), None);

        let code = encode_handoff("cookie: SID=companion123; HSID=companion456").expect("encode");
        state.handoff_text = code;

        let ok = state.import_handoff();
        assert!(ok);
        assert!(state.is_signed_in());
        assert_eq!(state.handoff_text, "");
        assert_eq!(state.session.load().as_deref(), Some("SID=companion123; HSID=companion456"));
        assert_eq!(
            state.catalog.last_session_cookies(),
            Some("SID=companion123; HSID=companion456")
        );

        state.sign_out();
        assert!(!state.is_signed_in());
        assert_eq!(state.session.load(), None);
        assert_eq!(state.catalog.last_session_cookies(), None);
        assert_eq!(state.handoff_text, "");
        assert_eq!(state.cookie_text, "");
    }

    #[test]
    fn import_handoff_failure_leaves_state_untouched() {
        let mut state = wired();
        let invalid_cases = [
            "",
            "   ",
            "MG1.invalid_base64!.12345678",
            "corrupt_handoff_envelope",
            "MG2.Zm9v.12345678",
        ];

        for invalid in invalid_cases {
            state.handoff_text = invalid.to_owned();
            let ok = state.import_handoff();
            assert!(!ok, "expected '{invalid}' to fail");
            assert!(!state.is_signed_in());
            assert_eq!(state.catalog.last_session_cookies(), None);
            assert_eq!(state.session.load(), None);
            assert_eq!(state.handoff_text, invalid);
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

    type RecordedLookups = Vec<(String, String, Option<u64>)>;

    #[derive(Default, Clone)]
    struct TrackingLyricsSource {
        recorded: std::sync::Arc<std::sync::Mutex<RecordedLookups>>,
    }

    impl LyricsSource for TrackingLyricsSource {
        fn lookup(
            &self,
            artist: &str,
            title: &str,
            duration_secs: Option<u64>,
        ) -> Option<SyncedLyrics> {
            if let Ok(mut lock) = self.recorded.lock() {
                lock.push((artist.to_owned(), title.to_owned(), duration_secs));
            }
            None
        }
    }

    #[test]
    fn refresh_lyrics_forwards_track_duration_to_lyrics_source() {
        let lyrics = TrackingLyricsSource::default();
        let recorded = lyrics.recorded.clone();
        let mut state = AppState::new(
            CatalogStub::new(),
            AudioStub::new(),
            MemorySessionStore::new(),
            lyrics,
            MemoryCoverCache::new(),
        );

        // Track with duration passes Some(secs)
        let track_with_dur = Track::new("t1", "Paranoid Android", "Radiohead").with_duration(383);
        state.queue.push(track_with_dur);
        state.refresh_lyrics();

        // Advance to track without duration passes None
        let track_without_dur = Track::new("t2", "Karma Police", "Radiohead");
        state.queue.push(track_without_dur);
        state.queue.advance();
        state.refresh_lyrics();

        let entries = recorded.lock().unwrap().clone();
        assert_eq!(
            entries,
            vec![
                ("Radiohead".into(), "Paranoid Android".into(), Some(383)),
                ("Radiohead".into(), "Karma Police".into(), None),
            ]
        );
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
            secondary_lastfm_enabled: false,
            hi_res_covers: false,
            skip_silence: false,
            loudness_normalization: false,
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
    fn prefetch_detail_fetches_once_and_deduplicates_repeated_intent() {
        let catalog = CountingCatalog::default();
        let mut state = AppState::new(
            catalog,
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );

        assert_eq!(state.catalog.detail_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        state.prefetch_detail("alb-1", DetailKind::Album);
        assert_eq!(state.catalog.detail_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(state.detail_prefetch.contains_key(&("alb-1".into(), DetailKind::Album)));

        // Second intent for the exact same target does not make a new catalog call
        state.prefetch_detail("alb-1", DetailKind::Album);
        assert_eq!(state.catalog.detail_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn open_detail_serves_from_prefetch_cache_without_catalog_call() {
        let catalog = CountingCatalog::default();
        let mut state = AppState::new(
            catalog,
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );

        state.prefetch_detail("alb-1", DetailKind::Album);
        assert_eq!(state.catalog.detail_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let cached = state
            .detail_prefetch
            .get(&("alb-1".into(), DetailKind::Album))
            .cloned()
            .expect("prefetched detail present");

        state.open_detail("alb-1", DetailKind::Album);
        // Zero new catalog calls on open_detail hit
        assert_eq!(state.catalog.detail_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(state.page, Page::Detail);
        assert_eq!(state.detail.as_ref(), Some(&cached));
    }

    #[test]
    fn prefetch_detail_blank_id_never_fetches() {
        let catalog = CountingCatalog::default();
        let mut state = AppState::new(
            catalog,
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );

        state.prefetch_detail("", DetailKind::Album);
        state.prefetch_detail("   ", DetailKind::Artist);
        state.prefetch_detail("\t\n", DetailKind::Playlist);
        assert_eq!(state.catalog.detail_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(state.detail_prefetch.is_empty());
    }

    #[test]
    fn prefetch_detail_failed_fetch_is_not_cached_and_retries() {
        let catalog = CountingCatalog::default();
        let mut state = AppState::new(
            catalog,
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );

        state.catalog.fail_detail.store(true, std::sync::atomic::Ordering::SeqCst);
        state.prefetch_detail("alb-fail", DetailKind::Album);
        assert_eq!(state.catalog.detail_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(!state.detail_prefetch.contains_key(&("alb-fail".into(), DetailKind::Album)));

        // Unset failure flag: subsequent intent retries and successfully caches
        state.catalog.fail_detail.store(false, std::sync::atomic::Ordering::SeqCst);
        state.prefetch_detail("alb-fail", DetailKind::Album);
        assert_eq!(state.catalog.detail_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(state.detail_prefetch.contains_key(&("alb-fail".into(), DetailKind::Album)));
    }

    #[test]
    fn prefetch_detail_cap_eviction_drops_oldest_entry() {
        let catalog = CountingCatalog::default();
        let mut state = AppState::new(
            catalog,
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );

        // Fill cache up to cap
        for i in 0..DETAIL_PREFETCH_CAP {
            state.prefetch_detail(&format!("item-{i}"), DetailKind::Album);
        }
        assert_eq!(state.detail_prefetch.len(), DETAIL_PREFETCH_CAP);
        assert!(state.detail_prefetch.contains_key(&("item-0".into(), DetailKind::Album)));

        // Prefetching one more item exceeds cap and evicts oldest entry (item-0)
        state.prefetch_detail("item-overflow", DetailKind::Album);
        assert_eq!(state.detail_prefetch.len(), DETAIL_PREFETCH_CAP);
        assert!(!state.detail_prefetch.contains_key(&("item-0".into(), DetailKind::Album)));
        assert!(state.detail_prefetch.contains_key(&("item-1".into(), DetailKind::Album)));
        assert!(state.detail_prefetch.contains_key(&("item-overflow".into(), DetailKind::Album)));
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
                secondary_lastfm_enabled: false,
                hi_res_covers: false,
                skip_silence: false,
                loudness_normalization: false,
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

    #[test]
    fn test_app_state_queue_persistence_across_instances() {
        let store = MemoryQueueStore::new();

        // Instance 1: populate queue, advance cursor, set repeat mode
        let mut state1 = wired();
        state1.set_queue_store(Box::new(store.clone()));

        state1.mutate_queue(|q| {
            q.push(Track::new("track-1", "Track 1", "Artist 1"));
            q.push(Track::new("track-2", "Track 2", "Artist 2"));
            q.push(Track::new("track-3", "Track 3", "Artist 3"));
            q.advance();
            q.set_repeat(RepeatMode::All);
        });

        assert_eq!(state1.queue.len(), 3);
        assert_eq!(state1.queue.cursor(), 1);
        assert_eq!(state1.queue.repeat(), RepeatMode::All);

        // Instance 2: new state instance injected with the same store
        let mut state2 = wired();
        state2.set_queue_store(Box::new(store));

        assert_eq!(state2.queue.len(), 3);
        assert_eq!(state2.queue.cursor(), 1);
        assert_eq!(state2.queue.repeat(), RepeatMode::All);
        assert_eq!(state2.queue.tracks(), state1.queue.tracks());
    }

    #[test]
    fn test_app_state_queue_mutations_persist_automatically() {
        let store = MemoryQueueStore::new();
        let mut state1 = wired();
        state1.set_queue_store(Box::new(store.clone()));

        state1.search_text = "midnight".to_owned();
        state1.submit_search();
        state1.search_text = "sunrise".to_owned();
        state1.submit_search();

        assert_eq!(state1.queue.len(), 2);
        state1.next_track();
        assert_eq!(state1.queue.cursor(), 1);

        let mut state2 = wired();
        state2.set_queue_store(Box::new(store));
        assert_eq!(state2.queue.len(), 2);
        assert_eq!(state2.queue.cursor(), 1);
        assert_eq!(state2.queue.current().map(|t| t.title.as_str()), Some("sunrise"));
    }

    #[test]
    fn sleep_timer_injected_elapsed_pauses_and_counts_down() {
        let mut state = wired();
        state.queue.push(Track::new("vid-1", "Karma Police", "Radiohead"));
        state.play_current_quiet();
        assert!(state.audio.is_playing());

        state.set_sleep_timer_preset(TimerPreset::Minutes15);
        assert!(state.sleep_timer.is_armed());
        assert_eq!(state.sleep_timer.remaining_secs(), Some(900));

        // Partway through interval, playback continues and remaining counts down
        state.poll_sleep_timer_with_elapsed(500);
        assert!(state.audio.is_playing());
        assert_eq!(state.sleep_timer.remaining_secs(), Some(400));

        // Just before expiry, still playing
        state.poll_sleep_timer_with_elapsed(899);
        assert!(state.audio.is_playing());
        assert_eq!(state.sleep_timer.remaining_secs(), Some(1));

        // Exactly at expiry, playback pauses and remaining reaches 0
        state.poll_sleep_timer_with_elapsed(900);
        assert!(!state.audio.is_playing());
        assert_eq!(state.sleep_timer.remaining_secs(), Some(0));
        assert_eq!(state.last_sleep_timer_fire, Some(900));

        // Subsequent poll remains silent and stays paused
        state.poll_sleep_timer_with_elapsed(901);
        assert!(!state.audio.is_playing());
    }

    #[test]
    fn sleep_timer_cancel_disarms_in_app_state() {
        let mut state = wired();
        state.set_sleep_timer_preset(TimerPreset::Minutes30);
        assert!(state.sleep_timer.is_armed());
        assert_eq!(state.sleep_timer.remaining_secs(), Some(1800));

        state.cancel_sleep_timer();
        assert!(!state.sleep_timer.is_armed());
        assert_eq!(state.sleep_timer.preset(), TimerPreset::Off);
        assert_eq!(state.sleep_timer.remaining_secs(), None);
    }

    #[test]
    fn sleep_timer_stop_after_track_pauses_on_queue_advance() {
        let mut state = wired();
        state.queue.push(Track::new("track-1", "Track 1", "Artist"));
        state.queue.push(Track::new("track-2", "Track 2", "Artist"));
        state.play_current_quiet();
        assert!(state.audio.is_playing());
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("track-1"));

        state.set_sleep_timer_preset(TimerPreset::StopAfterTrack);
        assert!(state.sleep_timer.is_armed());

        // Queue advance moves to track-2, which fires StopAfterTrack and pauses audio
        state.next_track();
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("track-2"));
        assert!(!state.audio.is_playing());
        assert!(!state.sleep_timer.is_armed());
    }

    #[test]
    fn test_dsp_settings_toggle_forward_to_audio_backend_and_persist() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        state.set_settings_store(Box::new(store.clone()));

        assert!(!state.skip_silence);
        assert!(!state.loudness_normalization);
        assert!(!state.audio.skip_silence());
        assert!(!state.audio.loudness_normalization());

        state.set_skip_silence(true);
        assert!(state.skip_silence);
        assert!(state.audio.skip_silence());
        assert!(store.load().skip_silence);

        state.set_loudness_normalization(true);
        assert!(state.loudness_normalization);
        assert!(state.audio.loudness_normalization());
        assert!(store.load().loudness_normalization);

        state.set_skip_silence(false);
        assert!(!state.skip_silence);
        assert!(!state.audio.skip_silence());
        assert!(!store.load().skip_silence);

        state.set_loudness_normalization(false);
        assert!(!state.loudness_normalization);
        assert!(!state.audio.loudness_normalization());
        assert!(!store.load().loudness_normalization);
    }

    #[test]
    fn test_dsp_settings_forward_on_set_settings_store() {
        let mut state = wired();
        let store = MemorySettingsStore::default();
        let custom_settings =
            Settings { skip_silence: true, loudness_normalization: true, ..Default::default() };
        store.save(&custom_settings).expect("save custom settings");

        state.set_settings_store(Box::new(store));
        assert!(state.skip_silence);
        assert!(state.loudness_normalization);
        assert!(state.audio.skip_silence());
        assert!(state.audio.loudness_normalization());
    }

    #[test]
    fn test_blank_import_path_rejected() {
        let mut state = wired();
        for invalid in ["", "   "] {
            let res = state.import_library_backup(invalid);
            assert!(res.is_err());
            assert!(state.backup_status_text.contains("backup path cannot be blank"));
        }
    }

    #[test]
    fn test_dual_scrobbling_forwards_events_to_both_scrobblers_with_identical_args() {
        let mut state = wired();
        let primary_stub = Box::new(StubScrobbler::new());
        let secondary_stub = Box::new(StubScrobbler::new());

        state.set_scrobbler(primary_stub);
        state.set_secondary_scrobbler(Some(secondary_stub));

        let track = Track::new_full("vid-dual", "Paranoid Android", "Radiohead", None, Some(380));
        state.queue.push(track);
        state.play_current_quiet();

        let prim = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().unwrap();
        let sec = state
            .secondary_scrobbler
            .as_ref()
            .unwrap()
            .as_any()
            .downcast_ref::<StubScrobbler>()
            .unwrap();

        assert_eq!(prim.now_playing_calls, vec![("Radiohead".into(), "Paranoid Android".into())]);
        assert_eq!(sec.now_playing_calls, vec![("Radiohead".into(), "Paranoid Android".into())]);

        state.maybe_scrobble_with_elapsed(200);

        let prim = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().unwrap();
        let sec = state
            .secondary_scrobbler
            .as_ref()
            .unwrap()
            .as_any()
            .downcast_ref::<StubScrobbler>()
            .unwrap();

        assert_eq!(prim.scrobble_calls.len(), 1);
        assert_eq!(sec.scrobble_calls.len(), 1);
        assert_eq!(prim.scrobble_calls[0].0, "Radiohead");
        assert_eq!(prim.scrobble_calls[0].1, "Paranoid Android");
        assert_eq!(sec.scrobble_calls[0].0, "Radiohead");
        assert_eq!(sec.scrobble_calls[0].1, "Paranoid Android");
        assert_eq!(prim.scrobble_calls[0].2, sec.scrobble_calls[0].2);
    }

    #[test]
    fn test_secondary_none_keeps_primary_only_behavior() {
        let mut state = wired();
        let primary_stub = Box::new(StubScrobbler::new());
        state.set_scrobbler(primary_stub);
        state.set_secondary_scrobbler(None);

        assert!(state.secondary_scrobbler.is_none());
        assert!(!state.secondary_lastfm_is_live());

        let track = Track::new_full("vid-single", "Karma Police", "Radiohead", None, Some(240));
        state.queue.push(track);
        state.play_current_quiet();
        state.maybe_scrobble_with_elapsed(150);

        let prim = state.scrobbler.as_any().downcast_ref::<StubScrobbler>().unwrap();
        assert_eq!(prim.now_playing_calls, vec![("Radiohead".into(), "Karma Police".into())]);
        assert_eq!(prim.scrobble_calls.len(), 1);
        assert_eq!(prim.scrobble_calls[0].0, "Radiohead");
        assert_eq!(prim.scrobble_calls[0].1, "Karma Police");
        assert!(state.secondary_scrobbler.is_none());
    }

    #[test]
    fn test_enabling_secondary_lastfm_with_complete_creds_yields_live_secondary() {
        let mut state = wired();
        let creds = LastFmCredentials::new("sec-key", "sec-secret", "sec-session");
        state.save_secondary_lastfm_credentials(&creds);
        assert!(!state.secondary_lastfm_is_live());
        assert!(state.secondary_scrobbler.is_none());

        state.set_secondary_lastfm_enabled(true);
        assert!(state.secondary_lastfm_enabled);
        assert!(state.secondary_lastfm_is_live());
        assert!(state.settings_store.load().secondary_lastfm_enabled);

        state.set_secondary_lastfm_enabled(false);
        assert!(!state.secondary_lastfm_enabled);
        assert!(!state.secondary_lastfm_is_live());
        assert!(state.secondary_scrobbler.is_none());
        assert!(!state.settings_store.load().secondary_lastfm_enabled);
        assert_eq!(state.secondary_credentials.load_lastfm(), Some(creds));
    }

    #[test]
    fn test_incomplete_secondary_creds_never_go_live() {
        let mut state = wired();
        state.set_secondary_lastfm_enabled(true);

        for (k, s, sk) in [
            ("", "secret", "sk"),
            ("key", "", "sk"),
            ("key", "secret", ""),
            ("   ", "secret", "sk"),
            ("key", "   ", "sk"),
            ("key", "secret", "   "),
        ] {
            let incomplete = LastFmCredentials::new(k, s, sk);
            state.save_secondary_lastfm_credentials(&incomplete);
            assert!(
                !state.secondary_lastfm_is_live(),
                "expected incomplete secondary creds ({k}, {s}, {sk}) to remain dormant"
            );
            assert!(state.secondary_scrobbler.is_none());
        }
    }

    #[test]
    fn test_app_state_export_clear_likes_import_restores_likes() {
        let mut state = wired();
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-test-app-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).expect("create test temp dir");
        state.backup_dir = Some(temp_dir.clone());

        assert!(state.is_liked("stub-1"));

        let path = state.export_library_backup().expect("export succeeds");
        assert!(path.exists());
        assert!(state.backup_status_text.contains("Exported to:"));

        state.catalog.set_liked("stub-1", false);
        assert!(!state.is_liked("stub-1"));
        assert!(state.catalog.liked_tracks().is_empty());

        let summary = state
            .import_library_backup(path.to_str().expect("valid utf-8 path"))
            .expect("import succeeds");
        assert_eq!(summary.liked_applied, 1);
        assert!(state.is_liked("stub-1"));
        assert!(state.backup_status_text.contains("Imported: 1 liked"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_app_state_prefers_override_over_stub_provider_lines_and_falls_back_when_absent() {
        let mut state = wired();
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-test-override-pref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let override_dir = LyricOverrideDir::new(&temp_dir);
        state.set_lyric_overrides(override_dir.clone());

        state.queue.push(Track::new("video-pref-1", "Prefer Song", "Prefer Artist"));

        // When override file does not exist, falls back to provider
        state.refresh_lyrics();
        assert!(!state.lyrics_from_override);
        assert_eq!(state.lyrics_lines.len(), 2);
        assert_eq!(state.lyrics_lines[1].text, "Deterministic synced lyrics");

        // Save override for the current track id
        let lrc = "[00:02.50]Local override line\n";
        override_dir.save("video-pref-1", lrc).expect("save override");

        // Simulate track change or cache clear
        state.lyrics_key = None;
        state.refresh_lyrics();
        assert!(state.lyrics_from_override);
        assert_eq!(state.lyrics_lines.len(), 1);
        assert_eq!(state.lyrics_lines[0].start_ms, 2500);
        assert_eq!(state.lyrics_lines[0].text, "Local override line");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn app_metadata_override_save_and_clear_persists_through_disk_store() {
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-app-meta-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let mut state = wired();
        let store = DiskMetaStore::new(temp_dir.clone());
        state.set_meta_store(Box::new(store));

        let track = Track::new("track-persist", "Original Title", "Original Artist");
        state.queue.push(track.clone());

        // Initial resolved text equals canonical track text
        assert_eq!(
            state.resolved_track_text(&track),
            ("Original Title".to_string(), "Original Artist".to_string())
        );

        // Save override
        let patch =
            MetadataOverride::new(Some("Custom Display Title"), None).expect("valid override");
        state.save_meta_override(&track.id, patch).expect("save override succeeds");

        // Display updates immediately
        assert_eq!(
            state.resolved_track_text(&track),
            ("Custom Display Title".to_string(), "Original Artist".to_string())
        );

        // Disk store reloaded in a fresh store instance contains the persisted override
        let fresh_store = DiskMetaStore::new(temp_dir.clone());
        let disk_overrides = fresh_store.load();
        assert_eq!(
            disk_overrides.get(&track.id).and_then(|m| m.title.as_deref()),
            Some("Custom Display Title")
        );

        // Clear override
        state.clear_meta_override(&track.id);
        assert_eq!(
            state.resolved_track_text(&track),
            ("Original Title".to_string(), "Original Artist".to_string())
        );

        // Disk store reloaded reflects the clearance
        let cleared_store = DiskMetaStore::new(temp_dir.clone());
        assert!(cleared_store.load().is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_import_library_backup_nonexistent_file_surfaces_error() {
        let mut state = wired();
        let bad_path = "/tmp/moggyt_definitely_nonexistent_backup.json";
        let res = state.import_library_backup(bad_path);
        assert!(res.is_err());
        assert!(state.backup_status_text.contains("Import error:"));
    }

    #[test]
    fn app_state_lyrics_submit_surfaces_validation_errors_for_blank_input() {
        let mut state = wired();
        state.set_lyrics_mode(true);
        assert!(state.is_lyrics_mode());
        state.search_text = "   ".into();
        state.submit_search();

        assert_eq!(state.search_error.as_deref(), Some("lyrics snippet cannot be blank"));
        assert_eq!(state.lyrics_error.as_deref(), Some("lyrics snippet cannot be blank"));
        assert!(state.lyrics_results.is_empty());
        assert!(state.search_results.is_empty());
        assert_eq!(state.page, Page::Search);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn app_state_lyrics_submit_surfaces_validation_errors_for_overlong_input() {
        let mut state = wired();
        state.lyrics_mode = true;
        state.search_text = "x".repeat(201);
        state.submit_search();

        assert!(state.search_error.is_some());
        assert!(state.lyrics_results.is_empty());
        assert_eq!(state.page, Page::Search);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn app_state_lyrics_submit_populates_results_and_enqueues_hit() {
        let mut state = wired();
        state.lyrics_mode = true;
        state.search_text = "  midnight breeze  ".into();
        state.submit_search();

        assert_eq!(state.search_error, None);
        assert_eq!(state.lyrics_error, None);
        assert_eq!(state.lyrics_results.len(), 1);
        assert_eq!(state.lyrics_results[0].id, "stub-1");
        assert_eq!(state.lyrics_results[0].title, "Midnight City");
        assert_eq!(state.search_results, state.lyrics_results);
        assert_eq!(state.page, Page::Queue);
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue.current().map(|t| t.id.as_str()), Some("stub-1"));
    }

    #[test]
    fn app_state_direct_submit_lyrics_search_call() {
        let mut state = wired();
        state.search_text = "whisper softly".into();
        state.submit_lyrics_search();

        assert_eq!(state.search_error, None);
        assert_eq!(state.lyrics_results.len(), 1);
        assert_eq!(state.lyrics_results[0].id, "stub-2");
        assert_eq!(state.lyrics_results[0].title, "Echoes of Silence");
        assert_eq!(state.page, Page::Queue);
    }

    #[test]
    fn app_state_lyrics_submit_miss_clears_results_and_stays_on_page() {
        let mut state = wired();
        state.lyrics_mode = true;
        state.search_text = "lyrics not matching anything".into();
        state.submit_search();

        assert_eq!(state.search_error, None);
        assert!(state.lyrics_results.is_empty());
        assert_eq!(state.page, Page::Search);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn test_auto_skip_advances_past_marked_intro_and_does_not_loop() {
        let mut state = wired();
        let track = Track::new("intro-vid", "Intro Song", "Artist");
        state.queue.push(track);
        state.play_head();

        state.mark_segment_start_at(0);
        assert!(state.mark_segment_end_at(15));
        assert_eq!(state.audio.position_secs(), 0);

        // First frame: inside 0..15 range -> auto-skip seeks to 15
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 15);

        // Second frame: at boundary (15) -> does not seek again
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 15);

        // Advance playback past range -> does not seek
        state.audio.seek(25);
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 25);
    }

    #[test]
    fn test_auto_skip_does_not_seek_backwards_or_outside_ranges() {
        let mut state = wired();
        let track = Track::new("seg-vid", "Segment Song", "Artist");
        state.queue.push(track);
        state.play_head();

        state.mark_segment_start_at(20);
        assert!(state.mark_segment_end_at(30));

        // Position 10 is outside -> no jump
        state.audio.seek(10);
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 10);

        // Position 20 is at start -> jumps to 30
        state.audio.seek(20);
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 30);

        // Position 30 is at end -> no jump
        state.check_auto_skip();
        assert_eq!(state.audio.position_secs(), 30);
    }

    #[test]
    fn test_mark_segment_flow_validation_and_errors() {
        let mut state = wired();

        // 1. Mark end without active track
        assert!(!state.mark_segment_end_at(10));
        assert_eq!(state.skip_error.as_deref(), Some("No active track"));

        let track = Track::new("track-flow", "Flow Song", "Artist");
        state.queue.push(track);

        // 2. Mark end without marking start first
        assert!(!state.mark_segment_end_at(10));
        assert_eq!(state.skip_error.as_deref(), Some("Mark start first"));

        // 3. Mark start at 20, then mark end at 10 (end < start)
        state.mark_segment_start_at(20);
        assert_eq!(state.marked_start_secs, Some(20));
        assert!(!state.mark_segment_end_at(10));
        assert_eq!(state.skip_error.as_deref(), Some("End must be greater than start"));
        assert_eq!(state.marked_start_secs, Some(20));

        // 4. Mark end at 20 (end == start)
        assert!(!state.mark_segment_end_at(20));
        assert_eq!(state.skip_error.as_deref(), Some("End must be greater than start"));

        // 5. Mark end at 35 (valid: 20 < 35)
        assert!(state.mark_segment_end_at(35));
        assert_eq!(state.marked_start_secs, None);
        assert_eq!(state.skip_error, None);

        let stored = state.current_skip_ranges().expect("ranges present");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored.segments(), &[SkipSegment { start_secs: 20, end_secs: 35 }]);
    }

    #[test]
    fn test_delete_segment_and_store_persistence() {
        let mut state = wired();
        let store = MemorySkipStore::new();
        state.set_skip_store(Box::new(store.clone()));

        let track = Track::new("track-del", "Delete Song", "Artist");
        state.queue.push(track);

        state.mark_segment_start_at(5);
        state.mark_segment_end_at(15);
        state.mark_segment_start_at(30);
        state.mark_segment_end_at(40);

        assert_eq!(state.current_skip_ranges().unwrap().len(), 2);
        assert_eq!(store.load().get("track-del").unwrap().len(), 2);

        // Delete invalid index returns false
        assert!(!state.delete_segment("track-del", 5));

        // Delete first segment
        assert!(state.delete_segment("track-del", 0));
        assert_eq!(state.current_skip_ranges().unwrap().len(), 1);
        assert_eq!(
            state.current_skip_ranges().unwrap().segments(),
            &[SkipSegment { start_secs: 30, end_secs: 40 }]
        );
        assert_eq!(store.load().get("track-del").unwrap().len(), 1);

        // Delete remaining segment
        assert!(state.delete_segment("track-del", 0));
        assert_eq!(state.current_skip_ranges().unwrap().len(), 0);
        assert_eq!(store.load().get("track-del").unwrap().len(), 0);
    }

    #[test]
    fn test_mark_segment_captures_audio_position() {
        let mut state = wired();
        let track = Track::new("track-audio", "Audio Pos Song", "Artist");
        state.queue.push(track);
        state.play_head();

        state.audio.seek(12);
        state.mark_segment_start();
        assert_eq!(state.marked_start_secs, Some(12));

        state.audio.seek(24);
        assert!(state.mark_segment_end());
        assert_eq!(state.marked_start_secs, None);

        let stored = state.current_skip_ranges().expect("stored ranges");
        assert_eq!(stored.segments(), &[SkipSegment { start_secs: 12, end_secs: 24 }]);
    }

    #[test]
    fn test_offset_applies_to_override_lines() {
        let mut state = wired();
        let temp_dir = std::env::temp_dir()
            .join(format!("moggyt-test-override-offset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let override_dir = LyricOverrideDir::new(&temp_dir);
        let lrc = "[00:10.00]Override line\n";
        override_dir.save("video-offset-1", lrc).expect("save override");
        state.set_lyric_overrides(override_dir);

        state.queue.push(Track::new("video-offset-1", "Offset Song", "Offset Artist"));
        state.refresh_lyrics();
        assert!(state.lyrics_from_override);

        // Baseline (0ms offset)
        assert_eq!(state.visible_lyric_lines(), vec![(10000, "Override line".to_string())]);

        // Positive offset (+500ms)
        state.nudge_lyrics_offset(500);
        assert_eq!(state.visible_lyric_lines(), vec![(10500, "Override line".to_string())]);

        // Negative offset (-2000ms): 10000 + 500 - 2000 = 8500ms
        state.nudge_lyrics_offset(-2000);
        assert_eq!(state.visible_lyric_lines(), vec![(8500, "Override line".to_string())]);

        // Clamping at zero: nudge by -20000ms
        state.nudge_lyrics_offset(-20000);
        assert_eq!(state.visible_lyric_lines(), vec![(0, "Override line".to_string())]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[derive(Default)]
    struct ImportTestingCatalog {
        inner: CatalogStub,
    }

    impl Catalog for ImportTestingCatalog {
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
            if browse_id.contains("unknown") {
                return None;
            }
            if browse_id.contains("empty") {
                return Some(Detail::new(kind, "Empty Title", "Subtitle", Vec::new()));
            }
            self.inner.detail(browse_id, kind)
        }
        fn set_session_cookies(&mut self, cookies: Option<&str>) {
            self.inner.set_session_cookies(cookies);
        }
    }

    #[test]
    fn import_playlist_url_enqueues_stub_detail_tracks_and_returns_count() {
        let mut state = wired();
        let count = state
            .import_playlist_url("https://music.youtube.com/playlist?list=PLstub123")
            .expect("import succeeds");
        assert_eq!(count, 2);
        assert_eq!(state.queue.len(), 2);
        assert_eq!(state.queue.tracks()[0].id, "PLstub123-1");
        assert_eq!(state.queue.tracks()[1].id, "PLstub123-2");
    }

    #[test]
    fn import_playlist_url_unknown_id_errors_loudly() {
        let mut state = AppState::new(
            ImportTestingCatalog::default(),
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );
        let result =
            state.import_playlist_url("https://music.youtube.com/playlist?list=PLunknown_456");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("playlist detail not found"));
        assert!(state.queue.is_empty());
    }

    #[test]
    fn import_playlist_url_blank_url_errors() {
        let mut state = wired();
        assert!(state.import_playlist_url("").is_err());
        assert!(state.import_playlist_url("   ").is_err());
        assert!(state.queue.is_empty());
    }

    #[test]
    fn import_playlist_url_invalid_url_leaves_queue_untouched() {
        let mut state = wired();
        state.queue.push(Track::new("pre-existing", "Title", "Artist"));
        assert_eq!(state.queue.len(), 1);

        assert!(state.import_playlist_url("https://spotify.com/playlist?list=PL123").is_err());
        assert_eq!(state.queue.len(), 1);

        assert!(
            state.import_playlist_url("https://music.youtube.com/playlist?list=BADPREFIX").is_err()
        );
        assert_eq!(state.queue.len(), 1);

        assert!(state.import_playlist_url("ftp://music.youtube.com/playlist?list=PL123").is_err());
        assert_eq!(state.queue.len(), 1);

        assert_eq!(state.queue.tracks()[0].id, "pre-existing");
    }

    #[test]
    fn import_playlist_url_empty_tracks_enqueues_zero_and_reports_zero() {
        let mut state = AppState::new(
            ImportTestingCatalog::default(),
            AudioStub::new(),
            MemorySessionStore::new(),
            LrclibStub::new(),
            MemoryCoverCache::new(),
        );
        let count = state
            .import_playlist_url("https://music.youtube.com/playlist?list=PLempty_999")
            .expect("empty playlist succeeds");
        assert_eq!(count, 0);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn test_secondary_credentials_save_clear_round_trip_and_namespace_separation() {
        let mut state = wired();
        assert_eq!(state.credentials.load_lastfm(), None);
        assert_eq!(state.secondary_credentials.load_lastfm(), None);

        let primary_creds = LastFmCredentials::new("prim-key", "prim-sec", "prim-session");
        let secondary_creds = LastFmCredentials::new("sec-key", "sec-sec", "sec-session");

        // Primary save does not satisfy secondary load
        state.save_lastfm_credentials(&primary_creds);
        assert_eq!(state.credentials.load_lastfm(), Some(primary_creds.clone()));
        assert_eq!(state.secondary_credentials.load_lastfm(), None);

        // Secondary save does not overwrite primary
        state.save_secondary_lastfm_credentials(&secondary_creds);
        assert_eq!(state.credentials.load_lastfm(), Some(primary_creds));
        assert_eq!(state.secondary_credentials.load_lastfm(), Some(secondary_creds.clone()));

        // Clearing primary leaves secondary intact
        state.clear_lastfm_credentials();
        assert_eq!(state.credentials.load_lastfm(), None);
        assert_eq!(state.secondary_credentials.load_lastfm(), Some(secondary_creds));

        // Clearing secondary leaves both empty
        state.clear_secondary_lastfm_credentials();
        assert_eq!(state.secondary_credentials.load_lastfm(), None);
        assert!(!state.secondary_lastfm_is_live());
    }

    #[test]
    fn test_secondary_credentials_from_input_buffers() {
        let mut state = wired();
        state.secondary_lastfm_api_key_text = "sec-input-key".into();
        state.secondary_lastfm_secret_text = "sec-input-sec".into();
        state.secondary_lastfm_session_key_text = "sec-input-session".into();
        state.set_secondary_lastfm_enabled(true);

        let ok = state.save_secondary_lastfm_credentials_from_input();
        assert!(ok);
        assert!(state.secondary_lastfm_is_live());
        assert!(state.secondary_lastfm_api_key_text.is_empty());
        assert!(state.secondary_lastfm_secret_text.is_empty());
        assert!(state.secondary_lastfm_session_key_text.is_empty());

        state.clear_secondary_lastfm_credentials();
        assert!(!state.secondary_lastfm_is_live());
        assert_eq!(state.secondary_credentials.load_lastfm(), None);
    }

    #[test]
    fn identity_scrobble_lyrics_and_presence_use_canonical_while_display_uses_resolved() {
        let mut state = wired();
        let track = Track::new_full(
            "identity-track-1",
            "Canonical Title",
            "Canonical Artist",
            Some("Album".into()),
            Some(200),
        );
        state.queue.push(track.clone());

        // Apply metadata override for title and artist
        let patch = MetadataOverride::new(Some("Overridden Title"), Some("Overridden Artist"))
            .expect("valid override");
        state.save_meta_override(&track.id, patch).expect("save succeeds");

        // Display resolves to the overridden strings
        let (disp_title, disp_artist) = state.resolved_track_text(&track);
        assert_eq!(disp_title, "Overridden Title");
        assert_eq!(disp_artist, "Overridden Artist");

        // Canonical identity check 1: play_current_quiet forwards canonical to scrobbler now_playing
        state.play_current_quiet();
        let stub =
            state.scrobbler.as_any().downcast_ref::<StubScrobbler>().expect("stub scrobbler");
        assert_eq!(
            stub.now_playing_calls.last(),
            Some(&("Canonical Artist".to_string(), "Canonical Title".to_string()))
        );

        // Canonical identity check 2: scrobble forwards canonical to scrobbler scrobble
        state.maybe_scrobble_with_elapsed(120);
        let stub =
            state.scrobbler.as_any().downcast_ref::<StubScrobbler>().expect("stub scrobbler");
        assert_eq!(
            stub.scrobble_calls.last().map(|(a, t, _)| (a.as_str(), t.as_str())),
            Some(("Canonical Artist", "Canonical Title"))
        );

        // Canonical identity check 3: refresh_lyrics indexes by canonical key
        state.refresh_lyrics();
        assert_eq!(
            state.lyrics_key,
            Some(("Canonical Artist".to_string(), "Canonical Title".to_string()))
        );

        // Canonical identity check 4: presence sync uses canonical track text
        state.sync_presence();
        assert_eq!(
            state.last_presence_key,
            Some(("Canonical Artist".to_string(), "Canonical Title".to_string()))
        );
        assert_eq!(
            state.presence.last_published(),
            Some(("Canonical Artist".to_string(), "Canonical Title".to_string()))
        );
        // Discord records (details, state) = (title, artist) per `activity_parts`.
        assert_eq!(
            state.discord.last_published(),
            Some(("Canonical Title".to_string(), "Canonical Artist".to_string()))
        );

        // Canonical identity check 5: like status keys on catalog track id,
        // unaffected by the unrelated override entry (stub hits only).
        state.catalog.set_liked("stub-1", true);
        assert!(state.is_liked("stub-1"));
        assert!(!state.is_liked(&track.id));
    }

    #[test]
    fn meta_override_from_input_helpers_save_and_reset_buffers() {
        let mut state = wired();
        let track = Track::new("t-input", "Song", "Band");
        state.queue.push(track.clone());

        state.sync_meta_edit_buffers(&track);
        assert_eq!(state.meta_edit_title, "Song");
        assert_eq!(state.meta_edit_artist, "Band");

        // Edit title, leave artist blank (means keep canonical artist)
        state.meta_edit_title = "Edited Song".into();
        state.meta_edit_artist = "   ".into();
        state.save_meta_override_from_input(&track);

        let (t, a) = state.resolved_track_text(&track);
        assert_eq!(t, "Edited Song");
        assert_eq!(a, "Band");
        assert_eq!(state.meta_edit_title, "Edited Song");
        assert_eq!(state.meta_edit_artist, "Band");

        // Clear and reset
        state.clear_meta_override_and_reset(&track);
        let (t, a) = state.resolved_track_text(&track);
        assert_eq!(t, "Song");
        assert_eq!(a, "Band");
        assert_eq!(state.meta_edit_title, "Song");
        assert_eq!(state.meta_edit_artist, "Band");
    }
}
