//! Top navigation bar component.

use eframe::egui;

use crate::app::{AppState, Page, UpdateStatus};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

/// Renders the top navigation bar panel with page selectors and update status.
pub(crate) fn render_top_bar<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading("moggyt");
            ui.separator();
            ui.selectable_value(&mut state.page, Page::Search, "Search");
            ui.selectable_value(&mut state.page, Page::Queue, "Queue");
            ui.selectable_value(&mut state.page, Page::NowPlaying, "Now playing");
            ui.selectable_value(&mut state.page, Page::Library, "Library");
            ui.selectable_value(&mut state.page, Page::Detail, "Detail");
            render_top_bar_update_badge(ui, &state.update_status);
        });
    });
}

/// Renders the top bar update notice badge when a newer release is available.
pub(crate) fn render_top_bar_update_badge(ui: &mut egui::Ui, status: &UpdateStatus) {
    let UpdateStatus::Available { version, .. } = status else {
        return;
    };
    // Keep top bar lean with just a colored notice; interactive download link lives in Library.
    let display_tag = if version.starts_with(['v', 'V']) {
        format!("Update {version}")
    } else {
        format!("Update v{version}")
    };
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.colored_label(egui::Color32::from_rgb(255, 180, 50), display_tag);
    });
}
