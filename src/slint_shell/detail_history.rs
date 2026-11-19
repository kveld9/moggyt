//! Detail container navigation history restored by the back action.
//!
//! Hexagonal invariant: pure in-memory navigation state with zero I/O.

use crate::domain::{Detail, DetailKind};
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::view_phase::ViewPhase;

/// One visited detail container, captured whole so Back restores it without reloading.
#[derive(Debug, Clone)]
pub struct DetailVisit {
    /// Browse id and kind of the visited container.
    pub request: (String, DetailKind),
    /// Container model rendered when the visit was left.
    pub detail: Option<Detail>,
    /// View phase of the container when the visit was left.
    pub phase: ViewPhase,
}

/// Last-in-first-out stack of visited detail containers.
#[derive(Debug, Default)]
pub struct DetailHistory {
    visits: Vec<DetailVisit>,
}

impl DetailHistory {
    /// Records a visit on top of the stack.
    pub fn push(&mut self, visit: DetailVisit) {
        self.visits.push(visit);
    }

    /// Removes and returns the most recently recorded visit.
    pub fn pop(&mut self) -> Option<DetailVisit> {
        self.visits.pop()
    }

    /// Drops every recorded visit.
    pub fn clear(&mut self) {
        self.visits.clear();
    }

    /// Number of recorded visits.
    pub fn len(&self) -> usize {
        self.visits.len()
    }

    /// Whether no visit is recorded.
    pub fn is_empty(&self) -> bool {
        self.visits.is_empty()
    }
}

impl SlintAppState {
    /// Switches to the detail container identified by `request`.
    ///
    /// Opening a container from another detail page records the open one so Back returns
    /// to it; opening from any other page makes that page the exit target and drops the
    /// stale history. Re-opening the container already shown records nothing, so a retry
    /// never stacks a duplicate visit.
    pub(crate) fn record_detail_navigation(&mut self, request: (String, DetailKind)) {
        if self.page != SlintPage::Detail {
            self.detail_return = self.page;
            self.detail_history.clear();
        } else if self.detail_request.as_ref() != Some(&request) {
            if let Some(previous) = self.detail_request.clone() {
                self.detail_history.push(DetailVisit {
                    request: previous,
                    detail: self.detail.clone(),
                    phase: self.detail_phase,
                });
            }
        }
        self.page = SlintPage::Detail;
        self.detail_request = Some(request);
    }

    /// Returns from the open detail to the previous container, or to the page that opened it.
    ///
    /// Bumps the detail generation so an in-flight load of the container being left cannot
    /// overwrite the restored view when its response arrives.
    pub(crate) fn pop_detail_history(&mut self) {
        self.detail_generation = self.detail_generation.wrapping_add(1);
        let Some(visit) = self.detail_history.pop() else {
            self.page = self.detail_return;
            return;
        };
        self.detail = visit.detail;
        self.detail_phase = visit.phase;
        self.detail_request = Some(visit.request);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::test_wired;

    fn visit(id: &str, kind: DetailKind) -> DetailVisit {
        DetailVisit { request: (id.to_owned(), kind), detail: None, phase: ViewPhase::Ready }
    }

    #[test]
    fn history_pops_visits_last_in_first_out() {
        let mut history = DetailHistory::default();
        assert!(history.is_empty());
        history.push(visit("artist-1", DetailKind::Artist));
        history.push(visit("album-1", DetailKind::Album));
        assert_eq!(history.len(), 2);
        assert_eq!(history.pop().map(|visit| visit.request.0), Some("album-1".to_owned()));
        assert_eq!(history.pop().map(|visit| visit.request.0), Some("artist-1".to_owned()));
        assert!(history.pop().is_none());
        history.push(visit("playlist-1", DetailKind::Playlist));
        history.clear();
        assert!(history.is_empty());
    }

    #[test]
    fn back_restores_the_previous_detail_container() {
        let mut state = test_wired();
        state.open_detail("artist-1", DetailKind::Artist);
        state.open_detail("album-1", DetailKind::Album);
        assert_eq!(state.detail_request, Some(("album-1".to_owned(), DetailKind::Album)));
        assert_eq!(state.detail_history.len(), 1);

        state.go_back();

        assert_eq!(state.page, SlintPage::Detail);
        assert_eq!(state.detail_request, Some(("artist-1".to_owned(), DetailKind::Artist)));
        assert_eq!(
            state.detail.as_ref().map(|detail| detail.title.as_str()),
            Some("artist-1 Title")
        );
        assert_eq!(state.detail_phase, ViewPhase::Ready);
        assert!(state.detail_history.is_empty());

        state.go_back();
        assert_eq!(state.page, SlintPage::Search);
    }

    #[test]
    fn opening_a_container_from_a_root_page_clears_previous_history() {
        let mut state = test_wired();
        state.open_detail("artist-1", DetailKind::Artist);
        state.open_detail("album-1", DetailKind::Album);
        assert_eq!(state.detail_history.len(), 1);

        state.select_page(SlintPage::Library);
        state.open_detail("playlist-1", DetailKind::Playlist);
        assert!(state.detail_history.is_empty());
        assert_eq!(state.detail_return, SlintPage::Library);

        state.go_back();
        assert_eq!(state.page, SlintPage::Library);
    }

    #[test]
    fn reopening_the_open_container_records_no_duplicate_visit() {
        let mut state = test_wired();
        state.open_detail("artist-1", DetailKind::Artist);
        state.open_detail("album-1", DetailKind::Album);
        state.open_detail("album-1", DetailKind::Album);
        assert_eq!(state.detail_history.len(), 1);
    }

    #[test]
    fn back_invalidates_the_load_of_the_container_it_leaves() {
        let mut state = test_wired();
        state.open_detail("artist-1", DetailKind::Artist);
        state.open_detail("album-1", DetailKind::Album);
        let album_generation = state.detail_generation;

        state.go_back();

        assert_ne!(state.detail_generation, album_generation);
    }
}
