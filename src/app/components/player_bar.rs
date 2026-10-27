//! Bottom player bar component.

use eframe::egui;

use crate::app::AppState;
use crate::domain::DesignTokens;
use crate::infra::{AudioBackend, Catalog, CoverCache, LyricsSource, SessionStore};

/// Renders the bottom persistent player bar panel with controls and status info.
pub(crate) fn render_player_bar<
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
    egui::Panel::bottom("player_bar").show(ui, |ui| {
        ui.add_space(tokens.space_xxs);
        ui.horizontal(|ui| {
            render_track_info(ui, state);
            ui.separator();
            render_position_info(ui, state);
            ui.separator();
            render_playback_controls(ui, state);
            ui.separator();
            render_seek_controls(ui, state);
            ui.separator();
            render_volume_controls(ui, state);
        });
        ui.add_space(tokens.space_xxs);
    });
}

/// Renders active track title or queue empty state.
pub(crate) fn render_track_info<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S, L, K>,
) {
    let tokens = DesignTokens::desktop();
    let (title, is_empty) = match state.queue.current() {
        Some(t) => (t.display(), false),
        None => ("Queue empty".to_owned(), true),
    };
    let text = egui::RichText::new(title).size(tokens.font_body);
    if is_empty {
        ui.label(text.color(ui.visuals().weak_text_color()));
    } else {
        ui.label(text);
    }
}

/// Renders playback controls (previous, toggle play/pause, next).
pub(crate) fn render_playback_controls<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    if ui.button("Previous").clicked() {
        state.prev_track();
    }
    render_play_pause_button(ui, state);
    if ui.button("Next").clicked() {
        state.next_track();
    }
}

fn render_play_pause_button<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    let toggle_label = if state.audio.is_playing() { "Pause" } else { "Play" };
    if ui.button(toggle_label).clicked() {
        state.toggle_play();
    }
}

/// Renders relative seek buttons (-10s, +10s).
pub(crate) fn render_seek_controls<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    if ui.button("-10s").clicked() {
        state.seek_by(-10);
    }
    if ui.button("+10s").clicked() {
        state.seek_by(10);
    }
}

/// Renders volume slider control.
pub(crate) fn render_volume_controls<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    let mut vol = state.audio.volume();
    if ui.add(egui::Slider::new(&mut vol, 0..=100).text("Vol")).changed() {
        state.set_volume(vol);
    }
}

/// Renders current playback position and total track duration.
pub(crate) fn render_position_info<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &AppState<C, A, S, L, K>,
) {
    let tokens = DesignTokens::desktop();
    let pos = state.audio.position_secs();
    let text = match state.queue.current().and_then(|t| t.duration_secs) {
        Some(dur) => format!("{pos}s / {dur}s"),
        None => format!("{pos}s"),
    };
    ui.label(
        egui::RichText::new(text).size(tokens.font_caption).color(ui.visuals().weak_text_color()),
    );
}
