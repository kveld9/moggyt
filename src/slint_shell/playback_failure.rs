//! User-facing notification when a track cannot start playback.

use crate::i18n::{Locale, tr};
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::toast::notify;
use crate::slint_shell::{ContextIcon, MoggytWindow};

/// Builds the localized message reported when a track fails to start playback.
pub(crate) fn failure_message(locale: Locale, title: &str) -> String {
    format!("{}: {}", tr(locale, "Failed to play track"), title)
}

/// Reports a playback start failure without replacing the current track metadata.
pub(crate) fn notify_playback_failure(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    title: &str,
) {
    let message = failure_message(state.locale, title);
    notify(window, state, &message, ContextIcon::Close);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Track;
    use crate::slint_shell::MoggytWindow;
    use crate::slint_shell::PlayerUi;
    use crate::slint_shell::platform::PresenceSnapshot;
    use crate::slint_shell::playback_control::apply_play_track_completion;
    use crate::slint_shell::playback_worker::PlayTrackResult;
    use crate::slint_shell::test_support::test_wired;
    use slint::ComponentHandle;

    #[test]
    fn failure_message_localizes_and_names_the_track() {
        assert_eq!(
            failure_message(Locale::English, "Caprichosa"),
            "Failed to play track: Caprichosa"
        );
        assert_eq!(
            failure_message(Locale::Spanish, "Caprichosa"),
            "Error al reproducir la canción: Caprichosa"
        );
    }

    #[test]
    fn failed_completion_keeps_the_title_and_queues_a_toast() {
        let Ok(window) = MoggytWindow::new() else {
            return;
        };
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Caprichosa", "Santa Fe Klan"));
        window.set_active_track_title("Caprichosa".into());

        apply_play_track_completion(
            &window,
            &mut state,
            PlayTrackResult {
                generation: 1,
                track_id: "t1".into(),
                title: "Caprichosa".into(),
                artist: "Santa Fe Klan".into(),
                success: false,
                lyrics_lines: Vec::new(),
                cover_data: None,
                presence: PresenceSnapshot::new("Caprichosa", "Santa Fe Klan", false),
            },
        );

        assert_eq!(window.get_active_track_title().as_str(), "Caprichosa");
        assert!(!window.global::<PlayerUi>().get_is_playing());
        let toasts = state.toasts.visible();
        assert_eq!(toasts.len(), 1);
        assert_eq!(toasts[0].text, failure_message(state.locale, "Caprichosa"));
    }
}
