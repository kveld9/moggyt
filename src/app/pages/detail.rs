//! Detail page view.

use eframe::egui;

use crate::app::AppState;
use crate::domain::Track;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

/// Renders the detail container view with navigation back button and track list.
pub(crate) fn render_detail<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    if ui.button("Back").clicked() {
        state.close_detail();
        return;
    }
    let Some(detail) = state.detail.clone() else {
        ui.label("No detail available.");
        return;
    };
    ui.heading(&detail.title);
    if !detail.subtitle.is_empty() {
        ui.label(&detail.subtitle);
    }
    ui.separator();
    render_detail_tracks(ui, state, &detail.tracks);
}

/// Renders tracks belonging to an album, artist, or playlist detail view.
pub(crate) fn render_detail_tracks<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
    tracks: &[Track],
) {
    if tracks.is_empty() {
        ui.label("No tracks available.");
        return;
    }
    let mut to_play = None;
    for track in tracks {
        ui.horizontal(|ui| {
            let dur_text = track.duration_secs.map_or_else(String::new, |d| format!(" ({d}s)"));
            ui.label(format!("{} - {}{dur_text}", track.title, track.artist));
            if ui.button("Play").clicked() {
                to_play = Some(track.clone());
            }
        });
    }
    if let Some(track) = to_play {
        state.queue.push(track);
        state.play_head();
    }
}
