//! Bounded background cover fetcher: command channel, worker threads, decode-free hand-off to the UI thread.

use std::sync::{Arc, Mutex};

use crate::slint_shell::platform::{dispatch_to_ui, rgba_to_image};
use crate::slint_shell::playback_worker::{
    COVER_FETCH_CONCURRENCY_LIMIT, fetch_and_decode_cover, thumbnail_variant_for_row,
};
use crate::slint_shell::state::{SharedCovers, SlintAppState};
use crate::slint_shell::sync::{
    sync_category_shelves, sync_explore_shelves, sync_library_and_detail, sync_shelves_and_queue,
};
use crate::slint_shell::window::with_ui_state;
use crate::slint_shell::{CoverArtData, MoggytWindow};

/// Handles background cover fetch result and injects loaded Image into UI state.
pub(crate) fn handle_fetch_cover_command(
    url: &str,
    generation: u64,
    covers: &SharedCovers,
    weak: slint::Weak<MoggytWindow>,
) {
    let is_hi_res = url != thumbnail_variant_for_row(url);
    let Some(cover) = fetch_and_decode_cover(url, covers, is_hi_res) else {
        return;
    };
    let _ = dispatch_to_ui(move || {
        let Some(win) = weak.upgrade() else {
            return;
        };
        with_ui_state(|st| {
            apply_fetched_cover_to_state(&win, st, cover, generation);
        });
    });
}

/// Domain namespace for cover request generations, preventing cross-view collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverDomain {
    Search,
    Detail,
    Library,
    Play,
    Explore,
    Category,
}

impl CoverDomain {
    pub const fn base(self) -> u64 {
        match self {
            Self::Search => 0x01_0000_0000_0000,
            Self::Detail => 0x02_0000_0000_0000,
            Self::Library => 0x03_0000_0000_0000,
            Self::Play => 0x04_0000_0000_0000,
            Self::Explore => 0x05_0000_0000_0000,
            Self::Category => 0x06_0000_0000_0000,
        }
    }

    pub fn from_generation(generation: u64) -> Option<Self> {
        match generation & 0xFF_0000_0000_0000 {
            0x01_0000_0000_0000 => Some(Self::Search),
            0x02_0000_0000_0000 => Some(Self::Detail),
            0x03_0000_0000_0000 => Some(Self::Library),
            0x04_0000_0000_0000 => Some(Self::Play),
            0x05_0000_0000_0000 => Some(Self::Explore),
            0x06_0000_0000_0000 => Some(Self::Category),
            _ => None,
        }
    }
}

/// Which views a freshly fetched cover belongs to, keyed by generation match.
#[derive(Clone, Copy)]
struct CoverTargets {
    shelves_and_queue: bool,
    library_and_detail: bool,
    explore: bool,
    category: bool,
}

impl CoverTargets {
    /// True when the cover belongs to at least one live view.
    fn any(self) -> bool {
        self.shelves_and_queue || self.library_and_detail || self.explore || self.category
    }
}

fn is_domain_generation_active(st: &SlintAppState, domain: CoverDomain, generation: u64) -> bool {
    match domain {
        CoverDomain::Search => st.search_generation == generation,
        CoverDomain::Detail => st.detail_generation == generation,
        CoverDomain::Library => st.library_generation == generation,
        CoverDomain::Play => st.play_generation == generation,
        CoverDomain::Explore => st.explore_generation == generation,
        CoverDomain::Category => st.category_generation == generation,
    }
}

fn active_cover_targets(domain: CoverDomain) -> CoverTargets {
    match domain {
        CoverDomain::Search | CoverDomain::Play => CoverTargets {
            shelves_and_queue: true,
            library_and_detail: false,
            explore: false,
            category: false,
        },
        CoverDomain::Detail | CoverDomain::Library => CoverTargets {
            shelves_and_queue: false,
            library_and_detail: true,
            explore: false,
            category: false,
        },
        CoverDomain::Explore => CoverTargets {
            shelves_and_queue: false,
            library_and_detail: false,
            explore: true,
            category: false,
        },
        CoverDomain::Category => CoverTargets {
            shelves_and_queue: false,
            library_and_detail: false,
            explore: false,
            category: true,
        },
    }
}

