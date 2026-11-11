//! Slint UI shell theme bridge: color conversions and palette bindings.
//!
//! Hexagonal architecture invariant: this module performs zero I/O and
//! converts domain color models ([`ThemeId`], [`Palette`]) into Slint-compatible
//! color primitives.

use crate::domain::{Palette, ThemeId};
use crate::i18n::tr;
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::state::SlintAppState;

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
        ThemeId::Custom => Palette::black_oled(),
    }
}

/// Available themes with their domain identifier and human-readable display name.
pub const ALL_THEMES: [(ThemeId, &str); 4] = [
    (ThemeId::White, "White"),
    (ThemeId::BlackOled, "Black OLED"),
    (ThemeId::Mono, "Monochrome"),
    (ThemeId::Custom, "Custom"),
];

/// Maps a 0-based integer index to its corresponding [`ThemeId`].
///
/// Returns [`ThemeId::White`] for 0 or unknown values,
/// [`ThemeId::BlackOled`] for 1, [`ThemeId::Mono`] for 2, and [`ThemeId::Custom`] for 3.
pub fn theme_from_index(index: i32) -> ThemeId {
    match index {
        1 => ThemeId::BlackOled,
        2 => ThemeId::Mono,
        3 => ThemeId::Custom,
        _ => ThemeId::White,
    }
}

/// Maps a [`ThemeId`] to its canonical 0-based integer index.
pub fn theme_to_index(id: ThemeId) -> i32 {
    match id {
        ThemeId::White => 0,
        ThemeId::BlackOled => 1,
        ThemeId::Mono => 2,
        ThemeId::Custom => 3,
    }
}

/// Returns the human-readable display name for the given [`ThemeId`].
pub fn theme_display_name(id: ThemeId) -> &'static str {
    match id {
        ThemeId::White => "White",
        ThemeId::BlackOled => "Black OLED",
        ThemeId::Mono => "Monochrome",
        ThemeId::Custom => "Custom",
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

/// Convenience helper to push an arbitrary palette's color slots to window.
pub fn apply_palette(window: &MoggytWindow, palette: &Palette) {
    let colors = ThemeColors::from(palette);
    colors.apply_to_window(window);
}

impl SlintAppState {
    /// Returns the active color palette, resolving custom overrides if Custom theme is selected.
    pub fn active_palette(&self) -> Palette {
        if self.theme == ThemeId::Custom {
            return self.custom_palette.clone().unwrap_or_else(|| palette_for(ThemeId::Custom));
        }
        let palette = palette_for(self.theme);
        match self.accent_override {
            Some(accent) => palette.with_accent(accent),
            None => palette,
        }
    }

    /// Reloads custom theme from disk if configured, updating state status message.
    // NOTE(clippy): cognitive_complexity counts tracing expansion (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    pub fn reload_custom_theme(&mut self) {
        let Some(ref mut custom) = self.custom_theme else {
            return;
        };
        if custom.active_theme_name().is_none() && !custom.path().exists() {
            return;
        }
        let path_display = custom.path().display().to_string();
        match custom.load() {
            Ok(palette) => {
                self.custom_palette = Some(palette);
                self.custom_theme_status = tr(self.locale, "Custom theme loaded from {path}")
                    .replace("{path}", &path_display);
            }
            Err(err) => {
                tracing::warn!("failed to load custom theme: {err:#}");
                self.custom_theme_status = tr(self.locale, "Custom theme not loaded: {error}")
                    .replace("{error}", &format!("{err:#}"));
            }
        }
    }

    /// Polls custom theme file on disk and reloads palette if modified and Custom theme is active.
    pub fn poll_custom_theme(&mut self) -> bool {
        if self.theme != ThemeId::Custom {
            return false;
        }
        let has_changed = self.custom_theme.as_ref().is_some_and(|c| c.has_changed());
        if !has_changed {
            return false;
        }
        self.reload_custom_theme();
        true
    }
}

