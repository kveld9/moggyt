//! Transient informational toasts rendered above the player bar.
//!
//! Hexagonal invariant: pure in-memory message buffer; the window layer only mirrors it.

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::{MoggytWindow, ToastData, ToastsUi};

/// Lifetime of one toast, measured in 500 ms application timer ticks.
pub const TOAST_LIFETIME_TICKS: u32 = 6;

/// Maximum number of simultaneous toasts; pushing past it drops the oldest.
pub const MAX_TOASTS: usize = 3;

/// One queued informational message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    /// Message text already localized.
    pub text: String,
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
    /// Queues a message, dropping the oldest when the buffer is full.
    pub fn push(&mut self, text: impl Into<String>) {
        if self.toasts.len() >= MAX_TOASTS {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast { text: text.into(), remaining_ticks: TOAST_LIFETIME_TICKS });
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

/// Mirrors the visible toasts into the Slint global.
pub(crate) fn sync_toasts(window: &MoggytWindow, state: &SlintAppState) {
    let items: Vec<ToastData> = state
        .toasts
        .visible()
        .iter()
        .map(|toast| ToastData { text: toast.text.clone().into(), progress: toast.progress() })
        .collect();
    window.global::<ToastsUi>().set_items(ModelRc::new(VecModel::from(items)));
}

/// Queues a localized message and mirrors it immediately.
pub(crate) fn notify(window: &MoggytWindow, state: &mut SlintAppState, text: &str) {
    state.toasts.push(text.to_owned());
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
            state.push(format!("t{index}"));
        }
        let texts: Vec<&str> = state.visible().iter().map(|toast| toast.text.as_str()).collect();
        assert_eq!(texts, ["t2", "t3", "t4"]);
    }

    #[test]
    fn tick_expires_toasts_after_the_configured_lifetime() {
        let mut state = ToastState::default();
        state.push("message");

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
        state.push("a");
        state.push("b");
        state.clear();
        assert!(state.visible().is_empty());
    }
}
