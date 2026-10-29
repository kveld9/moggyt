#![cfg(test)]
//! Shared test support fakes and state fixtures for UI shell tests.
//!
//! Hexagonal invariant: in-memory stubs only; zero network, zero audio hardware, zero async.

use std::sync::{Arc, Mutex};

use crate::domain::{
    Album, Catalog, Detail, DetailKind, Playlist, SearchQuery, SearchResult, Shelf, Track,
};
use crate::i18n::Locale;
use crate::infra::{AudioStub, CatalogStub, LrclibStub, MemoryCoverCache, MemorySessionStore};
use crate::slint_shell::state::SlintAppState;

/// Constructs a SlintAppState wired with memory-only test stub adapters.
pub fn test_wired()
-> SlintAppState<CatalogStub, AudioStub, MemorySessionStore, LrclibStub, MemoryCoverCache> {
    let catalog = CatalogStub::new();
    let audio = AudioStub::new();
    let session = MemorySessionStore::new();
    let lyrics = LrclibStub::new();
    let covers = MemoryCoverCache::new();
    SlintAppState::new(catalog, audio, session, lyrics, covers)
}

pub type LocaleLog = Arc<Mutex<Vec<Locale>>>;

/// Mock catalog recording locale setting and home shelf requests for verification.
#[derive(Debug, Clone)]
pub struct LocaleTrackingCatalog {
    pub locales: LocaleLog,
    pub fetch_locales: LocaleLog,
    pub current: Arc<Mutex<Locale>>,
}

impl LocaleTrackingCatalog {
    pub fn new() -> (Self, LocaleLog, LocaleLog) {
        let locales = Arc::new(Mutex::new(Vec::new()));
        let fetch_locales = Arc::new(Mutex::new(Vec::new()));
        let current = Arc::new(Mutex::new(Locale::default()));
        (
            Self {
                locales: Arc::clone(&locales),
                fetch_locales: Arc::clone(&fetch_locales),
                current,
            },
            locales,
            fetch_locales,
        )
    }
}

impl Catalog for LocaleTrackingCatalog {
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
        let loc = *self.current.lock().unwrap();
        self.fetch_locales.lock().unwrap().push(loc);
        let title = match loc {
            Locale::English => "Home English",
            Locale::Spanish => "Home Spanish",
        };
        vec![Shelf::new(title)]
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

    fn set_locale(&mut self, locale: Locale) {
        self.locales.lock().unwrap().push(locale);
        *self.current.lock().unwrap() = locale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::page::SlintPage;

    #[test]
    fn test_test_wired_builds_default_search_page() {
        let state = test_wired();
        assert_eq!(state.page, SlintPage::Search);
        assert!(state.queue.is_empty());
        assert_eq!(state.locale, Locale::default());
    }
}
