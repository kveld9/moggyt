//! Search state extensions for SlintAppState.

use super::state::SlintAppState;
use crate::domain::{SearchResult, best_match};

impl SlintAppState {
    /// Clears the lyrics-search mode when a new catalog search starts.
    pub fn clear_lyrics_search(&mut self) {
        self.lyrics_search = false;
    }

    /// Promotes the best catalog match for the active query and removes it from the shelves
    /// so the result list never duplicates the top-result card.
    pub fn split_search_top_result(&mut self) {
        self.search_top_result = None;
        if self.lyrics_search || !self.search_active {
            return;
        }
        let query = self.search_query.clone();
        let items: Vec<SearchResult> =
            self.home_shelves.iter().flat_map(|shelf| shelf.items.iter().cloned()).collect();
        let Some(best) = best_match(&query, &items) else { return };
        for shelf in &mut self.home_shelves {
            if let Some(position) = shelf.items.iter().position(|item| item == &best) {
                shelf.items.remove(position);
                break;
            }
        }
        self.search_results = self
            .home_shelves
            .iter()
            .flat_map(|shelf| shelf.items.iter())
            .filter_map(|item| match item {
                SearchResult::Track(track) => Some(track.clone()),
                _ => None,
            })
            .collect();
        self.search_top_result = Some(best);
    }

    /// Whether the search view has anything to render, including the top-result card.
    pub fn has_search_results(&self) -> bool {
        self.search_top_result.is_some()
            || self.home_shelves.iter().any(|shelf| !shelf.items.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Album, Artist, SearchFilter, Shelf, Track};
    use crate::slint_shell::sync::observe_view_phases;
    use crate::slint_shell::test_support::test_wired;
    use crate::slint_shell::view_phase::ViewPhase;

    fn shelves_with(items: Vec<SearchResult>) -> Vec<Shelf> {
        let mut shelf = Shelf::new("Search Results");
        shelf.items = items;
        vec![shelf]
    }

    #[test]
    fn test_lyrics_search_mode_clearing() {
        let mut state = test_wired();
        state.lyrics_search = true;
        state.clear_lyrics_search();
        assert!(!state.lyrics_search);
        state.lyrics_search = true;
        state.clear_search();
        assert!(!state.lyrics_search);
    }

    #[test]
    fn split_promotes_best_match_and_removes_it_from_the_shelf() {
        let mut state = test_wired();
        state.search_active = true;
        state.search_query = "celoso".to_string();
        state.home_shelves = shelves_with(vec![
            SearchResult::Track(Track::new("t1", "Celoso", "Santa Fe Klan")),
            SearchResult::Track(Track::new("t2", "Starboy", "The Weeknd")),
        ]);

        state.split_search_top_result();

        let top = state.search_top_result.clone().expect("top result");
        assert_eq!(top, SearchResult::Track(Track::new("t1", "Celoso", "Santa Fe Klan")));
        assert_eq!(state.home_shelves[0].items.len(), 1);
        assert_eq!(state.search_results.len(), 1);
        assert_eq!(state.search_results[0].id, "t2");
    }

    #[test]
    fn split_leaves_shelves_untouched_when_nothing_clears_the_threshold() {
        let mut state = test_wired();
        state.search_active = true;
        state.search_query = "midnight".to_string();
        let items = vec![SearchResult::Artist(Artist::new("a1", "Starboy", None))];
        state.home_shelves = shelves_with(items.clone());

        state.split_search_top_result();

        assert!(state.search_top_result.is_none());
        assert_eq!(state.home_shelves[0].items, items);
    }

    #[test]
    fn split_is_disabled_for_lyrics_search() {
        let mut state = test_wired();
        state.search_active = true;
        state.lyrics_search = true;
        state.search_query = "celoso".to_string();
        let items = vec![SearchResult::Track(Track::new("t1", "Celoso", "Santa Fe Klan"))];
        state.home_shelves = shelves_with(items.clone());

        state.split_search_top_result();

        assert!(state.search_top_result.is_none());
        assert_eq!(state.home_shelves[0].items, items);
    }

    #[test]
    fn top_result_alone_keeps_the_search_view_ready() {
        let mut state = test_wired();
        state.search_active = true;
        state.search_query = "album".to_string();
        state.search_filter = SearchFilter::All;
        state.home_shelves =
            shelves_with(vec![SearchResult::Album(Album::new("al1", "Album", "Artist", None))]);

        state.split_search_top_result();
        observe_view_phases(&mut state);

        assert_eq!(state.home_shelves[0].items.len(), 0);
        assert!(state.search_top_result.is_some());
        assert!(state.has_search_results());
        assert_eq!(state.search_phase, ViewPhase::Ready);
    }

    #[test]
    fn empty_search_has_no_results_to_render() {
        let mut state = test_wired();
        state.home_shelves = shelves_with(Vec::new());
        assert!(!state.has_search_results());
    }
}
