//! Search page view.

use eframe::egui;

use crate::app::AppState;
use crate::app::pages::home::render_home_shelves;
use crate::domain::DesignTokens;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

fn render_search_error(ui: &mut egui::Ui, error: Option<&str>) {
    let Some(err) = error else {
        return;
    };
    ui.colored_label(egui::Color32::from_rgb(220, 50, 50), err);
}

fn render_search_controls(ui: &mut egui::Ui, lyrics_mode: &mut bool) {
    ui.horizontal(|ui| {
        ui.checkbox(lyrics_mode, "Lyrics");
    });
}

/// Renders the search page view with search input, lyrics mode toggle, and catalog shelves.
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
        render_search_controls(ui, &mut state.lyrics_mode);
        ui.label(
            egui::RichText::new("(stub returns fixed hits)")
                .size(tokens.font_caption)
                .color(ui.visuals().weak_text_color()),
        );
    });
    render_search_error(ui, state.search_error.as_deref().or(state.lyrics_error.as_deref()));
}
