//! Empty state list placeholder component.

use eframe::egui;

use crate::domain::DesignTokens;

/// Renders a centered empty state placeholder message with weak text coloring.
pub(crate) fn empty_state(ui: &mut egui::Ui, message: &str) {
    let tokens = DesignTokens::desktop();
    ui.add_space(tokens.space_md);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(message)
                .size(tokens.font_body)
                .color(ui.visuals().weak_text_color()),
        );
    });
    ui.add_space(tokens.space_md);
}
