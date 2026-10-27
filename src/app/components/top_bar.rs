//! Top navigation bar component.

use eframe::egui;

use crate::app::{AppState, Page, UpdateStatus};
use crate::domain::DesignTokens;
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
    let tokens = DesignTokens::desktop();
    egui::Panel::top("top_bar").show(ui, |ui| {
        ui.add_space(tokens.space_xxs);
        ui.horizontal(|ui| {
            render_title(ui, &tokens);
            ui.add_space(tokens.space_xs);
            ui.separator();
            ui.add_space(tokens.space_xs);
            render_nav_links(ui, state, &tokens);
            render_top_bar_update_badge(ui, &state.update_status);
        });
        ui.add_space(tokens.space_xxs);
    });
}

fn render_title(ui: &mut egui::Ui, tokens: &DesignTokens) {
    ui.label(egui::RichText::new("moggyt").size(tokens.font_display).strong());
}

fn render_nav_links<
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
    for (page, label) in [
        (Page::Search, "Search"),
        (Page::Queue, "Queue"),
        (Page::NowPlaying, "Now playing"),
        (Page::Library, "Library"),
        (Page::Detail, "Detail"),
    ] {
        ui.selectable_value(&mut state.page, page, label);
        ui.add_space(tokens.space_xs);
    }
}

/// Renders the top bar update notice badge when a newer release is available.
pub(crate) fn render_top_bar_update_badge(ui: &mut egui::Ui, status: &UpdateStatus) {
    let UpdateStatus::Available { version, .. } = status else {
        return;
    };
    let tokens = DesignTokens::desktop();
    let display_tag = if version.starts_with(['v', 'V']) {
        format!("Update {version}")
    } else {
        format!("Update v{version}")
    };
    let badge_color = ui.visuals().selection.bg_fill;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(egui::RichText::new(display_tag).size(tokens.font_caption).color(badge_color));
    });
}
