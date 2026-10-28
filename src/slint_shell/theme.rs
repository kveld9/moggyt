//! Slint UI shell theme bridge: color conversions and palette bindings.
//!
//! Hexagonal architecture invariant: this module performs zero I/O and
//! converts domain color models ([`ThemeId`], [`Palette`]) into Slint-compatible
//! color primitives.

use crate::domain::{Palette, ThemeId};
use crate::slint_shell::MoggytWindow;

/// Converts a domain [u8; 3] sRGB array into a Slint [`slint::Color`].
pub fn rgb_to_slint(c: [u8; 3]) -> slint::Color {
    slint::Color::from_rgb_u8(c[0], c[1], c[2])
}

/// Resolves the domain [`Palette`] corresponding to the given [`ThemeId`].
pub fn palette_for(id: ThemeId) -> Palette {
    match id {
        ThemeId::White => Palette::white(),
        ThemeId::BlackOled => Palette::black_oled(),
        ThemeId::Mono => Palette::mono(),
    }
}

/// Available themes with their domain identifier and human-readable display name.
pub const ALL_THEMES: [(ThemeId, &str); 3] =
    [(ThemeId::White, "White"), (ThemeId::BlackOled, "Black OLED"), (ThemeId::Mono, "Monochrome")];

/// Maps a 0-based integer index to its corresponding [`ThemeId`].
///
/// Returns [`ThemeId::White`] for 0 or unknown values,
/// [`ThemeId::BlackOled`] for 1, and [`ThemeId::Mono`] for 2.
pub fn theme_from_index(index: i32) -> ThemeId {
    match index {
        1 => ThemeId::BlackOled,
        2 => ThemeId::Mono,
        _ => ThemeId::White,
    }
}

/// Maps a [`ThemeId`] to its canonical 0-based integer index.
pub fn theme_to_index(id: ThemeId) -> i32 {
    match id {
        ThemeId::White => 0,
        ThemeId::BlackOled => 1,
        ThemeId::Mono => 2,
    }
}

/// Returns the human-readable display name for the given [`ThemeId`].
pub fn theme_display_name(id: ThemeId) -> &'static str {
    match id {
        ThemeId::White => "White",
        ThemeId::BlackOled => "Black OLED",
        ThemeId::Mono => "Monochrome",
    }
}

/// Slint-native color representations for all nine Palette slots plus dark mode flag.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeColors {
    pub window: slint::Color,
    pub panel: slint::Color,
    pub surface: slint::Color,
    pub text: slint::Color,
    pub secondary: slint::Color,
    pub accent: slint::Color,
    pub border: slint::Color,
    pub hover: slint::Color,
    pub overlay: slint::Color,
    pub dark: bool,
}

impl From<&Palette> for ThemeColors {
    fn from(palette: &Palette) -> Self {
        Self {
            window: rgb_to_slint(palette.window),
            panel: rgb_to_slint(palette.panel),
            surface: rgb_to_slint(palette.surface),
            text: rgb_to_slint(palette.text),
            secondary: rgb_to_slint(palette.secondary),
            accent: rgb_to_slint(palette.accent),
            border: rgb_to_slint(palette.border),
            hover: rgb_to_slint(palette.hover),
            overlay: rgb_to_slint(palette.overlay),
            dark: palette.dark,
        }
    }
}

impl ThemeColors {
    /// Constructs [`ThemeColors`] for the given [`ThemeId`].
    pub fn for_theme(id: ThemeId) -> Self {
        let palette = palette_for(id);
        Self::from(&palette)
    }

    /// Pushes all theme color slot properties to the given Slint window.
    pub fn apply_to_window(&self, window: &MoggytWindow) {
        window.set_theme_window(self.window);
        window.set_theme_panel(self.panel);
        window.set_theme_surface(self.surface);
        window.set_theme_text(self.text);
        window.set_theme_secondary(self.secondary);
        window.set_theme_accent(self.accent);
        window.set_theme_border(self.border);
        window.set_theme_hover(self.hover);
        window.set_theme_overlay(self.overlay);
        window.set_theme_dark(self.dark);
    }
}

