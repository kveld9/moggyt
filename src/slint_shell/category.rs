//! Category navigation and back navigation transitions for SlintAppState.
//!
//! Hexagonal invariant: pure in-memory page mutations without direct I/O.

use crate::infra::CatalogError;
use crate::slint_shell::page::SlintPage;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::view_phase::ViewPhase;

impl SlintAppState {
    /// Opens category container for moods and genres identifier.
    pub fn open_category(&mut self, browse_id: &str, params: Option<&str>, title: &str) {
        let trimmed = browse_id.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.page != SlintPage::Category {
            self.category_return = self.page;
        }
        self.category_title = title.to_owned();
        let res = self
            .catalog
            .lock()
            .map_or(Err(CatalogError::Transport), |c| c.category_shelves(trimmed, params));
        match res {
            Ok(shelves) => {
                let is_empty = shelves.is_empty();
                self.category_shelves = shelves;
                self.category_phase = if is_empty { ViewPhase::Empty } else { ViewPhase::Ready };
            }
            Err(_) => {
                self.category_phase = ViewPhase::Error;
            }
        }
        self.page = SlintPage::Category;
    }

    /// Returns from detail or category view to the previous page.
    pub fn go_back(&mut self) {
        if self.page == SlintPage::Detail {
            self.detail_error = None;
            self.page = self.detail_return;
        } else if self.page == SlintPage::Category {
            self.page = self.category_return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DetailKind;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_detail_back_navigation_transitions_to_return_page() {
        let mut state = test_wired();
        assert_eq!(state.detail_return, SlintPage::Search);

        state.open_detail("album-1", DetailKind::Album);
        state.detail_error = Some("error".to_string());
        assert_eq!(state.page, SlintPage::Detail);
        assert_eq!(state.detail_return, SlintPage::Search);
        state.go_back();
        assert_eq!(state.page, SlintPage::Search);
        assert_eq!(state.detail_error, None);

        state.select_page(SlintPage::Library);
        state.open_detail("artist-1", DetailKind::Artist);
        assert_eq!(state.page, SlintPage::Detail);
        assert_eq!(state.detail_return, SlintPage::Library);
        state.go_back();
        assert_eq!(state.page, SlintPage::Library);

        state.go_back();
        assert_eq!(state.page, SlintPage::Library);
    }

    #[test]
    fn test_category_navigation_and_back_transition() {
        let mut state = test_wired();
        assert_eq!(state.category_return, SlintPage::Explore);

        state.select_page(SlintPage::Explore);
        state.open_category("FEmusic_moods_and_genres_category", Some("calma"), "Calma");
        assert_eq!(state.page, SlintPage::Category);
        assert_eq!(state.category_title, "Calma");
        assert_eq!(state.category_return, SlintPage::Explore);

        state.open_category("FEmusic_moods_and_genres_category", Some("chill"), "Chill");
        assert_eq!(state.category_return, SlintPage::Explore);
        state.go_back();
        assert_eq!(state.page, SlintPage::Explore);
    }

    #[test]
    fn category_error_sets_error_phase_and_preserves_shelves() {
        use crate::slint_shell::test_support::MockLibraryCatalog;

        let mut state = test_wired();
        let err_cat = MockLibraryCatalog::default().with_category(Err(CatalogError::Transport));
        state.catalog = std::sync::Arc::new(std::sync::Mutex::new(Box::new(err_cat)));
        state.category_shelves =
            vec![crate::domain::Shelf { title: "Previous".into(), items: Vec::new() }];

        state.open_category("some_id", None, "Title");
        assert_eq!(state.category_phase, ViewPhase::Error);
        assert_eq!(state.category_shelves.len(), 1);
        assert_eq!(state.category_shelves[0].title, "Previous");
    }
}
