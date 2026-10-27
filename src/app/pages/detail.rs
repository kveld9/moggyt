//! Detail page view.

use eframe::egui;

use crate::app::AppState;
use crate::app::components::{empty_state, track_row};
use crate::domain::{DesignTokens, Detail, DetailKind, Track};
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
    let tokens = DesignTokens::desktop();
    if ui.button("Back").clicked() {
        state.close_detail();
        return;
    }
    ui.add_space(tokens.space_xs);
    let Some(detail) = state.detail.clone() else {
        empty_state(ui, "No detail available.");
        return;
    };
    render_detail_header(ui, &detail, &tokens);
    ui.add_space(tokens.space_sm);
    ui.separator();
    ui.add_space(tokens.space_xs);
    render_detail_tracks(ui, state, &detail.tracks);
}

fn render_detail_header(ui: &mut egui::Ui, detail: &Detail, tokens: &DesignTokens) {
    ui.label(
        egui::RichText::new(detail_kind_caption(detail.kind))
            .size(tokens.font_caption)
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(tokens.space_xxs);
    ui.label(egui::RichText::new(&detail.title).size(tokens.font_display).strong());
    if !detail.subtitle.is_empty() {
        ui.add_space(tokens.space_xxs);
        ui.label(
            egui::RichText::new(&detail.subtitle)
                .size(tokens.font_body)
                .color(ui.visuals().weak_text_color()),
        );
    }
}

fn detail_kind_caption(kind: DetailKind) -> &'static str {
    match kind {
        DetailKind::Album => "Album",
        DetailKind::Artist => "Artist",
        DetailKind::Playlist => "Playlist",
    }
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
        empty_state(ui, "No tracks available.");
        return;
    }
    let to_play = render_detail_track_rows(ui, state, tracks);
    if let Some(track) = to_play {
        state.queue.push(track);
        state.play_head();
    }
}

fn render_detail_track_rows<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S, L, K>,
    tracks: &[Track],
) -> Option<Track> {
    let mut to_play = None;
    let current_id = state.queue.current().map(|cur| cur.id.as_str());
    for track in tracks {
        let is_active = current_id == Some(track.id.as_str());
        if render_detail_track_row(ui, track, is_active) {
            to_play = Some(track.clone());
        }
    }
    to_play
}

fn render_detail_track_row(ui: &mut egui::Ui, track: &Track, is_active: bool) -> bool {
    let artist_display = track_artist_display(track);
    let mut play_clicked = false;
    ui.horizontal(|ui| {
        let row_response = track_row(ui, &track.title, &artist_display, is_active);
        if row_response.clicked() || ui.button("Play").clicked() {
            play_clicked = true;
        }
    });
    play_clicked
}

fn track_artist_display(track: &Track) -> String {
    match track.duration_secs {
        Some(secs) => format!("{} ({secs}s)", track.artist),
        None => track.artist.clone(),
    }
}
