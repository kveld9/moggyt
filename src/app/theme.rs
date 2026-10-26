//! Application theme mapping: converts domain [`Palette`] and [`DesignTokens`] to [`egui::Visuals`].
//!
//! Hexagonal invariant: this adapter lives strictly in `app` and translates pure
//! domain styling data ([`crate::domain::Palette`], [`crate::domain::DesignTokens`])
//! to UI framework types ([`egui::Visuals`], [`egui::Style`]).

use eframe::egui;

use crate::domain::{DesignTokens, Palette};

/// Maps a domain [`Palette`] and default [`DesignTokens`] into [`egui::Visuals`] and applies it to context.
pub fn apply(ctx: &egui::Context, palette: &Palette) {
    apply_with_tokens(ctx, palette, &DesignTokens::desktop());
}

/// Maps a domain [`Palette`] and explicit [`DesignTokens`] into [`egui::Visuals`] and [`egui::Style`].
pub fn apply_with_tokens(ctx: &egui::Context, palette: &Palette, tokens: &DesignTokens) {
    let mut visuals = base_visuals(palette.dark);
    apply_palette_colors(&mut visuals, palette);
    apply_corner_radius(&mut visuals, tokens);
    ctx.set_visuals(visuals);
    apply_spacing(ctx, tokens);
}

fn base_visuals(dark: bool) -> egui::Visuals {
    if dark { egui::Visuals::dark() } else { egui::Visuals::light() }
}

fn apply_palette_colors(visuals: &mut egui::Visuals, palette: &Palette) {
    let window_color = color32(palette.window);
    let panel_color = color32(palette.panel);
    let surface_color = color32(palette.surface);
    let text_color = color32(palette.text);
    let accent_color = color32(palette.accent);
    let border_color = color32(palette.border);
    let hover_color = color32(palette.hover);
    let overlay_color = color32(palette.overlay);

    visuals.panel_fill = panel_color;
    visuals.window_fill = window_color;
    visuals.extreme_bg_color = surface_color;
    visuals.faint_bg_color = overlay_color;
    visuals.override_text_color = Some(text_color);
    visuals.selection.bg_fill = accent_color;
    visuals.window_stroke = egui::Stroke::new(1.0, border_color);

    visuals.widgets.noninteractive.bg_fill = panel_color;
    visuals.widgets.noninteractive.fg_stroke.color = text_color;
    visuals.widgets.inactive.bg_fill = surface_color;
    visuals.widgets.hovered.bg_fill = hover_color;
    visuals.widgets.active.bg_fill = accent_color;
    visuals.widgets.open.bg_fill = overlay_color;
}

fn apply_corner_radius(visuals: &mut egui::Visuals, tokens: &DesignTokens) {
    let widget_radius = egui::CornerRadius::same(tokens.radius_sm as u8);
    let window_radius = egui::CornerRadius::same(tokens.radius_md as u8);

    visuals.window_corner_radius = window_radius;
    visuals.menu_corner_radius = window_radius;
    visuals.widgets.noninteractive.corner_radius = widget_radius;
    visuals.widgets.inactive.corner_radius = widget_radius;
    visuals.widgets.hovered.corner_radius = widget_radius;
    visuals.widgets.active.corner_radius = widget_radius;
    visuals.widgets.open.corner_radius = widget_radius;
}

fn apply_spacing(ctx: &egui::Context, tokens: &DesignTokens) {
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(tokens.space_xs, tokens.space_xxs);
        style.spacing.window_margin = egui::Margin::same(tokens.space_xs as i8);
    });
}

fn color32(rgb: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_palette_colors_sets_visuals_fields_deterministically() {
        let palette = Palette::black_oled();
        let mut visuals = base_visuals(palette.dark);
        apply_palette_colors(&mut visuals, &palette);

        assert_eq!(visuals.window_fill, egui::Color32::from_rgb(0, 0, 0));
        assert_eq!(visuals.panel_fill, egui::Color32::from_rgb(12, 12, 12));
        assert_eq!(visuals.override_text_color, Some(egui::Color32::from_rgb(240, 240, 240)));
        assert_eq!(visuals.window_stroke.color, egui::Color32::from_rgb(36, 36, 36));
        assert_eq!(visuals.widgets.hovered.bg_fill, egui::Color32::from_rgb(28, 28, 28));
        assert_eq!(visuals.widgets.open.bg_fill, egui::Color32::from_rgb(24, 24, 24));
        assert_eq!(visuals.faint_bg_color, egui::Color32::from_rgb(24, 24, 24));
    }

    #[test]
    fn apply_corner_radius_sets_widget_and_window_radii() {
        let tokens = DesignTokens::desktop();
        let mut visuals = base_visuals(true);
        apply_corner_radius(&mut visuals, &tokens);

        assert_eq!(visuals.window_corner_radius, egui::CornerRadius::same(8));
        assert_eq!(visuals.menu_corner_radius, egui::CornerRadius::same(8));
        assert_eq!(visuals.widgets.noninteractive.corner_radius, egui::CornerRadius::same(4));
        assert_eq!(visuals.widgets.inactive.corner_radius, egui::CornerRadius::same(4));
        assert_eq!(visuals.widgets.hovered.corner_radius, egui::CornerRadius::same(4));
        assert_eq!(visuals.widgets.active.corner_radius, egui::CornerRadius::same(4));
        assert_eq!(visuals.widgets.open.corner_radius, egui::CornerRadius::same(4));
    }

    #[test]
    fn apply_with_tokens_configures_context_visuals_and_spacing() {
        let ctx = egui::Context::default();
        ctx.set_theme(egui::Theme::Light);
        let palette = Palette::white();
        let tokens = DesignTokens::desktop();
        apply_with_tokens(&ctx, &palette, &tokens);

        let style = ctx.style_of(egui::Theme::Light);
        assert_eq!(style.visuals.panel_fill, egui::Color32::from_rgb(240, 240, 240));
        assert_eq!(style.spacing.item_spacing, egui::vec2(8.0, 4.0));
        assert_eq!(style.spacing.window_margin, egui::Margin::same(8));
    }
}
