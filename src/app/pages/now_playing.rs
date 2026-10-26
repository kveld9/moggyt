//! Now playing page view.

use eframe::egui;

use crate::app::{AppState, format_lyric_timestamp};
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

/// Renders the now playing page view with metadata, radio, cover art, and lyrics.
pub(crate) fn render_now_playing<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    let Some(track) = state.queue.current().cloned() else {
        debug_assert!(state.queue.is_empty());
        ui.label("Queue is empty. Search and enqueue a track first.");
        return;
    };
    state.refresh_covers(ui.ctx());
    if state.lyrics_key.as_ref().is_none_or(|k| k.0 != track.artist || k.1 != track.title) {
        state.refresh_lyrics();
    }
    ui.heading(track.display());
    ui.label(format!("by {}", track.artist));
    ui.separator();
    ui.label(format!("requested: {}", state.audio.last_requested().unwrap_or("none")));
    if ui.button("Start radio").clicked() {
        state.start_radio(track.clone());
    }
    ui.separator();
    render_cover_art_section(ui, state.cover_texture.as_ref());
    ui.separator();
    render_lyrics_panel(ui, state);
}

/// Renders cover art texture or placeholder when unavailable.
pub(crate) fn render_cover_art_section(
    ui: &mut egui::Ui,
    texture: Option<&(String, egui::TextureHandle)>,
) {
    if let Some((_, tex)) = texture {
        ui.image((tex.id(), egui::Vec2::new(256.0, 256.0)));
    } else {
        ui.label("Cover art unavailable (offline or pending fetch)");
    }
}

/// Renders synced lyrics panel with time offset adjustment controls.
pub(crate) fn render_lyrics_panel<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    ui.heading("Lyrics");
    ui.horizontal(|ui| {
        ui.label(format!("Offset: {}ms", state.lyrics_offset_ms));
        if ui.button("-250ms").clicked() {
            state.nudge_lyrics_offset(-250);
        }
        if ui.button("+250ms").clicked() {
            state.nudge_lyrics_offset(250);
        }
    });
    let lines = state.visible_lyric_lines();
    if lines.is_empty() {
        ui.label("No synced lyrics for this track.");
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (ms, text) in lines {
            ui.label(format!("{} {text}", format_lyric_timestamp(ms)));
        }
    });
}
