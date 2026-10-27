//! Home shelves browsing, detail navigation, and prefetch cache for [`AppState`].

use crate::app::{AppState, DETAIL_PREFETCH_CAP, Page};
use crate::domain::{Detail, DetailKind, SearchResult};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

impl<C: Catalog, A: AudioBackend, S: SessionStore, L: LyricsSource, K: CoverCache>
    AppState<C, A, S, L, K>
{
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
    use super::super::test_support::{CountingCatalog, wired};
    use super::*;
    use crate::domain::{Album, Artist, Playlist, Track};
    use crate::infra::{AudioStub, LrclibStub, MemoryCoverCache, MemorySessionStore};

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
}
