//! Theme domain models: ThemeId and Palette data.
//!
//! Invariant: this module performs no I/O and contains no egui/rendering types.
//! Color palette data uses plain [u8; 3] RGB values.

use serde::{Deserialize, Serialize};

/// Built-in theme identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeId {
    /// Clean light theme (tranche 1 default).
    #[default]
    White,
    /// Pure black dark theme for OLED displays.
    BlackOled,
    /// High-contrast monochromatic theme with zero saturation.
    Mono,
}

/// Plain RGB color palette data with dark mode flag.
///
/// Contains up to 6 core slots for tranche 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    pub window: [u8; 3],
    pub panel: [u8; 3],
    pub surface: [u8; 3],
    pub text: [u8; 3],
    pub secondary: [u8; 3],
    pub accent: [u8; 3],
    pub dark: bool,
}

impl Palette {
    /// White (light) built-in palette.
    pub fn white() -> Self {
        Self {
            window: [250, 250, 250],
            panel: [240, 240, 240],
            surface: [255, 255, 255],
            text: [20, 20, 20],
            secondary: [100, 100, 100],
            accent: [220, 30, 30],
            dark: false,
        }
    }

    /// Pure black OLED dark palette.
    pub fn black_oled() -> Self {
        Self {
            window: [0, 0, 0],
            panel: [12, 12, 12],
            surface: [20, 20, 20],
            text: [240, 240, 240],
            secondary: [160, 160, 160],
            accent: [255, 60, 60],
            dark: true,
        }
    }

    /// Monochromatic palette with zero color saturation.
    pub fn mono() -> Self {
        Self {
            window: [18, 18, 18],
            panel: [28, 28, 28],
            surface: [38, 38, 38],
            text: [230, 230, 230],
            secondary: [150, 150, 150],
            accent: [255, 255, 255],
            dark: true,
        }
    }

    /// Dynamically sets a color slot by name with typo-safety.
    ///
    /// Returns true if the slot was recognized and updated; false if unknown.
    pub fn set(&mut self, slot: &str, rgb: [u8; 3]) -> bool {
        let Some(target) = self.slot_mut(slot) else {
            return false;
        };
        *target = rgb;
        true
    }

    fn slot_mut(&mut self, slot: &str) -> Option<&mut [u8; 3]> {
        match slot {
            "window" => Some(&mut self.window),
            "panel" => Some(&mut self.panel),
            "surface" => Some(&mut self.surface),
            "text" => Some(&mut self.text),
            "secondary" => Some(&mut self.secondary),
            "accent" => Some(&mut self.accent),
            _ => None,
        }
    }
}

impl ThemeId {
    /// Returns the color palette definition for this theme.
    pub fn palette(&self) -> Palette {
        match self {
            Self::White => Palette::white(),
            Self::BlackOled => Palette::black_oled(),
            Self::Mono => Palette::mono(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_2_given_each_theme_id_when_palette_read_then_differs_black_oled_is_pure_black_and_mono_zero_saturation()
     {
        let white = ThemeId::White.palette();
        let black = ThemeId::BlackOled.palette();
        let mono = ThemeId::Mono.palette();

        // 1. The three palettes must differ from each other
        assert_ne!(white, black);
        assert_ne!(white, mono);
        assert_ne!(black, mono);

        // 2. BlackOled window must be pure black [0, 0, 0]
        assert_eq!(black.window, [0, 0, 0]);
        assert!(black.dark);

        // 3. Mono has zero saturation (r == g == b on every color slot)
        let mono_colors =
            [mono.window, mono.panel, mono.surface, mono.text, mono.secondary, mono.accent];
        for color in mono_colors {
            assert_eq!(color[0], color[1], "expected r == g in mono: {color:?}");
            assert_eq!(color[1], color[2], "expected g == b in mono: {color:?}");
        }
    }

    #[test]
    fn test_3_given_unknown_color_name_when_set_called_then_returns_false_and_changes_nothing() {
        let mut palette = ThemeId::White.palette();
        let original = palette.clone();

        // 1. Unknown color names return false and leave palette untouched
        assert!(!palette.set("unknown_slot", [12, 34, 56]));
        assert!(!palette.set("wndow", [12, 34, 56])); // typo safety
        assert_eq!(palette, original);

        // 2. Valid slot returns true and updates value
        assert!(palette.set("window", [1, 2, 3]));
        assert_eq!(palette.window, [1, 2, 3]);
    }
}
