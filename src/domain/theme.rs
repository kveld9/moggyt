//! Theme domain models: ThemeId, Palette, and DesignTokens.
//!
//! Invariant: this module performs no I/O and contains no egui/rendering types.
//! Color palette data uses plain [u8; 3] RGB values.
//! Derived color slots follow uniform OKLCH lightness steps converted to sRGB u8
//! without introducing new hue families or gradients.

use serde::{Deserialize, Serialize};

/// Presentation mode of the playback bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlayerBarMode {
    /// Docked, full-width bar flush with the window bottom edge.
    #[default]
    #[serde(rename = "classic")]
    Classic,
    /// Rounded bar floating above the bottom edge, centered and width-capped.
    #[serde(rename = "floating")]
    Floating,
}

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
/// Contains core slots and derived-ready slots for borders, hovers, and overlays.
/// Derivation choice: uniform lightness steps in perceptual contrast converted to sRGB u8.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    pub window: [u8; 3],
    pub panel: [u8; 3],
    pub surface: [u8; 3],
    pub text: [u8; 3],
    pub secondary: [u8; 3],
    pub accent: [u8; 3],
    pub border: [u8; 3],
    pub hover: [u8; 3],
    pub overlay: [u8; 3],
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
            border: [215, 215, 215],
            hover: [232, 232, 232],
            overlay: [248, 248, 248],
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
            border: [36, 36, 36],
            hover: [28, 28, 28],
            overlay: [24, 24, 24],
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
            border: [56, 56, 56],
            hover: [46, 46, 46],
            overlay: [42, 42, 42],
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
            "border" => Some(&mut self.border),
            "hover" => Some(&mut self.hover),
            "overlay" => Some(&mut self.overlay),
            _ => None,
        }
    }
}

/// Design tokens defining spatial scale, corner radii, typography sizes, and focus strokes.
///
/// Multiples of 4 for spacing scale and corner radii; perceptual type scale with uniform discipline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DesignTokens {
    pub space_xxs: f32,
    pub space_xs: f32,
    pub space_sm: f32,
    pub space_md: f32,
    pub space_lg: f32,
    pub space_xl: f32,

    pub radius_none: f32,
    pub radius_sm: f32,
    pub radius_md: f32,
    pub radius_lg: f32,
    pub radius_xl: f32,

    pub font_caption: f32,
    pub font_body: f32,
    pub font_title_sm: f32,
    pub font_title_lg: f32,
    pub font_display: f32,

    pub focus_ring_width: f32,
}

impl DesignTokens {
    pub const SPACE_XXS: f32 = 4.0;
    pub const SPACE_XS: f32 = 8.0;
    pub const SPACE_SM: f32 = 12.0;
    pub const SPACE_MD: f32 = 16.0;
    pub const SPACE_LG: f32 = 24.0;
    pub const SPACE_XL: f32 = 32.0;

    pub const RADIUS_NONE: f32 = 0.0;
    pub const RADIUS_SM: f32 = 4.0;
    pub const RADIUS_MD: f32 = 8.0;
    pub const RADIUS_LG: f32 = 12.0;
    pub const RADIUS_XL: f32 = 16.0;

    pub const FONT_CAPTION: f32 = 12.0;
    pub const FONT_BODY: f32 = 14.0;
    pub const FONT_TITLE_SM: f32 = 16.0;
    pub const FONT_TITLE_LG: f32 = 20.0;
    pub const FONT_DISPLAY: f32 = 28.0;

    pub const FOCUS_RING_WIDTH: f32 = 2.0;

    /// Standard desktop design tokens instance.
    pub const fn desktop() -> Self {
        Self {
            space_xxs: Self::SPACE_XXS,
            space_xs: Self::SPACE_XS,
            space_sm: Self::SPACE_SM,
            space_md: Self::SPACE_MD,
            space_lg: Self::SPACE_LG,
            space_xl: Self::SPACE_XL,
            radius_none: Self::RADIUS_NONE,
            radius_sm: Self::RADIUS_SM,
            radius_md: Self::RADIUS_MD,
            radius_lg: Self::RADIUS_LG,
            radius_xl: Self::RADIUS_XL,
            font_caption: Self::FONT_CAPTION,
            font_body: Self::FONT_BODY,
            font_title_sm: Self::FONT_TITLE_SM,
            font_title_lg: Self::FONT_TITLE_LG,
            font_display: Self::FONT_DISPLAY,
            focus_ring_width: Self::FOCUS_RING_WIDTH,
        }
    }
}

