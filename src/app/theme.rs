//! Application theme mapping: converts domain [`Palette`] to [`egui::Visuals`].
//!
//! Hexagonal invariant: this adapter lives strictly in `app` and translates pure
//! domain color data ([`crate::domain::Palette`]) to UI framework types ([`egui::Visuals`]).

use eframe::egui;

use crate::domain::Palette;

/// Maps a domain [`Palette`] into [`egui::Visuals`] and applies it to the context.
pub fn apply(ctx: &egui::Context, palette: &Palette) {
    let mut visuals = base_visuals(palette.dark);
    apply_palette_colors(&mut visuals, palette);
    ctx.set_visuals(visuals);
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

    visuals.panel_fill = panel_color;
    visuals.window_fill = window_color;
    visuals.extreme_bg_color = surface_color;
    visuals.override_text_color = Some(text_color);
    visuals.selection.bg_fill = accent_color;
    visuals.widgets.noninteractive.bg_fill = panel_color;
    visuals.widgets.noninteractive.fg_stroke.color = text_color;
    visuals.widgets.inactive.bg_fill = surface_color;
    visuals.widgets.hovered.bg_fill = surface_color;
    visuals.widgets.active.bg_fill = accent_color;
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
    }
}
