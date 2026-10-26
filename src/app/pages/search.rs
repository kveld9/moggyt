//! Search page view.

use eframe::egui;

use crate::app::AppState;
use crate::app::pages::home::render_home_shelves;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

/// Renders the search page view with home shelves and search input.
pub(crate) fn render_search<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    state.ensure_home();
    render_home_shelves(ui, state);
    ui.separator();
    ui.label("Search the catalog (stub returns fixed hits):");
    let response = ui.text_edit_singleline(&mut state.search_text);
    state.text_focused = response.has_focus();
    let submitted = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    if ui.button("Search").clicked() || submitted {
        state.submit_search();
    }
}