fn domain_cover_targets(st: &SlintAppState, domain: CoverDomain, generation: u64) -> CoverTargets {
    if !is_domain_generation_active(st, domain, generation) {
        return CoverTargets {
            shelves_and_queue: false,
            library_and_detail: false,
            explore: false,
            category: false,
        };
    }
    active_cover_targets(domain)
}

fn legacy_cover_targets(st: &SlintAppState, generation: u64) -> CoverTargets {
    let is_search = st.search_generation == generation;
    let is_detail = st.detail_generation == generation;
    let is_library = st.library_generation == generation;
    let is_play = st.play_generation == generation;
    let is_explore = st.explore_generation == generation;
    let is_category = st.category_generation == generation;

    CoverTargets {
        shelves_and_queue: is_search || is_play,
        library_and_detail: is_detail || is_library,
        explore: is_explore,
        category: is_category,
    }
}

/// Maps the current generation counters to the views that must be resynced.
fn cover_targets(st: &SlintAppState, generation: u64) -> CoverTargets {
    if let Some(domain) = CoverDomain::from_generation(generation) {
        domain_cover_targets(st, domain, generation)
    } else {
        legacy_cover_targets(st, generation)
    }
}

/// Resyncs the views selected by cover generation targets.
fn sync_target_views(win: &MoggytWindow, st: &mut SlintAppState, targets: CoverTargets) {
    if targets.shelves_and_queue {
        sync_shelves_and_queue(win, st);
    }
    if targets.library_and_detail {
        sync_library_and_detail(win, st);
    }
    if targets.explore {
        sync_explore_shelves(win, st);
    }
    if targets.category {
        sync_category_shelves(win, st);
    }
}

/// Applies decoded cover art to UI state, triggering shelf card and search row resync.
pub(crate) fn apply_fetched_cover_to_state(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    cover: CoverArtData,
    generation: u64,
) {
    let Some(img) = rgba_to_image(&cover.rgba, cover.width, cover.height) else {
        return;
    };
    let targets = cover_targets(st, generation);
    if targets.any() {
        st.search_covers.insert(cover.url.clone(), img.clone());
        sync_target_views(win, st, targets);
    }
    sync_now_playing_after_cover_fetch(win, st, &cover.url, &img);
}

/// Updates now-playing cover when fetched artwork matches active track without downgrading hi-res.
pub(crate) fn sync_now_playing_after_cover_fetch(
    win: &MoggytWindow,
    st: &mut SlintAppState,
    cover_url: &str,
    img: &slint::Image,
) {
    let Some(track) = st.queue.current() else {
        return;
    };
    let is_full_match = track.artwork.as_deref() == Some(cover_url);
    let is_thumb_match =
        track.artwork.as_deref().map(thumbnail_variant_for_row).as_deref() == Some(cover_url);
    if is_full_match {
        st.now_playing_cover = Some(img.clone());
        st.current_cover_image = Some(img.clone());
        win.set_now_playing_cover(img.clone());
        win.set_cover_image(img.clone());
        win.set_has_cover(true);
    } else if is_thumb_match && st.now_playing_cover.is_none() {
        st.now_playing_cover = Some(img.clone());
        win.set_now_playing_cover(img.clone());
    }
}

/// Spawns a pool of background cover fetch workers bounded by COVER_FETCH_CONCURRENCY_LIMIT.
pub(crate) fn spawn_cover_fetch_pool(
    covers: SharedCovers,
    weak: slint::Weak<MoggytWindow>,
) -> std::sync::mpsc::Sender<(String, u64)> {
    let (tx, rx) = std::sync::mpsc::channel::<(String, u64)>();
    let rx_arc = Arc::new(Mutex::new(rx));

    for i in 0..COVER_FETCH_CONCURRENCY_LIMIT {
        let rx_worker = Arc::clone(&rx_arc);
        let covers_worker = Arc::clone(&covers);
        let weak_worker = weak.clone();
        let _ = std::thread::Builder::new().name(format!("moggyt-cover-{i}")).spawn(move || {
            run_cover_fetch_worker_loop(&rx_worker, &covers_worker, &weak_worker);
        });
    }

    tx
}