impl Default for DesignTokens {
    fn default() -> Self {
        Self::desktop()
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

    /// Returns the design tokens definition for this theme.
    pub fn tokens(&self) -> DesignTokens {
        DesignTokens::desktop()
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
        let mono_colors = [
            mono.window,
            mono.panel,
            mono.surface,
            mono.text,
            mono.secondary,
            mono.accent,
            mono.border,
            mono.hover,
            mono.overlay,
        ];
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

        // 2. Valid core slot returns true and updates value
        assert!(palette.set("window", [1, 2, 3]));
        assert_eq!(palette.window, [1, 2, 3]);

        // 3. Valid derived slot returns true and updates value
        assert!(palette.set("border", [10, 20, 30]));
        assert_eq!(palette.border, [10, 20, 30]);
        assert!(palette.set("hover", [40, 50, 60]));
        assert_eq!(palette.hover, [40, 50, 60]);
        assert!(palette.set("overlay", [70, 80, 90]));
        assert_eq!(palette.overlay, [70, 80, 90]);
    }

    #[test]
    fn test_given_each_theme_when_derived_slots_inspected_then_non_equal_to_existing_core_slots() {
        for theme in [ThemeId::White, ThemeId::BlackOled, ThemeId::Mono] {
            let palette = theme.palette();

            // Derived slots must be distinct from base structural slots
            assert_ne!(palette.border, palette.window);
            assert_ne!(palette.border, palette.panel);
            assert_ne!(palette.border, palette.surface);

            assert_ne!(palette.hover, palette.window);
            assert_ne!(palette.hover, palette.panel);
            assert_ne!(palette.hover, palette.surface);

            assert_ne!(palette.overlay, palette.window);
            assert_ne!(palette.overlay, palette.panel);
            assert_ne!(palette.overlay, palette.surface);

            // Derived slots must be mutually distinct
            assert_ne!(palette.border, palette.hover);
            assert_ne!(palette.border, palette.overlay);
            assert_ne!(palette.hover, palette.overlay);
        }
    }

    #[test]
    fn test_given_design_tokens_desktop_when_read_then_values_match_multiples_of_four_and_type_scale()
     {
        let tokens = DesignTokens::desktop();
        assert_eq!(tokens, DesignTokens::default());
        assert_eq!(tokens, ThemeId::White.tokens());

        // Spacing multiples of 4
        assert_eq!(tokens.space_xxs, 4.0);
        assert_eq!(tokens.space_xs, 8.0);
        assert_eq!(tokens.space_sm, 12.0);
        assert_eq!(tokens.space_md, 16.0);
        assert_eq!(tokens.space_lg, 24.0);
        assert_eq!(tokens.space_xl, 32.0);

        // Corner radii
        assert_eq!(tokens.radius_none, 0.0);
        assert_eq!(tokens.radius_sm, 4.0);
        assert_eq!(tokens.radius_md, 8.0);
        assert_eq!(tokens.radius_lg, 12.0);
        assert_eq!(tokens.radius_xl, 16.0);

        // Typography scale
        assert_eq!(tokens.font_caption, 12.0);
        assert_eq!(tokens.font_body, 14.0);
        assert_eq!(tokens.font_title_sm, 16.0);
        assert_eq!(tokens.font_title_lg, 20.0);
        assert_eq!(tokens.font_display, 28.0);

        // Focus ring width
        assert_eq!(tokens.focus_ring_width, 2.0);
    }

    #[test]
    fn test_given_palette_and_design_tokens_when_serde_round_trip_then_preserves_equality() {
        let palette = ThemeId::White.palette();
        let serialized_palette = serde_json::to_string(&palette).expect("palette serialization");
        let deserialized_palette: Palette =
            serde_json::from_str(&serialized_palette).expect("palette deserialization");
        assert_eq!(palette, deserialized_palette);

        let tokens = DesignTokens::desktop();
        let serialized_tokens = serde_json::to_string(&tokens).expect("tokens serialization");
        let deserialized_tokens: DesignTokens =
            serde_json::from_str(&serialized_tokens).expect("tokens deserialization");
        assert_eq!(tokens, deserialized_tokens);
    }

    #[test]
    fn test_player_bar_mode_serde_and_default() {
        assert_eq!(PlayerBarMode::default(), PlayerBarMode::Classic);

        let classic_json = serde_json::to_string(&PlayerBarMode::Classic).unwrap();
        assert_eq!(classic_json, "\"classic\"");
        let classic_deser: PlayerBarMode = serde_json::from_str("\"classic\"").unwrap();
        assert_eq!(classic_deser, PlayerBarMode::Classic);

        let floating_json = serde_json::to_string(&PlayerBarMode::Floating).unwrap();
        assert_eq!(floating_json, "\"floating\"");
        let floating_deser: PlayerBarMode = serde_json::from_str("\"floating\"").unwrap();
        assert_eq!(floating_deser, PlayerBarMode::Floating);
    }
}
