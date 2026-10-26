//! Home shelves page view.

use eframe::egui;

use crate::app::{AppState, item_detail_target, shelf_item_display};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

/// Renders home catalog shelves with browsing links.
pub(crate) fn render_home_shelves<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.horizontal(|ui| {
        ui.heading("Home");
        if ui.button("Refresh").clicked() {
            state.reload_home();
        }
    });
    let mut to_open = None;
    for shelf in &state.home_shelves {
        ui.heading(&shelf.title);
        for item in &shelf.items {
            ui.horizontal(|ui| {
                ui.label(shelf_item_display(item));
                if let Some((id, kind)) = item_detail_target(item) {
                    if ui.button("Open").clicked() {
                        to_open = Some((id, kind));
                    }
                }
            });
        }
    }
    if let Some((id, kind)) = to_open {
        state.open_detail(&id, kind);
    }
}
