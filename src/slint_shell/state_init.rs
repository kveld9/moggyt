//! Application state constructor and default collaborator wiring.
//!
//! Hexagonal invariant: wires injected ports into thread-safe containers
//! and initializes in-memory UI state with zero I/O.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::domain::{PlaybackQueue, SearchFilter, SleepTimer, ThemeId};
use crate::i18n::Locale;
use crate::infra::{
    AudioBackend, Catalog, CoverCache, DesktopPresence, DiscordPresence, LyricsSource,
    MemoryQueueStore, MemorySettingsStore, MemorySkipStore, SessionStore, SettingsStore,
    StubScrobbler,
};
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::player_bar::PlayerBarPrefs;
use crate::slint_shell::state::{
    SharedAudio, SharedCatalog, SharedCovers, SharedLyrics, SlintAppState,
};

impl SlintAppState {
    /// Creates the Slint application state with injected collaborators.
    pub fn new(
        mut catalog: impl Catalog + Send + 'static,
        audio: impl AudioBackend + Send + 'static,
        session: impl SessionStore + 'static,
        lyrics: impl LyricsSource + Send + Sync + 'static,
        covers: impl CoverCache + Send + 'static,
    ) -> Self {
        catalog.set_locale(Locale::default());
        let stored_session = session.load();
        let signed_in = stored_session.is_some();
        if let Some(ref cookies) = stored_session {
            catalog.set_session_cookies(Some(cookies));
        }
        let catalog: SharedCatalog = Arc::new(Mutex::new(Box::new(catalog)));
        let audio: SharedAudio = Arc::new(Mutex::new(Box::new(audio)));
        let session = Box::new(session) as Box<dyn SessionStore>;
        let lyrics: SharedLyrics = Arc::new(Box::new(lyrics));
        let covers: SharedCovers = Arc::new(Mutex::new(Box::new(covers)));
        let settings_store = Box::new(MemorySettingsStore::default());
        let recent_searches = settings_store.load().recent_searches;
        Self {
            catalog,
            audio,
            session,
            lyrics,
            covers,
            lyric_overrides: None,
            tray: None,
            presence: DesktopPresence::new(),
            discord: DiscordPresence::new(),
            scrobbler: Arc::new(Mutex::new(Box::new(StubScrobbler::new()))),
            secondary_scrobbler: None,
            settings_store,
            queue_store: Box::new(MemoryQueueStore::default()),
            skip_store: Box::new(MemorySkipStore::new()),
            updater_rx: None,
            page: SlintPage::Search,
            queue_panel_open: false,
            queue: PlaybackQueue::new(),
            home_shelves: Vec::new(),
            search_query: String::new(),
            search_active: false,
            recent_searches,
            search_results: Vec::new(),
            library_playlists: Vec::new(),
            library_albums: Vec::new(),
            detail: None,
            detail_return: SlintPage::Search,
            lyrics_lines: Vec::new(),
            lyrics_key: None,
            cover_key: None,
            current_cover_image: None,
            last_presence_key: None,
            started_at: None,
            last_skip_target: None,
            cached_skip_ranges: None,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::LocaleTrackingCatalog;

    #[test]
    fn test_slint_app_state_new_configures_locale_and_initializes_empty_shelves() {
        let (catalog, locales, fetch_locales) = LocaleTrackingCatalog::new();
        let audio = crate::infra::AudioStub::new();
        let session = crate::infra::MemorySessionStore::new();
        let lyrics = crate::infra::LrclibStub::new();
        let covers = crate::infra::MemoryCoverCache::new();

        let state = SlintAppState::new(catalog, audio, session, lyrics, covers);

        assert_eq!(*locales.lock().unwrap(), vec![Locale::English]);
        assert!(fetch_locales.lock().unwrap().is_empty());
        assert!(state.home_shelves.is_empty());
        assert!(!state.search_active);
        assert!(state.recent_searches.is_empty());
    }
}
