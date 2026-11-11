//! Theme domain models: ThemeId and Palette.
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
    /// Material You palette produced by matugen and read from `custom-theme.json`.
    Matugen,
    /// User palette loaded from the custom theme file.
    Custom,
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
            accent: [255, 255, 255],
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

    /// Returns this palette with its accent slot replaced.
    pub fn with_accent(mut self, accent: [u8; 3]) -> Self {
        self.accent = accent;
        self
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

    /// Constructs a [`Palette`] from custom key-value entries.
    ///
    /// Requires a `"mode"` key (`"dark"` or `"light"`). Unspecified slots inherit
    /// from [`Palette::black_oled()`] for dark mode or [`Palette::white()`] for light mode.
    pub fn from_custom_entries<'a>(
        entries: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Palette, CustomPaletteError> {
        let items: Vec<(&'a str, &'a str)> = entries.into_iter().collect();
        let mode = items.iter().find(|(k, _)| *k == "mode").map(|(_, v)| *v);
        let mut palette = resolve_mode(mode)?;

        for &(slot, value) in &items {
            if slot == "mode" {
                continue;
            }
            apply_entry(&mut palette, slot, value)?;
        }

        Ok(palette)
    }
}

/// Error conditions encountered when building a palette from user-defined custom theme entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomPaletteError {
    /// Missing required "mode" field ("dark" or "light").
    MissingMode,
    /// Invalid mode value (expected "dark" or "light").
    InvalidMode(String),
    /// Unknown palette color slot name.
    UnknownSlot(String),
    /// Invalid RGB hex color format for a slot.
    InvalidColor { slot: String, value: String },
}

impl std::fmt::Display for CustomPaletteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingMode => write!(f, "missing required 'mode' field"),
            Self::InvalidMode(mode) => {
                write!(f, "invalid mode '{mode}', expected 'dark' or 'light'")
            }
            Self::UnknownSlot(slot) => write!(f, "unknown color slot '{slot}'"),
            Self::InvalidColor { slot, value } => {
                write!(f, "invalid color value '{value}' for slot '{slot}'")
            }
        }
    }
}

impl std::error::Error for CustomPaletteError {}

/// Parses a hex color string in `#rrggbb` format into a plain `[u8; 3]` RGB array.
///
/// Accepts case-insensitive hex digits. Returns `None` if the input is not exactly
/// a `#` followed by 6 hex characters.
pub fn parse_hex_rgb(value: &str) -> Option<[u8; 3]> {
    if value.len() != 7 || !value.starts_with('#') {
        return None;
    }
    if !value[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&value[1..3], 16).ok()?;
    let g = u8::from_str_radix(&value[3..5], 16).ok()?;
    let b = u8::from_str_radix(&value[5..7], 16).ok()?;
    Some([r, g, b])
}

