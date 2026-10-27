//! Search page view.

use eframe::egui;

use crate::app::AppState;
use crate::app::pages::home::render_home_shelves;
use crate::domain::DesignTokens;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

/// Renders the search page view with search input and catalog shelves.
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
    let tokens = DesignTokens::desktop();
    render_search_bar(ui, state, &tokens);
    ui.add_space(tokens.space_sm);
    ui.separator();
    ui.add_space(tokens.space_xs);
    state.ensure_home();
    render_home_shelves(ui, state);
}

fn render_search_bar<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
    tokens: &DesignTokens,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Search:").size(tokens.font_body));
        let response = ui.text_edit_singleline(&mut state.search_text);
        state.text_focused = response.has_focus();
        let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
        let submitted = response.lost_focus() && enter_pressed;
        if ui.button("Search").clicked() || submitted {
            state.submit_search();
        }
        ui.label(
            egui::RichText::new("(stub returns fixed hits)")
                .size(tokens.font_caption)
                .color(ui.visuals().weak_text_color()),
        );
    });
}