/// Checks for custom theme file modifications and applies colors to window if updated.
pub(crate) fn apply_custom_theme_if_changed(window: &MoggytWindow, state: &mut SlintAppState) {
    if state.poll_custom_theme() {
        apply_palette(window, &state.active_palette());
        window.set_custom_theme_status_text(state.custom_theme_status.clone().into());
    }
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
        assert_eq!(palette.accent, [255, 255, 255]);
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
            [255, 255, 255]
        );
        assert!(oled_colors.dark);
    }

    #[test]
    fn test_theme_from_index_mappings() {
        assert_eq!(theme_from_index(0), ThemeId::White);
        assert_eq!(theme_from_index(1), ThemeId::BlackOled);
        assert_eq!(theme_from_index(2), ThemeId::Mono);
        assert_eq!(theme_from_index(3), ThemeId::Custom);
        assert_eq!(theme_from_index(-1), ThemeId::White);
        assert_eq!(theme_from_index(99), ThemeId::White);
    }

    #[test]
    fn test_theme_to_index_mappings() {
        assert_eq!(theme_to_index(ThemeId::White), 0);
        assert_eq!(theme_to_index(ThemeId::BlackOled), 1);
        assert_eq!(theme_to_index(ThemeId::Mono), 2);
        assert_eq!(theme_to_index(ThemeId::Custom), 3);
    }

    #[test]
    fn test_theme_display_names() {
        assert_eq!(theme_display_name(ThemeId::White), "White");
        assert_eq!(theme_display_name(ThemeId::BlackOled), "Black OLED");
        assert_eq!(theme_display_name(ThemeId::Mono), "Monochrome");
        assert_eq!(theme_display_name(ThemeId::Custom), "Custom");
    }

    #[test]
    fn test_theme_roundtrip_index() {
        for (theme, name) in ALL_THEMES {
            let idx = theme_to_index(theme);
            assert_eq!(theme_from_index(idx), theme);
            assert_eq!(theme_display_name(theme), name);
        }
    }

    #[test]
    fn test_custom_theme_palette_for_returns_black_oled() {
        assert_eq!(palette_for(ThemeId::Custom), Palette::black_oled());
    }

    #[test]
    fn test_active_palette_resolves_builtin_and_custom() {
        let mut state = crate::slint_shell::test_support::test_wired();
        state.theme = ThemeId::White;
        assert_eq!(state.active_palette(), Palette::white());

        state.theme = ThemeId::Custom;
        state.custom_palette = None;
        assert_eq!(state.active_palette(), Palette::black_oled());

        let custom_pal = Palette::mono();
        state.custom_palette = Some(custom_pal.clone());
        assert_eq!(state.active_palette(), custom_pal);
    }

    #[test]
    fn test_active_palette_accent_override_for_builtin_and_ignored_for_custom() {
        let mut state = crate::slint_shell::test_support::test_wired();
        state.theme = ThemeId::White;
        state.accent_override = Some([0x12, 0x34, 0x56]);

        let palette = state.active_palette();
        assert_eq!(palette.accent, [0x12, 0x34, 0x56]);
        assert_eq!(palette.window, Palette::white().window);

        state.accent_override = None;
        assert_eq!(state.active_palette().accent, Palette::white().accent);

        state.theme = ThemeId::Custom;
        state.custom_palette = Some(Palette::mono());
        state.accent_override = Some([0x12, 0x34, 0x56]);
        assert_eq!(state.active_palette().accent, Palette::mono().accent);
    }

    #[test]
    fn test_poll_custom_theme_false_when_not_custom() {
        let mut state = crate::slint_shell::test_support::test_wired();
        state.theme = ThemeId::White;
        assert!(!state.poll_custom_theme());
    }

    #[test]
    fn test_reload_custom_theme_keeps_previous_palette_on_error() {
        let mut state = crate::slint_shell::test_support::test_wired();
        let temp_dir =
            std::env::temp_dir().join(format!("moggyt-test-shell-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let custom = crate::infra::CustomThemeFile::new(&temp_dir);
        let path = custom.path().to_path_buf();
        std::fs::write(&path, r##"{"mode": "light", "window": "#112233"}"##).unwrap();

        state.custom_theme = Some(custom);
        state.theme = ThemeId::Custom;
        state.reload_custom_theme();

        assert!(state.custom_palette.is_some());
        let previous = state.custom_palette.clone().unwrap();
        assert_eq!(previous.window, [0x11, 0x22, 0x33]);
        assert!(state.custom_theme_status.contains("Custom theme loaded from"));

        // Now break the file by writing invalid JSON
        std::fs::write(&path, "invalid json").unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        let future_time = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        file.set_modified(future_time).unwrap();
        drop(file);

        assert!(state.poll_custom_theme());
        // Custom palette should be KEPT (not wiped out)
        assert_eq!(state.custom_palette, Some(previous));
        assert!(state.custom_theme_status.contains("Custom theme not loaded:"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_custom_theme_file_integration_available_themes() {
        let mut state = crate::slint_shell::test_support::test_wired();
        let temp_dir = std::env::temp_dir()
            .join(format!("moggyt-test-shell-theme-avail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let themes_dir = temp_dir.join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();

        std::fs::write(themes_dir.join("rose-pine.json"), r##"{"mode": "dark"}"##).unwrap();
        std::fs::write(temp_dir.join("custom-theme.json"), r##"{"mode": "light"}"##).unwrap();

        let custom = crate::infra::CustomThemeFile::new(&temp_dir);
        state.custom_theme = Some(custom);

        let available = state.custom_theme.as_ref().unwrap().available_themes();
        assert_eq!(available, vec!["custom-theme".to_string(), "rose-pine".to_string()]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_set_custom_theme_name_reloads_palette_and_persists() {
        let mut state = crate::slint_shell::test_support::test_wired();
        let temp_dir = std::env::temp_dir()
            .join(format!("moggyt-test-shell-theme-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let themes_dir = temp_dir.join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();

        std::fs::write(themes_dir.join("nord.json"), r##"{"mode": "dark", "window": "#2e3440"}"##)
            .unwrap();

        let custom = crate::infra::CustomThemeFile::new(&temp_dir);
        state.custom_theme = Some(custom);
        state.theme = ThemeId::Custom;

        state.set_custom_theme_name(Some("nord"));
        assert_eq!(state.settings_store.load().custom_theme_name.as_deref(), Some("nord"));
        assert_eq!(state.custom_theme.as_ref().and_then(|ct| ct.active_theme_name()), Some("nord"));

        state.reload_custom_theme();
        assert_eq!(state.active_palette().window, [0x2e, 0x34, 0x40]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_reload_custom_theme_early_return_when_no_theme_and_missing_file() {
        let mut state = crate::slint_shell::test_support::test_wired();
        let temp_dir = std::env::temp_dir()
            .join(format!("moggyt-test-shell-theme-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let custom = crate::infra::CustomThemeFile::new(&temp_dir);
        state.custom_theme = Some(custom);
        state.theme = ThemeId::Custom;
        state.custom_theme_status = String::new();

        state.reload_custom_theme();

        assert!(state.custom_palette.is_none());
        assert!(state.custom_theme_status.is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
