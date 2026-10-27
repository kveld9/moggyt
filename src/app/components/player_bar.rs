//! Bottom player bar component.

use eframe::egui;

use crate::app::AppState;
use crate::domain::DesignTokens;
use crate::domain::TimerPreset;
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
            ui.separator();
            render_sleep_timer_controls(ui, state);
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

/// Renders sleep timer preset selector, countdown display, and cancel button.
pub(crate) fn render_sleep_timer_controls<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    render_sleep_timer_preset_menu(ui, state);
    if state.sleep_timer.is_armed() {
        render_sleep_timer_armed_info(ui, state);
    }
}

fn render_sleep_timer_preset_menu<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    let label = state.sleep_timer.preset().display_name();
    egui::ComboBox::from_id_salt("sleep_timer_dropdown").selected_text(label).show_ui(ui, |ui| {
        render_preset_option(ui, state, TimerPreset::Off, "Off");
        render_preset_option(ui, state, TimerPreset::Minutes15, "15 min");
        render_preset_option(ui, state, TimerPreset::Minutes30, "30 min");
        render_preset_option(ui, state, TimerPreset::Minutes45, "45 min");
        render_preset_option(ui, state, TimerPreset::Minutes60, "60 min");
        render_preset_option(ui, state, TimerPreset::StopAfterTrack, "After track");
    });
}

fn render_preset_option<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
    preset: TimerPreset,
    label: &str,
) {
    let is_selected = state.sleep_timer.preset() == preset;
    if ui.selectable_label(is_selected, label).clicked() && !is_selected {
        state.set_sleep_timer_preset(preset);
    }
}

fn render_sleep_timer_armed_info<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
) {
    if let Some(remaining) = state.sleep_timer.remaining_secs() {
        let mins = remaining / 60;
        let secs = remaining % 60;
        ui.label(format!("{mins}:{secs:02}"));
    } else {
        ui.label("After track");
    }
    if ui.button("Cancel").clicked() {
        state.cancel_sleep_timer();
    }
}
