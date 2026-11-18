//! Publishes the logical window width to the Slint shell.
//!
//! Slint exposes the window size only on the Rust side; deriving it inside the UI feeds the
//! layout pass back into itself, which the Slint compiler reports as a deprecated binding loop.
//! The shell therefore reads the size here and writes it to `PlayerBarUi.window-width`.

use slint::ComponentHandle;

use crate::slint_shell::{MoggytWindow, PlayerBarUi};

/// Converts a physical window width into logical pixels, guarding a degenerate scale factor.
pub(crate) fn logical_window_width(physical_width: u32, scale_factor: f32) -> f32 {
    if scale_factor <= 0.0 {
        return 0.0;
    }
    physical_width as f32 / scale_factor
}

/// Publishes the window's current logical width so overlay-aware padding can follow it.
pub(crate) fn sync_window_width(window: &MoggytWindow) {
    let slint_window = window.window();
    let width = logical_window_width(slint_window.size().width, slint_window.scale_factor());
    if width <= 0.0 {
        return;
    }
    let ui = window.global::<PlayerBarUi>();
    if (ui.get_window_width() - width).abs() > 0.5 {
        ui.set_window_width(width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_window_width_divides_by_scale_factor() {
        assert_eq!(logical_window_width(1600, 1.0), 1600.0);
        assert_eq!(logical_window_width(3200, 2.0), 1600.0);
        assert_eq!(logical_window_width(2400, 1.5), 1600.0);
    }

    #[test]
    fn logical_window_width_guards_degenerate_inputs() {
        assert_eq!(logical_window_width(0, 1.0), 0.0);
        assert_eq!(logical_window_width(1600, 0.0), 0.0);
        assert_eq!(logical_window_width(1600, -1.0), 0.0);
    }

    #[test]
    fn sync_window_width_publishes_the_current_window_width() {
        let Ok(win) = MoggytWindow::new() else {
            return;
        };
        sync_window_width(&win);
        let slint_window = win.window();
        let expected = logical_window_width(slint_window.size().width, slint_window.scale_factor());
        let published = win.global::<PlayerBarUi>().get_window_width();
        assert!((published - expected).abs() <= 0.5);
    }
}
