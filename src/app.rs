//! UI shell facade: navigation state, collaborator wiring, and event delegation.
//!
//! The shell holds no business rules; every user intent is forwarded to
//! [`crate::domain`] and every side effect goes through injected adapters.
//! State method implementations are partitioned by feature in [`state`].

pub mod components;
pub mod pages;
pub mod state;
pub mod theme;
pub mod views;

pub(crate) use state::item_detail_target;
pub use views::MoggytApp;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use eframe::egui;

use crate::domain::{
    Detail, DetailKind, MetadataOverride, PlaybackQueue, Shelf, SkipRanges, SleepTimer, ThemeId,
    Track,
};
use crate::i18n::Locale;
use crate::infra::{
    AudioBackend, AudioQuality, Catalog, CoverCache, CredentialStore, DesktopPresence,
    DiscordPresence, GithubUpdater, LyricLine, LyricOverrideDir, LyricsSource,
    MemoryCredentialStore, MemoryMetaStore, MemoryQueueStore, MemorySettingsStore, MemorySkipStore,
    MetaStore, QueueStore, Scrobbler, SessionStore, SettingsStore, SkipStore, StubScrobbler,
    TrayHandle,
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
}

/// Formats a millisecond duration as zero-padded "m:ss" (e.g. 65000 -> "1:05").
pub fn format_lyric_timestamp(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins}:{secs:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use state::test_support::wired;

    #[test]
    fn format_lyric_timestamp_table() {
        for (ms, exp) in [(0, "0:00"), (5_000, "0:05"), (65_000, "1:05"), (612_000, "10:12")] {
            assert_eq!(format_lyric_timestamp(ms), exp);
        }
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
}
