//! Settings callbacks: account sign-in, preferences, sleep timer and window resize.

use std::rc::Rc;

use slint::ComponentHandle;

use crate::domain::{ThemeId, TimerPreset};
use crate::slint_shell::MoggytWindow;
use crate::slint_shell::explore::explore_columns_for_width;
use crate::slint_shell::format::timer_preset_from_index;
use crate::slint_shell::playback_worker::PlaybackWorkerCommand;
use crate::slint_shell::player_bar::mode_from_index;
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::sync::{sync_explore_shelves, sync_sleep_timer, sync_window};
use crate::slint_shell::theme::{apply_palette, theme_from_index};
use crate::slint_shell::window::refresh_library_async;

/// Wires settings, account authentication, player bar preferences, and resize callbacks.
pub(crate) fn wire_settings_callbacks(
    window: &MoggytWindow,
    state_rc: &SharedState,
    worker_tx: &std::sync::mpsc::Sender<PlaybackWorkerCommand>,
) {
    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_sign_in_submitted(move |cookies| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        if st.sign_in_from_input(cookies.as_str()) {
            win.set_cookie_text("".into());
            win.set_settings_cookie_text("".into());
            refresh_library_async(&win, &mut st, &tx);
        }
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    let tx = worker_tx.clone();
    window.on_sign_out(move || {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.sign_out();
        refresh_library_async(&win, &mut st, &tx);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_theme_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let theme = theme_from_index(idx);
        st.set_theme(theme);
        if theme == ThemeId::Custom {
            st.reload_custom_theme();
        }
        apply_palette(&win, &st.active_palette());
        win.set_custom_theme_status_text(st.custom_theme_status.clone().into());
        win.set_active_theme_index(idx);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_player_bar_mode_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let mode = mode_from_index(idx);
        st.set_player_bar_mode(mode);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_player_bar_option_toggled(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        st.toggle_player_bar_option(idx);
        sync_window(&win, &mut st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_sleep_timer_preset_selected(move |idx| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        handle_sleep_timer_selection(&mut st, idx);
        sync_sleep_timer(&win, &st);
    });

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    window.on_width_changed(move |width| {
        let Some(win) = w.upgrade() else { return };
        let mut st = s.borrow_mut();
        let columns = explore_columns_for_width(width);
        if st.explore_columns == columns {
            return;
        }
        st.explore_columns = columns;
        if st.explore_loaded {
            sync_explore_shelves(&win, &st);
        }
    });
}

/// Handles sleep timer preset selection, arming or canceling the timer.
pub(crate) fn handle_sleep_timer_selection(state: &mut SlintAppState, idx: i32) {
    state.restore_sleep_fade_volume();
    let preset = timer_preset_from_index(idx);
    let track_id = state.queue.current().map(|t| t.id.clone());
    if preset == TimerPreset::Off {
        state.sleep_timer.cancel();
        state.sleep_timer_accumulated = 0;
        state.sleep_timer_base_offset = 0;
    } else {
        state.sleep_timer.set_preset_with_track(preset, track_id.as_deref());
        let pos = state.audio.try_lock().map(|a| a.position_secs()).unwrap_or(0);
        state.sleep_timer_accumulated = 0;
        state.sleep_timer_base_offset = pos;
    }
}

#[cfg(test)]
mod tests {
    use super::handle_sleep_timer_selection;
    use crate::domain::{TimerPreset, Track};
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_sleep_timer_selection_callback_path() {
        let mut state = test_wired();
        assert!(!state.sleep_timer.is_armed());
        assert_eq!(state.sleep_timer.preset(), TimerPreset::Off);

        // Picking 30m (index 2) arms the timer in state
        handle_sleep_timer_selection(&mut state, 2);
        assert!(state.sleep_timer.is_armed());
        assert_eq!(state.sleep_timer.preset(), TimerPreset::Minutes30);
        assert_eq!(state.sleep_timer.remaining_secs(), Some(1800));

        // Picking Off (index 0) cancels it
        handle_sleep_timer_selection(&mut state, 0);
        assert!(!state.sleep_timer.is_armed());
        assert_eq!(state.sleep_timer.preset(), TimerPreset::Off);
        assert_eq!(state.sleep_timer.remaining_secs(), None);

        // Picking End of track (index 5) with a track arms it with current track
        state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
        handle_sleep_timer_selection(&mut state, 5);
        assert!(state.sleep_timer.is_armed());
        assert_eq!(state.sleep_timer.preset(), TimerPreset::StopAfterTrack);
    }
}
