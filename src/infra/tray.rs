//! System tray icon integration with headless fallback.

use std::cell::RefCell;

use crate::i18n::{Locale, tr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    TogglePlay,
    Next,
    Previous,
    Show,
    Quit,
}

pub const ID_PLAY_PAUSE: &str = "play_pause";
pub const ID_TOGGLE_PLAY: &str = "toggle_play";
pub const ID_NEXT: &str = "next";
pub const ID_PREVIOUS: &str = "previous";
pub const ID_SHOW: &str = "show";
pub const ID_QUIT: &str = "quit";

pub fn action_from_id(id: &str) -> Option<TrayAction> {
    match id {
        ID_PLAY_PAUSE | ID_TOGGLE_PLAY => Some(TrayAction::TogglePlay),
        ID_NEXT => Some(TrayAction::Next),
        ID_PREVIOUS => Some(TrayAction::Previous),
        ID_SHOW => Some(TrayAction::Show),
        ID_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

pub fn default_icon_rgba() -> (Vec<u8>, u32, u32) {
    let width = 32u32;
    let height = 32u32;
    let total_pixels = (width * height) as usize;
    let mut bytes = Vec::with_capacity(total_pixels * 4);
    for _ in 0..total_pixels {
        bytes.extend_from_slice(&[229, 37, 39, 255]);
    }
    (bytes, width, height)
}

/// Tray operations the application shell drives.
pub trait TrayPublisher {
    /// Publishes the active track, or `None` when nothing is loaded.
    fn set_now_playing(&self, title: Option<&str>, artist: Option<&str>);
    /// Publishes the transport state.
    fn set_playing(&self, is_playing: bool);
    /// Drains pending menu actions.
    fn drain(&self) -> Vec<TrayAction>;
}

/// Localized menu labels.
struct TrayLabels {
    play: &'static str,
    pause: &'static str,
}

impl TrayLabels {
    fn for_locale(locale: Locale) -> Self {
        Self { play: tr(locale, "Play"), pause: tr(locale, "Pause") }
    }

    fn play_pause(&self, is_playing: bool) -> &'static str {
        if is_playing { self.pause } else { self.play }
    }
}

/// Last published label and transport state; identical snapshots are not re-published.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct TraySnapshot {
    label: String,
    is_playing: Option<bool>,
}

/// Maximum number of characters the active track label may use.
pub const TRAY_LABEL_MAX_CHARS: usize = 60;

/// Builds the tray label for the active track.
///
/// Returns `"moggyt"` when no title is loaded. A non-blank artist is appended as
/// `"<title> (em dash) <artist>"`. The result is trimmed and truncated to `TRAY_LABEL_MAX_CHARS`
/// characters, ending with an ellipsis when truncation happened.
pub fn now_playing_label(title: Option<&str>, artist: Option<&str>) -> String {
    let Some(title) = title.map(str::trim).filter(|s| !s.is_empty()) else {
        return "moggyt".to_string();
    };
    let artist = artist.map(str::trim).filter(|s| !s.is_empty());
    let raw = match artist {
        Some(artist) => format!("{title} \u{2014} {artist}"),
        None => title.to_string(),
    };
    let trimmed = raw.trim();
    if trimmed.chars().count() <= TRAY_LABEL_MAX_CHARS {
        return trimmed.to_string();
    }
    let limit = TRAY_LABEL_MAX_CHARS.saturating_sub(1);
    let Some((idx, _)) = trimmed.char_indices().nth(limit) else {
        return trimmed.to_string();
    };
    let mut truncated = trimmed[..idx].to_string();
    truncated.push('\u{2026}');
    truncated
}

/// System tray handle wrapping the platform icon, menu, and state snapshot.
pub struct TrayHandle {
    tray: tray_icon::TrayIcon,
    play_pause: tray_icon::menu::MenuItem,
    labels: TrayLabels,
    published: RefCell<TraySnapshot>,
}

impl TrayHandle {
    /// Attempts to build a system tray icon, catching panics in headless environments.
    pub fn try_new(locale: Locale) -> Option<Self> {
        std::panic::catch_unwind(move || Self::build_inner(locale)).ok().flatten()
    }

    fn build_inner(locale: Locale) -> Option<Self> {
        #[cfg(target_os = "linux")]
        gtk::init().ok()?;

        let (rgba, width, height) = default_icon_rgba();
        let icon = tray_icon::Icon::from_rgba(rgba, width, height).ok()?;
        let menu = tray_icon::menu::Menu::new();
        let labels = TrayLabels::for_locale(locale);
        let play_pause =
            tray_icon::menu::MenuItem::with_id(ID_PLAY_PAUSE, labels.play_pause(false), true, None);
        let next = tray_icon::menu::MenuItem::with_id(ID_NEXT, tr(locale, "Next"), true, None);
        let previous =
            tray_icon::menu::MenuItem::with_id(ID_PREVIOUS, tr(locale, "Previous"), true, None);
        let show = tray_icon::menu::MenuItem::with_id(ID_SHOW, tr(locale, "Show"), true, None);
        let quit = tray_icon::menu::MenuItem::with_id(ID_QUIT, tr(locale, "Quit"), true, None);
        menu.append(&play_pause).ok()?;
        menu.append(&next).ok()?;
        menu.append(&previous).ok()?;
        menu.append(&show).ok()?;
        menu.append(&quit).ok()?;

        let tray = tray_icon::TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("moggyt")
            .with_icon(icon)
            .build()
            .ok()?;

        Some(Self { tray, play_pause, labels, published: RefCell::new(TraySnapshot::default()) })
    }

