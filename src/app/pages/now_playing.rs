//! Now playing page view.

use eframe::egui;

use crate::app::components::{empty_state, section_header};
use crate::app::{AppState, format_lyric_timestamp};
use crate::domain::{DesignTokens, Track};
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
        empty_state(ui, "Queue is empty. Search and enqueue a track first.");
        return;
    };
    state.refresh_covers(ui.ctx());
    if state.lyrics_key.as_ref().is_none_or(|k| k.0 != track.artist || k.1 != track.title) {
        state.refresh_lyrics();
    }
    render_hero_section(ui, state, &track);
    ui.add_space(DesignTokens::desktop().space_lg);
    ui.separator();
    ui.add_space(DesignTokens::desktop().space_xs);
    render_lyrics_panel(ui, state);
}

fn render_hero_section<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
    track: &Track,
) {
    let tokens = DesignTokens::desktop();
    ui.horizontal(|ui| {
        render_cover_art_section(ui, state.cover_texture.as_ref());
        ui.add_space(tokens.space_md);
        render_track_details(ui, state, track);
    });
}

fn render_track_details<
    C: Catalog,
    A: AudioBackend,
    S: SessionStore,
    L: LyricsSource,
    K: CoverCache,
>(
    ui: &mut egui::Ui,
    state: &mut AppState<C, A, S, L, K>,
    track: &Track,
) {
    let tokens = DesignTokens::desktop();
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(track.display()).size(tokens.font_display).strong());
        ui.add_space(tokens.space_xxs);
        ui.label(
            egui::RichText::new(format!("by {}", track.artist))
                .size(tokens.font_body)
                .color(ui.visuals().weak_text_color()),
        );
        ui.add_space(tokens.space_xs);
        if ui.button("Start radio").clicked() {
            state.start_radio(track.clone());
        }
        ui.add_space(tokens.space_xs);
        render_offset_controls(ui, state);
        ui.add_space(tokens.space_xxs);
        let requested = state.audio.last_requested().unwrap_or("none");
        ui.label(
            egui::RichText::new(format!("requested: {requested}"))
                .size(tokens.font_caption)
                .color(ui.visuals().weak_text_color()),
        );
    });
}

fn render_offset_controls<
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
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("Offset: {}ms", state.lyrics_offset_ms))
                .size(tokens.font_caption)
                .color(ui.visuals().weak_text_color()),
        );
        if ui.button("-250ms").clicked() {
            state.nudge_lyrics_offset(-250);
        }
        if ui.button("+250ms").clicked() {
            state.nudge_lyrics_offset(250);
        }
    });
}

/// Renders cover art texture or placeholder when unavailable.
pub(crate) fn render_cover_art_section(
    ui: &mut egui::Ui,
    texture: Option<&(String, egui::TextureHandle)>,
) {
    let tokens = DesignTokens::desktop();
    let size = egui::vec2(256.0, 256.0);
    let radius = egui::CornerRadius::same(tokens.radius_md as u8);
    let frame = egui::Frame::new()
        .fill(ui.visuals().widgets.noninteractive.bg_fill)
        .stroke(ui.visuals().window_stroke)
        .corner_radius(radius)
        .inner_margin(egui::Margin::same(0));

    frame.show(ui, |ui| {
        ui.set_min_size(size);
        ui.set_max_size(size);
        if let Some((_, tex)) = texture {
            ui.image((tex.id(), size));
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    egui::RichText::new("Cover art unavailable")
                        .size(tokens.font_caption)
                        .color(ui.visuals().weak_text_color()),
                );
            });
        }
    });
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
    section_header(ui, "Lyrics");
    let lines = state.visible_lyric_lines();
    if lines.is_empty() {
        empty_state(ui, "No synced lyrics for this track.");
        return;
    }
    let current_ms = state.audio.position_secs().saturating_mul(1000);
    let active_idx = lines.iter().rposition(|(ms, _)| *ms <= current_ms);
    render_lyrics_lines(ui, &lines, active_idx);
}

fn render_lyrics_lines(ui: &mut egui::Ui, lines: &[(u64, String)], active_idx: Option<usize>) {
    let tokens = DesignTokens::desktop();
    let accent_color = ui.visuals().selection.bg_fill;
    let weak_color = ui.visuals().weak_text_color();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, (ms, text)) in lines.iter().enumerate() {
            let formatted = format!("{} {text}", format_lyric_timestamp(*ms));
            let is_active = Some(i) == active_idx;
            let rich = if is_active {
                egui::RichText::new(formatted).size(tokens.font_body).color(accent_color).strong()
            } else {
                egui::RichText::new(formatted).size(tokens.font_body).color(weak_color)
            };
            ui.label(rich);
        }
    });
}
