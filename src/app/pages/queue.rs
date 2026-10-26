//! Playback queue page view.

use eframe::egui;

use crate::app::AppState;
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
    for track in state.queue.tracks() {
        ui.label(track.display());
    }
}