    /// Drains pending menu actions.
    pub fn drain(&self) -> Vec<TrayAction> {
        tray_icon::menu::MenuEvent::receiver()
            .try_iter()
            .filter_map(|event| action_from_id(event.id().as_ref()))
            .collect()
    }

    fn publish_label(&self, label: &str) {
        if self.published.borrow().label == label {
            return;
        }
        self.tray.set_title(Some(label));
        let _ = self.tray.set_tooltip(Some(label));
        self.published.borrow_mut().label = label.to_string();
        tracing::debug!(label, "tray label updated");
    }

    fn publish_playing(&self, is_playing: bool) {
        if self.published.borrow().is_playing == Some(is_playing) {
            return;
        }
        self.play_pause.set_text(self.labels.play_pause(is_playing));
        self.published.borrow_mut().is_playing = Some(is_playing);
    }
}

impl TrayPublisher for TrayHandle {
    fn set_now_playing(&self, title: Option<&str>, artist: Option<&str>) {
        let label = now_playing_label(title, artist);
        self.publish_label(&label);
    }

    fn set_playing(&self, is_playing: bool) {
        self.publish_playing(is_playing);
    }

    fn drain(&self) -> Vec<TrayAction> {
        self.drain()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_icon_rgba_dimensions_and_pixel_values() {
        let (bytes, width, height) = default_icon_rgba();
        assert_eq!(width, 32);
        assert_eq!(height, 32);
        assert_eq!(bytes.len(), 32 * 32 * 4);
        assert_eq!(&bytes[0..4], &[229, 37, 39, 255]);
    }

    #[test]
    fn tray_handle_try_new_headless_or_none() {
        let handle = TrayHandle::try_new(Locale::English);
        if let Some(h) = handle {
            let drained = h.drain();
            assert!(drained.is_empty());
        }
    }

    #[test]
    fn action_from_id_table() {
        let cases = [
            (ID_PLAY_PAUSE, Some(TrayAction::TogglePlay)),
            (ID_TOGGLE_PLAY, Some(TrayAction::TogglePlay)),
            (ID_NEXT, Some(TrayAction::Next)),
            (ID_PREVIOUS, Some(TrayAction::Previous)),
            (ID_SHOW, Some(TrayAction::Show)),
            (ID_QUIT, Some(TrayAction::Quit)),
            ("unknown", None),
            ("", None),
        ];
        for (id, expected) in cases {
            assert_eq!(action_from_id(id), expected);
        }
    }

    #[test]
    fn now_playing_label_no_track() {
        assert_eq!(now_playing_label(None, None), "moggyt");
        assert_eq!(now_playing_label(None, Some("Artist")), "moggyt");
    }

    #[test]
    fn now_playing_label_title_only() {
        assert_eq!(now_playing_label(Some("Song Title"), None), "Song Title");
    }

    #[test]
    fn now_playing_label_title_plus_artist() {
        assert_eq!(
            now_playing_label(Some("Song Title"), Some("Artist Name")),
            "Song Title \u{2014} Artist Name"
        );
    }

    #[test]
    fn now_playing_label_blank_artist() {
        assert_eq!(now_playing_label(Some("Song Title"), Some("")), "Song Title");
        assert_eq!(now_playing_label(Some("Song Title"), Some("   ")), "Song Title");
    }

    #[test]
    fn now_playing_label_200_character_title_truncates() {
        let long_title = "a".repeat(200);
        let label = now_playing_label(Some(&long_title), None);
        assert!(label.ends_with('\u{2026}'));
        assert!(label.chars().count() <= TRAY_LABEL_MAX_CHARS);
        assert_eq!(label.chars().count(), TRAY_LABEL_MAX_CHARS);
    }

    #[test]
    fn now_playing_label_multi_byte_does_not_panic() {
        let multibyte = "\u{1F980}".repeat(100);
        let label = now_playing_label(Some(&multibyte), Some("Artista"));
        assert!(label.ends_with('\u{2026}'));
        assert!(label.chars().count() <= TRAY_LABEL_MAX_CHARS);
        assert_eq!(label.chars().count(), TRAY_LABEL_MAX_CHARS);
    }

    #[test]
    fn tray_labels_spanish_play_pause() {
        let labels = TrayLabels::for_locale(Locale::Spanish);
        assert_eq!(labels.play_pause(true), tr(Locale::Spanish, "Pause"));
        assert_eq!(labels.play_pause(false), tr(Locale::Spanish, "Play"));
    }
}
