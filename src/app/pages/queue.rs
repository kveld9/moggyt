//! Playback queue page view.

use eframe::egui;

use crate::app::AppState;
use crate::app::components::{empty_state, track_row};
use crate::domain::DesignTokens;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

/// Renders the playback queue page view with controls and track list.
pub(crate) fn render_queue<
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
    render_queue_count(ui, state.queue.len(), &tokens);
    ui.add_space(tokens.space_xs);
    render_queue_controls(ui, state, &tokens);
    ui.add_space(tokens.space_sm);
    render_queue_tracks(ui, state);
}

fn render_queue_count(ui: &mut egui::Ui, count: usize, tokens: &DesignTokens) {
    ui.label(
        egui::RichText::new(format!("{count} track(s) queued"))
            .size(tokens.font_caption)
            .color(ui.visuals().weak_text_color()),
    );
}

fn render_queue_controls<
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
        if ui.button("Previous").clicked() {
            state.queue.retreat();
        }
        ui.add_space(tokens.space_xs);
        if ui.button("Play head").clicked() {
            state.play_head();
        }
        ui.add_space(tokens.space_xs);
        if ui.button("Next").clicked() {
            state.queue.advance();
        }
    });
}

fn render_queue_tracks<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S, L, K>,
) {
    if state.queue.is_empty() {
        empty_state(ui, "No tracks queued");
        return;
    }
    let tokens = DesignTokens::desktop();
    for track in state.queue.tracks() {
        let is_active = state.queue.current().is_some_and(|cur| cur.id == track.id);
        track_row(ui, &track.title, &track.artist, is_active);
        ui.add_space(tokens.space_xxs);
    }
}
