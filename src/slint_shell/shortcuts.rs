//! Global keyboard shortcut actions, key event mapping, and window dispatch wiring.
//!
//! Hexagonal invariant: pure in-memory event mapping and Slint callback dispatch
//! without blocking I/O or direct audio hardware access.

use slint::ComponentHandle;
use slint::platform::Key;

use crate::slint_shell::{MoggytWindow, PlayerUi};

/// Actions triggered by global keyboard shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    TogglePlay,
    SeekBackward,
    SeekForward,
    VolumeUp,
    VolumeDown,
    NextTrack,
    PreviousTrack,
    ToggleLike,
}

/// Playback seek jump duration in seconds when triggered by shortcut keys.
pub const SHORTCUT_SEEK_SECS: i32 = 10;

/// Volume delta percentage step when adjusted via shortcut keys.
pub const SHORTCUT_VOLUME_STEP: i32 = 5;

fn arrow_shortcut(text: &str) -> Option<ShortcutAction> {
    let left = String::from(char::from(Key::LeftArrow));
    let right = String::from(char::from(Key::RightArrow));
    let up = String::from(char::from(Key::UpArrow));
    let down = String::from(char::from(Key::DownArrow));

    if text == left {
        Some(ShortcutAction::SeekBackward)
    } else if text == right {
        Some(ShortcutAction::SeekForward)
    } else if text == up {
        Some(ShortcutAction::VolumeUp)
    } else if text == down {
        Some(ShortcutAction::VolumeDown)
    } else {
        None
    }
}

fn letter_shortcut(text: &str) -> Option<ShortcutAction> {
    match text {
        " " => Some(ShortcutAction::TogglePlay),
        "n" | "N" => Some(ShortcutAction::NextTrack),
        "p" | "P" => Some(ShortcutAction::PreviousTrack),
        "l" | "L" => Some(ShortcutAction::ToggleLike),
        _ => None,
    }
}

/// Resolves an incoming key event text and modifier flag into an action, if mapped.
pub fn shortcut_action(text: &str, has_modifier: bool) -> Option<ShortcutAction> {
    if has_modifier {
        return None;
    }
    arrow_shortcut(text).or_else(|| letter_shortcut(text))
}

/// Steps volume by the given delta, clamping within valid range [0, 100].
pub fn stepped_volume(current: i32, delta: i32) -> i32 {
    current.saturating_add(delta).clamp(0, 100)
}

fn dispatch_transport_shortcut(window: &MoggytWindow, action: ShortcutAction) -> bool {
    let player = window.global::<PlayerUi>();
    match action {
        ShortcutAction::TogglePlay => player.invoke_toggle_play(),
        ShortcutAction::SeekBackward => player.invoke_seek_by(-SHORTCUT_SEEK_SECS),
        ShortcutAction::SeekForward => player.invoke_seek_by(SHORTCUT_SEEK_SECS),
        ShortcutAction::NextTrack => player.invoke_next_track(),
        _ => return false,
    }
    true
}

fn dispatch_shortcut(window: &MoggytWindow, action: ShortcutAction) {
    if dispatch_transport_shortcut(window, action) {
        return;
    }
    let player = window.global::<PlayerUi>();
    match action {
        ShortcutAction::VolumeUp => {
            player.invoke_set_volume(stepped_volume(player.get_volume(), SHORTCUT_VOLUME_STEP));
        }
        ShortcutAction::VolumeDown => {
            player.invoke_set_volume(stepped_volume(player.get_volume(), -SHORTCUT_VOLUME_STEP));
        }
        ShortcutAction::PreviousTrack => player.invoke_prev_track(),
        ShortcutAction::ToggleLike => player.invoke_toggle_like(),
        _ => {}
    }
}

/// Wires shortcut pressed event callback to MoggytWindow.
pub(crate) fn wire_shortcut_callbacks(window: &MoggytWindow) {
    let w = window.as_weak();
    window.on_shortcut_pressed(move |text, has_modifier| {
        let Some(win) = w.upgrade() else {
            return false;
        };
        let Some(action) = shortcut_action(text.as_str(), has_modifier) else {
            return false;
        };
        dispatch_shortcut(&win, action);
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shortcut_action_mapping() {
        assert_eq!(shortcut_action(" ", false), Some(ShortcutAction::TogglePlay));

        let left = String::from(char::from(Key::LeftArrow));
        let right = String::from(char::from(Key::RightArrow));
        let up = String::from(char::from(Key::UpArrow));
        let down = String::from(char::from(Key::DownArrow));

        assert_eq!(shortcut_action(&left, false), Some(ShortcutAction::SeekBackward));
        assert_eq!(shortcut_action(&right, false), Some(ShortcutAction::SeekForward));
        assert_eq!(shortcut_action(&up, false), Some(ShortcutAction::VolumeUp));
        assert_eq!(shortcut_action(&down, false), Some(ShortcutAction::VolumeDown));

        assert_eq!(shortcut_action("n", false), Some(ShortcutAction::NextTrack));
        assert_eq!(shortcut_action("N", false), Some(ShortcutAction::NextTrack));
        assert_eq!(shortcut_action("p", false), Some(ShortcutAction::PreviousTrack));
        assert_eq!(shortcut_action("P", false), Some(ShortcutAction::PreviousTrack));
        assert_eq!(shortcut_action("l", false), Some(ShortcutAction::ToggleLike));
        assert_eq!(shortcut_action("L", false), Some(ShortcutAction::ToggleLike));

        // Unknown keys and empty string
        assert_eq!(shortcut_action("x", false), None);
        assert_eq!(shortcut_action("Enter", false), None);
        assert_eq!(shortcut_action("", false), None);
    }

    #[test]
    fn test_shortcut_action_modifiers_suppress_all() {
        let left = String::from(char::from(Key::LeftArrow));
        let right = String::from(char::from(Key::RightArrow));
        let up = String::from(char::from(Key::UpArrow));
        let down = String::from(char::from(Key::DownArrow));

        let keys = [" ", &left, &right, &up, &down, "n", "N", "p", "P", "l", "L"];
        for key in keys {
            assert_eq!(shortcut_action(key, true), None, "modifier must suppress key: {key}");
        }
    }

    #[test]
    fn test_stepped_volume_clamping() {
        assert_eq!(stepped_volume(98, 5), 100);
        assert_eq!(stepped_volume(3, -5), 0);
        assert_eq!(stepped_volume(50, 5), 55);
        assert_eq!(stepped_volume(50, -5), 45);
    }
}
