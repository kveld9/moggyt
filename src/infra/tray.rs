//! System tray icon integration with headless fallback.

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

pub struct TrayHandle {
    _tray: tray_icon::TrayIcon,
}

impl TrayHandle {
    pub fn try_new() -> Option<Self> {
        std::panic::catch_unwind(Self::build_inner).ok().flatten()
    }

    fn build_inner() -> Option<Self> {
        let (rgba, width, height) = default_icon_rgba();
        let icon = tray_icon::Icon::from_rgba(rgba, width, height).ok()?;
        let menu = tray_icon::menu::Menu::new();
        let play_pause =
            tray_icon::menu::MenuItem::with_id(ID_PLAY_PAUSE, "Play/Pause", true, None);
        let next = tray_icon::menu::MenuItem::with_id(ID_NEXT, "Next", true, None);
        let previous = tray_icon::menu::MenuItem::with_id(ID_PREVIOUS, "Previous", true, None);
        let show = tray_icon::menu::MenuItem::with_id(ID_SHOW, "Show", true, None);
        let quit = tray_icon::menu::MenuItem::with_id(ID_QUIT, "Quit", true, None);
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

        Some(Self { _tray: tray })
    }

    pub fn drain(&self) -> Vec<TrayAction> {
        tray_icon::menu::MenuEvent::receiver()
            .try_iter()
            .filter_map(|event| action_from_id(event.id().as_ref()))
            .collect()
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
        let handle = TrayHandle::try_new();
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
}