pub(crate) fn run_cover_fetch_worker_loop(
    rx: &Arc<Mutex<std::sync::mpsc::Receiver<(String, u64)>>>,
    covers: &SharedCovers,
    weak: &slint::Weak<MoggytWindow>,
) {
    loop {
        let task = {
            let Ok(guard) = rx.lock() else { break };
            guard.recv().ok()
        };
        let Some((url, generation)) = task else { break };
        handle_fetch_cover_command(&url, generation, covers, weak.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_cover_fetch_pool_concurrency_bound_constant_integration() {
        assert_eq!(COVER_FETCH_CONCURRENCY_LIMIT, 4);

        let (tx, rx) = std::sync::mpsc::channel::<(String, u64)>();
        for i in 0..COVER_FETCH_CONCURRENCY_LIMIT {
            tx.send((format!("https://example.com/art/{i}"), i as u64)).unwrap();
        }

        let mut received = 0;
        while rx.try_recv().is_ok() {
            received += 1;
        }
        assert_eq!(received, COVER_FETCH_CONCURRENCY_LIMIT);
    }

    #[test]
    fn test_cover_targets_generation_matching() {
        let mut st = test_wired();
        st.search_generation = 10;
        st.play_generation = 20;
        st.detail_generation = 30;
        st.library_generation = 40;
        st.explore_generation = 50;
        st.category_generation = 60;

        // Matching search generation
        let search_targets = cover_targets(&st, 10);
        assert!(search_targets.any());
        assert!(search_targets.shelves_and_queue);
        assert!(!search_targets.library_and_detail);
        assert!(!search_targets.explore);
        assert!(!search_targets.category);

        // Matching play generation
        let play_targets = cover_targets(&st, 20);
        assert!(play_targets.any());
        assert!(play_targets.shelves_and_queue);
        assert!(!play_targets.library_and_detail);
        assert!(!play_targets.explore);
        assert!(!play_targets.category);

        // Matching detail generation
        let detail_targets = cover_targets(&st, 30);
        assert!(detail_targets.any());
        assert!(!detail_targets.shelves_and_queue);
        assert!(detail_targets.library_and_detail);
        assert!(!detail_targets.explore);
        assert!(!detail_targets.category);

        // Matching library generation
        let library_targets = cover_targets(&st, 40);
        assert!(library_targets.any());
        assert!(!library_targets.shelves_and_queue);
        assert!(library_targets.library_and_detail);
        assert!(!library_targets.explore);
        assert!(!library_targets.category);

        // Matching explore generation
        let explore_targets = cover_targets(&st, 50);
        assert!(explore_targets.any());
        assert!(!explore_targets.shelves_and_queue);
        assert!(!explore_targets.library_and_detail);
        assert!(explore_targets.explore);
        assert!(!explore_targets.category);

        // Matching category generation
        let category_targets = cover_targets(&st, 60);
        assert!(category_targets.any());
        assert!(!category_targets.shelves_and_queue);
        assert!(!category_targets.library_and_detail);
        assert!(!category_targets.explore);
        assert!(category_targets.category);

        // Non-matching generation
        let non_matching = cover_targets(&st, 999);
        assert!(!non_matching.any());
        assert!(!non_matching.shelves_and_queue);
        assert!(!non_matching.library_and_detail);
        assert!(!non_matching.explore);
        assert!(!non_matching.category);
    }

    #[test]
    fn test_cover_domain_from_generation() {
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Search.base()),
            Some(CoverDomain::Search)
        );
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Search.base() + 42),
            Some(CoverDomain::Search)
        );
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Detail.base()),
            Some(CoverDomain::Detail)
        );
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Detail.base() + 1),
            Some(CoverDomain::Detail)
        );
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Library.base()),
            Some(CoverDomain::Library)
        );
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Library.base() + 99),
            Some(CoverDomain::Library)
        );
        assert_eq!(CoverDomain::from_generation(CoverDomain::Play.base()), Some(CoverDomain::Play));
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Play.base() + 5),
            Some(CoverDomain::Play)
        );
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Explore.base()),
            Some(CoverDomain::Explore)
        );
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Explore.base() + 7),
            Some(CoverDomain::Explore)
        );
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Category.base()),
            Some(CoverDomain::Category)
        );
        assert_eq!(
            CoverDomain::from_generation(CoverDomain::Category.base() + 13),
            Some(CoverDomain::Category)
        );

        // Untagged generations, zero, or unmapped bases return None
        assert_eq!(CoverDomain::from_generation(0), None);
        assert_eq!(CoverDomain::from_generation(100), None);
        assert_eq!(CoverDomain::from_generation(0x07_0000_0000_0000), None);
        assert_eq!(CoverDomain::from_generation(0xFF_0000_0000_0000), None);
    }

    #[test]
    fn test_cover_targets_cross_domain_collision_prevention() {
        let mut st = test_wired();
        let search_gen = CoverDomain::Search.base() + 5;
        let detail_gen = CoverDomain::Detail.base() + 5;
        let library_gen = CoverDomain::Library.base() + 5;
        let play_gen = CoverDomain::Play.base() + 5;
        let explore_gen = CoverDomain::Explore.base() + 5;
        let category_gen = CoverDomain::Category.base() + 5;

        st.search_generation = search_gen;
        st.detail_generation = detail_gen;
        st.library_generation = library_gen;
        st.play_generation = play_gen;
        st.explore_generation = explore_gen;
        st.category_generation = category_gen;

        // Search generation matches ONLY shelves_and_queue
        let search_targets = cover_targets(&st, search_gen);
        assert!(search_targets.any());
        assert!(search_targets.shelves_and_queue);
        assert!(!search_targets.library_and_detail);
        assert!(!search_targets.explore);
        assert!(!search_targets.category);

        // Detail generation matches ONLY library_and_detail
        let detail_targets = cover_targets(&st, detail_gen);
        assert!(detail_targets.any());
        assert!(!detail_targets.shelves_and_queue);
        assert!(detail_targets.library_and_detail);
        assert!(!detail_targets.explore);
        assert!(!detail_targets.category);

        // Library generation matches ONLY library_and_detail
        let library_targets = cover_targets(&st, library_gen);
        assert!(library_targets.any());
        assert!(!library_targets.shelves_and_queue);
        assert!(library_targets.library_and_detail);
        assert!(!library_targets.explore);
        assert!(!library_targets.category);

        // Play generation matches ONLY shelves_and_queue
        let play_targets = cover_targets(&st, play_gen);
        assert!(play_targets.any());
        assert!(play_targets.shelves_and_queue);
        assert!(!play_targets.library_and_detail);
        assert!(!play_targets.explore);
        assert!(!play_targets.category);

        // Explore generation matches ONLY explore
        let explore_targets = cover_targets(&st, explore_gen);
        assert!(explore_targets.any());
        assert!(!explore_targets.shelves_and_queue);
        assert!(!explore_targets.library_and_detail);
        assert!(explore_targets.explore);
        assert!(!explore_targets.category);

        // Category generation matches ONLY category
        let category_targets = cover_targets(&st, category_gen);
        assert!(category_targets.any());
        assert!(!category_targets.shelves_and_queue);
        assert!(!category_targets.library_and_detail);
        assert!(!category_targets.explore);
        assert!(category_targets.category);

        // Inactive generation in a known domain returns empty targets
        let stale_search = cover_targets(&st, CoverDomain::Search.base() + 4);
        assert!(!stale_search.any());
        assert!(!stale_search.shelves_and_queue);
        assert!(!stale_search.library_and_detail);
        assert!(!stale_search.explore);
        assert!(!stale_search.category);
    }
}