/// Convenience helper to resolve theme palette and push all properties to window.
pub fn apply_theme(window: &MoggytWindow, theme_id: ThemeId) {
    let colors = ThemeColors::for_theme(theme_id);
    colors.apply_to_window(window);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_theme_is_white() {
        assert_eq!(ThemeId::default(), ThemeId::White);
    }

    #[test]
    fn test_palette_for_white_theme() {
        let palette = palette_for(ThemeId::White);
        assert_eq!(palette.window, [250, 250, 250]);
        assert_eq!(palette.panel, [240, 240, 240]);
        assert_eq!(palette.surface, [255, 255, 255]);
        assert_eq!(palette.text, [20, 20, 20]);
        assert_eq!(palette.secondary, [100, 100, 100]);
        assert_eq!(palette.accent, [220, 30, 30]);
        assert_eq!(palette.border, [215, 215, 215]);
        assert_eq!(palette.hover, [232, 232, 232]);
        assert_eq!(palette.overlay, [248, 248, 248]);
        assert!(!palette.dark);
    }

    #[test]
    fn test_palette_for_black_oled_theme() {
        let palette = palette_for(ThemeId::BlackOled);
        assert_eq!(palette.window, [0, 0, 0]);
        assert_eq!(palette.panel, [12, 12, 12]);
        assert_eq!(palette.surface, [20, 20, 20]);
        assert_eq!(palette.text, [240, 240, 240]);
        assert_eq!(palette.secondary, [160, 160, 160]);
        assert_eq!(palette.accent, [255, 60, 60]);
        assert_eq!(palette.border, [36, 36, 36]);
        assert_eq!(palette.hover, [28, 28, 28]);
        assert_eq!(palette.overlay, [24, 24, 24]);
        assert!(palette.dark);
    }

    #[test]
    fn test_palette_for_mono_theme() {
        let palette = palette_for(ThemeId::Mono);
        assert_eq!(palette.window, [18, 18, 18]);
        assert_eq!(palette.panel, [28, 28, 28]);
        assert_eq!(palette.surface, [38, 38, 38]);
        assert_eq!(palette.text, [230, 230, 230]);
        assert_eq!(palette.secondary, [150, 150, 150]);
        assert_eq!(palette.accent, [255, 255, 255]);
        assert_eq!(palette.border, [56, 56, 56]);
        assert_eq!(palette.hover, [46, 46, 46]);
        assert_eq!(palette.overlay, [42, 42, 42]);
        assert!(palette.dark);
    }

    #[test]
    fn test_rgb_to_slint_round_trip() {
        let black = rgb_to_slint([0, 0, 0]);
        assert_eq!([black.red(), black.green(), black.blue()], [0, 0, 0]);

        let white = rgb_to_slint([255, 255, 255]);
        assert_eq!([white.red(), white.green(), white.blue()], [255, 255, 255]);

        let red = rgb_to_slint([220, 30, 30]);
        assert_eq!([red.red(), red.green(), red.blue()], [220, 30, 30]);
    }

    #[test]
    fn test_theme_colors_from_palette_conversion() {
        let palette = Palette::white();
        let colors = ThemeColors::from(&palette);
        assert_eq!(
            [colors.window.red(), colors.window.green(), colors.window.blue()],
            [250, 250, 250]
        );
        assert_eq!(
            [colors.accent.red(), colors.accent.green(), colors.accent.blue()],
            [220, 30, 30]
        );
        assert!(!colors.dark);

        let oled_colors = ThemeColors::for_theme(ThemeId::BlackOled);
        assert_eq!(
            [oled_colors.window.red(), oled_colors.window.green(), oled_colors.window.blue()],
            [0, 0, 0]
        );
        assert_eq!(
            [oled_colors.accent.red(), oled_colors.accent.green(), oled_colors.accent.blue()],
            [255, 60, 60]
        );
        assert!(oled_colors.dark);
    }

    #[test]
    fn test_theme_from_index_mappings() {
        assert_eq!(theme_from_index(0), ThemeId::White);
        assert_eq!(theme_from_index(1), ThemeId::BlackOled);
        assert_eq!(theme_from_index(2), ThemeId::Mono);
        assert_eq!(theme_from_index(-1), ThemeId::White);
        assert_eq!(theme_from_index(99), ThemeId::White);
    }

    #[test]
    fn test_theme_to_index_mappings() {
        assert_eq!(theme_to_index(ThemeId::White), 0);
        assert_eq!(theme_to_index(ThemeId::BlackOled), 1);
        assert_eq!(theme_to_index(ThemeId::Mono), 2);
    }

    #[test]
    fn test_theme_display_names() {
        assert_eq!(theme_display_name(ThemeId::White), "White");
        assert_eq!(theme_display_name(ThemeId::BlackOled), "Black OLED");
        assert_eq!(theme_display_name(ThemeId::Mono), "Monochrome");
    }

    #[test]
    fn test_theme_roundtrip_index() {
        for (theme, name) in ALL_THEMES {
            let idx = theme_to_index(theme);
            assert_eq!(theme_from_index(idx), theme);
            assert_eq!(theme_display_name(theme), name);
        }
    }
}
