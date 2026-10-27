//! Single-line track row component.

use eframe::egui;

use crate::domain::DesignTokens;

/// Renders a single-line track item with body title, caption artist, and active highlight.
pub(crate) fn track_row(
    ui: &mut egui::Ui,
    title: &str,
    artist: &str,
    active: bool,
) -> egui::Response {
    let tokens = DesignTokens::desktop();
    let bg_color =
        if active { ui.visuals().widgets.hovered.bg_fill } else { egui::Color32::TRANSPARENT };
    let radius = egui::CornerRadius::same(tokens.radius_sm as u8);
    let margin = egui::Margin::symmetric(tokens.space_xs as i8, tokens.space_xxs as i8);

    let frame = egui::Frame::new().fill(bg_color).corner_radius(radius).inner_margin(margin);

    let inner = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).size(tokens.font_body));
            ui.label(
                egui::RichText::new(artist)
                    .size(tokens.font_caption)
                    .color(ui.visuals().weak_text_color()),
            );
        })
    });

    ui.interact(inner.response.rect, inner.response.id, egui::Sense::click())
}
