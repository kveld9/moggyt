//! Transient informational toasts rendered above the player bar.
//!
//! Hexagonal invariant: pure in-memory message buffer; the window layer only mirrors it.

use slint::ComponentHandle;

use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::{ContextIcon, MoggytWindow, ToastData, ToastsUi};

/// Lifetime of one toast, measured in 500 ms application timer ticks.
pub const TOAST_LIFETIME_TICKS: u32 = 6;

/// Maximum number of simultaneous toasts; pushing past it drops the oldest.
pub const MAX_TOASTS: usize = 3;

/// One queued informational message.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    /// Message text already localized.
    pub text: String,
    /// Icon of the action that produced the message.
    pub icon: ContextIcon,
    remaining_ticks: u32,
}

impl Toast {
    /// Fraction of the lifetime still remaining, in 0.0..=1.0.
    pub fn progress(&self) -> f32 {
        self.remaining_ticks as f32 / TOAST_LIFETIME_TICKS as f32
    }
}

/// Toast buffer owned by the UI state.
#[derive(Debug, Default)]
pub struct ToastState {
    toasts: Vec<Toast>,
}

impl ToastState {
    /// Queues a message with its action icon, dropping the oldest when the buffer is full.
    pub fn push(&mut self, text: impl Into<String>, icon: ContextIcon) {
        if self.toasts.len() >= MAX_TOASTS {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast { text: text.into(), icon, remaining_ticks: TOAST_LIFETIME_TICKS });
    }

    /// Advances one timer tick; returns true when a toast is visible or just expired.
    pub fn tick(&mut self) -> bool {
        if self.toasts.is_empty() {
            return false;
        }
        for toast in &mut self.toasts {
            toast.remaining_ticks = toast.remaining_ticks.saturating_sub(1);
        }
        self.toasts.retain(|toast| toast.remaining_ticks > 0);
        true
    }

    /// Messages to render, oldest first.
    pub fn visible(&self) -> &[Toast] {
        &self.toasts
    }

    /// Drops every message.
    pub fn clear(&mut self) {
        self.toasts.clear();
    }
}

/// Builds the toast rendered in the slot `back` positions behind the newest message.
fn slot_data(visible: &[Toast], back: usize) -> ToastData {
    visible.iter().rev().nth(back).map_or_else(
        || ToastData { text: "".into(), icon: ContextIcon::Next, progress: 0.0 },
        |toast| ToastData {
            text: toast.text.clone().into(),
            icon: toast.icon,
            progress: toast.progress(),
        },
    )
}

/// Mirrors the visible toasts into the Slint slots.
pub(crate) fn sync_toasts(window: &MoggytWindow, state: &SlintAppState) {
    let visible = state.toasts.visible();
    let ui = window.global::<ToastsUi>();
    ui.set_toast2(slot_data(visible, 0));
    ui.set_toast1(slot_data(visible, 1));
    ui.set_toast0(slot_data(visible, 2));
}

/// Queues a localized message with its action icon and mirrors it immediately.
pub(crate) fn notify(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    text: &str,
    icon: ContextIcon,
) {
    state.toasts.push(text.to_owned(), icon);
    sync_toasts(window, state);
}

/// Advances the toast buffer from the application timer.
pub(crate) fn tick_toasts(window: &MoggytWindow, state: &mut SlintAppState) {
    if state.toasts.tick() {
        sync_toasts(window, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_keeps_only_the_newest_toasts() {
        let mut state = ToastState::default();
        for index in 0..(MAX_TOASTS + 2) {
            state.push(format!("t{index}"), ContextIcon::Queue);
        }
        let texts: Vec<&str> = state.visible().iter().map(|toast| toast.text.as_str()).collect();
        assert_eq!(texts, ["t2", "t3", "t4"]);
        assert_eq!(state.visible().last().map(|toast| toast.icon), Some(ContextIcon::Queue));
    }

    #[test]
    fn tick_expires_toasts_after_the_configured_lifetime() {
        let mut state = ToastState::default();
        state.push("message", ContextIcon::Queue);

        for elapsed in 1..TOAST_LIFETIME_TICKS {
            assert!(state.tick());
            let remaining = TOAST_LIFETIME_TICKS - elapsed;
            let expected = remaining as f32 / TOAST_LIFETIME_TICKS as f32;
            assert!((state.visible()[0].progress() - expected).abs() < f32::EPSILON);
        }

        assert!(state.tick());
        assert!(state.visible().is_empty());
    }

    #[test]
    fn tick_reports_no_change_without_toasts() {
        let mut state = ToastState::default();
        assert!(!state.tick());
    }

    #[test]
    fn clear_drops_every_message() {
        let mut state = ToastState::default();
        state.push("a", ContextIcon::Queue);
        state.push("b", ContextIcon::Queue);
        state.clear();
        assert!(state.visible().is_empty());
    }
}
