//! Discord Rich Presence UI toggle handling and Slint callback dispatch wiring.
//!
//! Hexagonal invariant: pure in-memory state toggle and Slint callback dispatch
//! without blocking I/O or direct audio hardware access.

use std::rc::Rc;

use slint::ComponentHandle;

use crate::infra::{DiscordPresence, Presence};
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::{Integrations, MoggytWindow};

impl SlintAppState {
    /// Sets Discord Rich Presence active state and persists the preference.
    // NOTE(clippy): tracing macro expansion inflates cognitive complexity (rust-clippy#14417).
    #[allow(clippy::cognitive_complexity)]
    pub(crate) fn set_discord_presence(
        &mut self,
        enabled: bool,
        make: impl FnOnce() -> DiscordPresence,
    ) {
        let mut settings = self.settings_store.load();
        settings.discord_presence_enabled = enabled;
        if let Err(err) = self.settings_store.save(&settings) {
            tracing::warn!("failed to persist discord presence setting: {err:#}");
        }

        if enabled {
            self.discord = make();
            if let Some((artist, title)) = self.last_presence_key.clone() {
                self.discord.set_track(&artist, &title);
            }
        } else {
            self.discord.shutdown();
            self.discord = DiscordPresence::dormant();
        }
    }

    /// Toggles Discord Rich Presence preference, recreating or shutting down the adapter.
    pub fn toggle_discord_presence(&mut self) {
        let settings = self.settings_store.load();
        let current = settings.discord_presence_enabled;
        self.set_discord_presence(!current, DiscordPresence::new);
    }
}

/// Wires Discord Rich Presence toggle callbacks to the Slint window.
pub(crate) fn wire_discord_callbacks(window: &MoggytWindow, state_rc: &SharedState) {
    let integrations = window.global::<Integrations>();
    let initial_settings = state_rc.borrow().settings_store.load();
    integrations.set_discord_presence_enabled(initial_settings.discord_presence_enabled);

    let s = Rc::clone(state_rc);
    let w = window.as_weak();
    integrations.on_discord_presence_toggled(move || {
        let Some(win) = w.upgrade() else {
            return;
        };
        let mut st = s.borrow_mut();
        st.toggle_discord_presence();
        let enabled = st.settings_store.load().discord_presence_enabled;
        win.global::<Integrations>().set_discord_presence_enabled(enabled);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slint_shell::test_support::test_wired;

    #[test]
    fn test_set_discord_presence_persists_setting() {
        let mut state = test_wired();
        assert!(!state.settings_store.load().discord_presence_enabled);

        state.set_discord_presence(true, DiscordPresence::dormant);
        assert!(state.settings_store.load().discord_presence_enabled);

        state.set_discord_presence(false, DiscordPresence::dormant);
        assert!(!state.settings_store.load().discord_presence_enabled);
    }

    #[test]
    fn test_enabling_discord_presence_republishes_last_presence_key() {
        let mut state = test_wired();
        state.last_presence_key = Some(("Radiohead".to_string(), "Karma Police".to_string()));

        state.set_discord_presence(true, DiscordPresence::dormant);
        assert_eq!(
            state.discord.last_published(),
            Some(("Karma Police".to_string(), "Radiohead".to_string()))
        );
    }

    #[test]
    fn test_disabling_discord_presence_clears_last_published() {
        let mut state = test_wired();
        state.last_presence_key = Some(("Radiohead".to_string(), "Karma Police".to_string()));

        state.set_discord_presence(true, DiscordPresence::dormant);
        assert!(state.discord.last_published().is_some());

        state.set_discord_presence(false, DiscordPresence::dormant);
        assert_eq!(state.discord.last_published(), None);
    }
}
