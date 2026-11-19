//! Tray projection of the active track and transport state.
//!
//! Hexagonal invariant: reads application state only; the tray adapter owns the side effects.

use slint::ComponentHandle;

use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::{MoggytWindow, PlayerUi};

/// Publishes the active track and the transport state to the system tray.
pub fn sync_tray(window: &MoggytWindow, state: &SlintAppState) {
    let Some(tray) = &state.tray else {
        return;
    };
    let current = state.queue.current();
    tray.set_now_playing(
        current.map(|track| track.title.as_str()),
        current.map(|track| track.artist.as_str()),
    );
    tray.set_playing(window.global::<PlayerUi>().get_is_playing());
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use slint::ComponentHandle;

    use super::*;
    use crate::domain::Track;
    use crate::infra::{TrayAction, TrayPublisher};
    use crate::slint_shell::test_support::test_wired;

    struct FakeTray {
        log: Rc<RefCell<Vec<String>>>,
    }

    impl FakeTray {
        fn new() -> (Self, Rc<RefCell<Vec<String>>>) {
            let log = Rc::new(RefCell::new(Vec::new()));
            (Self { log: Rc::clone(&log) }, log)
        }
    }

    impl TrayPublisher for FakeTray {
        fn set_now_playing(&self, title: Option<&str>, artist: Option<&str>) {
            self.log.borrow_mut().push(format!(
                "now_playing:({}, {})",
                title.unwrap_or("none"),
                artist.unwrap_or("none")
            ));
        }

        fn set_playing(&self, is_playing: bool) {
            self.log.borrow_mut().push(format!("playing:{is_playing}"));
        }

        fn drain(&self) -> Vec<TrayAction> {
            Vec::new()
        }
    }

    #[test]
    fn test_sync_tray_no_tray_does_nothing() {
        let Ok(window) = MoggytWindow::new() else {
            return;
        };
        let state = test_wired();
        assert!(state.tray.is_none());
        sync_tray(&window, &state);
    }

    #[test]
    fn test_sync_tray_publishes_now_playing_track() {
        let Ok(window) = MoggytWindow::new() else {
            return;
        };
        let mut state = test_wired();
        let (fake, log) = FakeTray::new();
        state.tray = Some(Box::new(fake));
        state.queue.push(Track::new("t1", "Title", "Artist"));

        sync_tray(&window, &state);

        let entries = log.borrow();
        assert!(entries.contains(&"now_playing:(Title, Artist)".to_string()));
    }

    #[test]
    fn test_sync_tray_forwards_playing_flag() {
        let Ok(window) = MoggytWindow::new() else {
            return;
        };
        let mut state = test_wired();
        let (fake, log) = FakeTray::new();
        state.tray = Some(Box::new(fake));

        window.global::<PlayerUi>().set_is_playing(true);
        sync_tray(&window, &state);
        assert!(log.borrow().contains(&"playing:true".to_string()));

        window.global::<PlayerUi>().set_is_playing(false);
        sync_tray(&window, &state);
        assert!(log.borrow().contains(&"playing:false".to_string()));
    }
}
