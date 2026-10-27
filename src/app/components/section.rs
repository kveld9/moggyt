//! Section header component.

use eframe::egui;

use crate::domain::DesignTokens;

/// Renders a section header label with token-driven typography and bottom spacing.
pub(crate) fn section_header(ui: &mut egui::Ui, title: &str) {
    let tokens = DesignTokens::desktop();
    ui.label(egui::RichText::new(title).size(tokens.font_title_lg).strong());
    ui.add_space(tokens.space_xs);
}
