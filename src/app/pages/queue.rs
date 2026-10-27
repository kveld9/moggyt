//! Playback queue page view.

use eframe::egui;

use crate::app::AppState;
use crate::app::components::{empty_state, track_row};
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
    ui.label(format!("{} track(s) queued", state.queue.len()));
    render_queue_controls(ui, state);
    render_queue_tracks(ui, state);
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
) {
    ui.horizontal(|ui| {
        if ui.button("Previous").clicked() {
            state.queue.retreat();
        }
        if ui.button("Play head").clicked() {
            state.play_head();
        }
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
    for track in state.queue.tracks() {
        let is_active = state.queue.current().is_some_and(|cur| cur.id == track.id);
        track_row(ui, &track.title, &track.artist, is_active);
    }
}
