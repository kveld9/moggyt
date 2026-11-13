//! Video-policy refresh for cached catalog views after the settings toggle changes.

use std::sync::mpsc::Sender;

use crate::slint_shell::MoggytWindow;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::search_submit::submit_search_with_filter_async;
use crate::slint_shell::state::SlintAppState;

impl SlintAppState {
    /// Drops cached catalog browse data that may hold music videos so the next
    /// navigation refetches it under the active video policy.
    pub(crate) fn invalidate_video_dependent_views(&mut self) {
        self.explore_loaded = false;
        self.explore_loading = false;
        self.explore_shelves.clear();
        self.category_shelves.clear();
    }
}

/// Applies a video-policy change to cached views and re-runs the active search.
pub(crate) fn refresh_video_policy_views(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    worker_tx: &Sender<PlaybackWorkerCommand>,
) {
    state.invalidate_video_dependent_views();
    let query = state.search_query.clone();
    if !state.search_active || query.trim().is_empty() {
        return;
    }
    let filter = state.search_filter;
    submit_search_with_filter_async(window, state, &query, filter, worker_tx);
}

#[cfg(test)]
mod tests {
    use crate::domain::{Detail, DetailKind, MusicVideoType, SearchResult, Shelf, Track};
    use crate::slint_shell::explore::{apply_category_outcome, apply_explore_outcome};
    use crate::slint_shell::test_support::{MockLibraryCatalog, test_wired, wrap_mock_catalog};

    fn video(id: &str) -> Track {
        Track::new(id, id, "Artist").with_video_type(MusicVideoType::Omv)
    }

    fn song(id: &str) -> Track {
        Track::new(id, id, "Artist").with_video_type(MusicVideoType::Atv)
    }

    fn mixed_shelf(title: &str) -> Shelf {
        Shelf {
            title: title.to_owned(),
            items: vec![SearchResult::Track(video("v1")), SearchResult::Track(song("s1"))],
        }
    }

    #[test]
    fn invalidate_clears_cached_explore_and_category_shelves() {
        let mut state = test_wired();
        state.explore_loaded = true;
        state.explore_loading = true;
        state.explore_shelves = vec![Shelf::new("Explore")];
        state.category_shelves = vec![Shelf::new("Category")];

        state.invalidate_video_dependent_views();

        assert!(!state.explore_loaded);
        assert!(!state.explore_loading);
        assert!(state.explore_shelves.is_empty());
        assert!(state.category_shelves.is_empty());
    }

    #[test]
    fn explore_and_category_outcomes_hide_videos_when_enabled() {
        let mut state = test_wired();
        state.songs_only = true;
        let videos_only =
            Shelf { title: "Videos".to_owned(), items: vec![SearchResult::Track(video("v2"))] };

        apply_explore_outcome(&mut state, Ok(vec![mixed_shelf("Mixed"), videos_only.clone()]));
        apply_category_outcome(&mut state, Ok(vec![mixed_shelf("Mixed"), videos_only]));

        let expected = vec![SearchResult::Track(song("s1"))];
        assert_eq!(state.explore_shelves.len(), 1);
        assert_eq!(state.explore_shelves[0].items, expected);
        assert_eq!(state.category_shelves.len(), 1);
        assert_eq!(state.category_shelves[0].items, expected);
    }

    #[test]
    fn open_detail_hides_video_tracks_when_enabled() {
        let detail =
            Detail::new(DetailKind::Album, "Album", "Artist", vec![video("v1"), song("s1")]);
        let catalog = MockLibraryCatalog::default().with_detail(Ok(Some(detail)));
        let mut state = test_wired();
        state.songs_only = true;
        state.catalog = wrap_mock_catalog(catalog);

        state.open_detail("album-1", DetailKind::Album);

        let detail = state.detail.expect("detail");
        assert_eq!(detail.tracks, vec![song("s1")]);
    }
}