/// Formats a plain RGB array as a lowercase `#rrggbb` hex string.
pub fn format_hex_rgb(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

fn resolve_mode(mode: Option<&str>) -> Result<Palette, CustomPaletteError> {
    match mode {
        Some("dark") => Ok(Palette::black_oled()),
        Some("light") => Ok(Palette::white()),
        Some(other) => Err(CustomPaletteError::InvalidMode(other.to_string())),
        None => Err(CustomPaletteError::MissingMode),
    }
}

fn apply_entry(palette: &mut Palette, slot: &str, value: &str) -> Result<(), CustomPaletteError> {
    let Some(rgb) = parse_hex_rgb(value) else {
        return Err(CustomPaletteError::InvalidColor {
            slot: slot.to_string(),
            value: value.to_string(),
        });
    };
    if !palette.set(slot, rgb) {
        return Err(CustomPaletteError::UnknownSlot(slot.to_string()));
    }
    Ok(())
}

impl ThemeId {
    /// Returns the color palette definition for this theme.
    pub fn palette(&self) -> Palette {
        match self {
            Self::White => Palette::white(),
            Self::BlackOled => Palette::black_oled(),
            Self::Mono => Palette::mono(),
            Self::Matugen => Palette::black_oled(),
            // Fallback until a custom file is loaded.
            Self::Custom => Palette::black_oled(),
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
        // BlackOled accent is pure white (no hue on the OLED theme)
        assert_eq!(black.accent, [255, 255, 255]);

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

    #[test]
    fn test_parse_hex_rgb_valid_and_invalid_formats() {
        assert_eq!(parse_hex_rgb("#121318"), Some([0x12, 0x13, 0x18]));
        assert_eq!(parse_hex_rgb("#AABBCC"), Some([0xaa, 0xbb, 0xcc]));
        assert_eq!(parse_hex_rgb("#ffffff"), Some([255, 255, 255]));
        assert_eq!(parse_hex_rgb("#000000"), Some([0, 0, 0]));

        // Invalid: missing '#'
        assert_eq!(parse_hex_rgb("123456"), None);
        // Invalid: 5 digits
        assert_eq!(parse_hex_rgb("#12345"), None);
        // Invalid: non-hex characters
        assert_eq!(parse_hex_rgb("#12345g"), None);
        assert_eq!(parse_hex_rgb("#+12345"), None);
        assert_eq!(parse_hex_rgb("# 12345"), None);
        // Invalid: 7 digits
        assert_eq!(parse_hex_rgb("#1234567"), None);
        // Invalid: empty
        assert_eq!(parse_hex_rgb(""), None);
    }

    #[test]
    fn test_from_custom_entries_full_dark_palette() {
        let entries = [
            ("mode", "dark"),
            ("window", "#121318"),
            ("panel", "#1e1f25"),
            ("surface", "#282a2f"),
            ("text", "#e2e2e9"),
            ("secondary", "#c5c6d0"),
            ("accent", "#b1c5ff"),
            ("border", "#44464f"),
            ("hover", "#33343a"),
            ("overlay", "#38393f"),
        ];

        let palette = Palette::from_custom_entries(entries).expect("parse valid entries");
        assert!(palette.dark);
        assert_eq!(palette.window, [0x12, 0x13, 0x18]);
        assert_eq!(palette.panel, [0x1e, 0x1f, 0x25]);
        assert_eq!(palette.surface, [0x28, 0x2a, 0x2f]);
        assert_eq!(palette.text, [0xe2, 0xe2, 0xe9]);
        assert_eq!(palette.secondary, [0xc5, 0xc6, 0xd0]);
        assert_eq!(palette.accent, [0xb1, 0xc5, 0xff]);
        assert_eq!(palette.border, [0x44, 0x46, 0x4f]);
        assert_eq!(palette.hover, [0x33, 0x34, 0x3a]);
        assert_eq!(palette.overlay, [0x38, 0x39, 0x3f]);
    }

    #[test]
    fn test_from_custom_entries_partial_light_inherits_white_base() {
        let entries = [("mode", "light"), ("accent", "#001122")];

        let palette = Palette::from_custom_entries(entries).expect("parse partial light entries");
        assert!(!palette.dark);
        assert_eq!(palette.accent, [0x00, 0x11, 0x22]);
        // Unspecified slots match Palette::white()
        let default_white = Palette::white();
        assert_eq!(palette.window, default_white.window);
        assert_eq!(palette.panel, default_white.panel);
        assert_eq!(palette.surface, default_white.surface);
        assert_eq!(palette.text, default_white.text);
        assert_eq!(palette.secondary, default_white.secondary);
        assert_eq!(palette.border, default_white.border);
        assert_eq!(palette.hover, default_white.hover);
        assert_eq!(palette.overlay, default_white.overlay);
    }

    #[test]
    fn test_from_custom_entries_error_conditions() {
        // Missing mode
        let err = Palette::from_custom_entries([("window", "#112233")]).unwrap_err();
        assert_eq!(err, CustomPaletteError::MissingMode);

        // Invalid mode
        let err = Palette::from_custom_entries([("mode", "sepia")]).unwrap_err();
        assert_eq!(err, CustomPaletteError::InvalidMode("sepia".to_string()));

        // Unknown slot
        let err =
            Palette::from_custom_entries([("mode", "dark"), ("sidebar", "#112233")]).unwrap_err();
        assert_eq!(err, CustomPaletteError::UnknownSlot("sidebar".to_string()));

        // Invalid color
        let err = Palette::from_custom_entries([("mode", "dark"), ("accent", "red")]).unwrap_err();
        assert_eq!(
            err,
            CustomPaletteError::InvalidColor {
                slot: "accent".to_string(),
                value: "red".to_string(),
            }
        );
    }

    #[test]
    fn test_theme_id_custom_serde_and_fallback_palette() {
        assert_eq!(ThemeId::Custom.palette(), Palette::black_oled());

        let json = serde_json::to_string(&ThemeId::Custom).expect("serialize custom");
        assert_eq!(json, "\"custom\"");
        let deserialized: ThemeId = serde_json::from_str("\"custom\"").expect("deserialize custom");
        assert_eq!(deserialized, ThemeId::Custom);
    }

    #[test]
    fn test_theme_id_matugen_serde_and_fallback_palette() {
        assert_eq!(ThemeId::Matugen.palette(), Palette::black_oled());

        let json = serde_json::to_string(&ThemeId::Matugen).expect("serialize matugen");
        assert_eq!(json, "\"matugen\"");
        let deserialized: ThemeId =
            serde_json::from_str("\"matugen\"").expect("deserialize matugen");
        assert_eq!(deserialized, ThemeId::Matugen);
    }

    #[test]
    fn test_format_hex_rgb_and_roundtrip() {
        let hex = format_hex_rgb([0x12, 0x34, 0x56]);
        assert_eq!(hex, "#123456");
        assert_eq!(hex, hex.to_lowercase());
        assert_eq!(parse_hex_rgb(&hex), Some([0x12, 0x34, 0x56]));
    }

    #[test]
    fn test_palette_with_accent_preserves_other_slots() {
        let original = Palette::white();
        let modified = original.clone().with_accent([1, 2, 3]);

        assert_eq!(modified.accent, [1, 2, 3]);
        assert_eq!(modified.window, original.window);
        assert_eq!(modified.panel, original.panel);
        assert_eq!(modified.surface, original.surface);
        assert_eq!(modified.text, original.text);
        assert_eq!(modified.secondary, original.secondary);
        assert_eq!(modified.border, original.border);
        assert_eq!(modified.hover, original.hover);
        assert_eq!(modified.overlay, original.overlay);
        assert_eq!(modified.dark, original.dark);
    }
}
