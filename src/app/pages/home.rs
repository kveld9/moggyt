//! Home shelves page view.

use eframe::egui;

use crate::app::components::{empty_state, section_header, track_row};
use crate::app::{AppState, item_detail_target};
use crate::domain::{DesignTokens, DetailKind, SearchResult, Shelf};
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
    let tokens = DesignTokens::desktop();
    render_home_header(ui, state, &tokens);
    if state.home_shelves.is_empty() {
        empty_state(ui, "No shelves available.");
        return;
    }
    let to_open = render_shelves_list(ui, &state.home_shelves, &tokens);
    if let Some((id, kind)) = to_open {
        state.open_detail(&id, kind);
    }
}

fn render_home_header<
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
        ui.label(egui::RichText::new("Home").size(tokens.font_title_lg).strong());
        if ui.button("Refresh").clicked() {
            state.reload_home();
        }
    });
    ui.add_space(tokens.space_xs);
}

fn render_shelves_list(
    ui: &mut egui::Ui,
    shelves: &[Shelf],
    tokens: &DesignTokens,
) -> Option<(String, DetailKind)> {
    let mut to_open = None;
    for shelf in shelves {
        section_header(ui, &shelf.title);
        if shelf.items.is_empty() {
            empty_state(ui, "No items available.");
        } else {
            render_shelf_items(ui, &shelf.items, &mut to_open);
        }
        ui.add_space(tokens.space_sm);
    }
    to_open
}

fn render_shelf_items(
    ui: &mut egui::Ui,
    items: &[SearchResult],
    to_open: &mut Option<(String, DetailKind)>,
) {
    for item in items {
        if let Some(target) = render_shelf_item(ui, item) {
            *to_open = Some(target);
        }
    }
}

fn render_shelf_item(ui: &mut egui::Ui, item: &SearchResult) -> Option<(String, DetailKind)> {
    let (title, subtitle) = shelf_item_parts(item);
    let target = item_detail_target(item);
    let mut selected = None;

    ui.horizontal(|ui| {
        let response = track_row(ui, title, subtitle, false);
        let button_clicked = if target.is_some() { ui.button("Open").clicked() } else { false };
        if response.clicked() || button_clicked {
            selected = target;
        }
    });

    selected
}

fn shelf_item_parts(item: &SearchResult) -> (&str, &str) {
    match item {
        SearchResult::Track(track) => (&track.title, &track.artist),
        SearchResult::Album(album) => {
            let subtitle = if album.artist.is_empty() { "Album" } else { &album.artist };
            (&album.title, subtitle)
        }
        SearchResult::Artist(artist) => (&artist.name, "Artist"),
        SearchResult::Playlist(playlist) => (&playlist.title, "Playlist"),
    }
}
