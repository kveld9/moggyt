//! User-facing notification, error state, and retry recovery for playback failures.

use std::time::Instant;

use slint::ComponentHandle;

use crate::domain::{MAX_PLAYBACK_RETRIES, PlaybackFailure, PlaybackPhase};
use crate::i18n::{Locale, tr};
use crate::slint_shell::playback_worker::PlayTrackResult;
use crate::slint_shell::state::SlintAppState;
use crate::slint_shell::toast::notify;
use crate::slint_shell::{ContextIcon, MoggytWindow, PlayerUi};

/// Automatic recovery plan for a failed play attempt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PlaybackRecovery {
    /// No automatic retry is pending.
    #[default]
    Idle,
    /// A retry for `track_id` becomes due at `due_at`.
    Scheduled { track_id: String, attempt: u32, due_at: Instant },
}

/// User-visible state of the last failed play attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackErrorState {
    /// Classified failure shown in the player bar.
    pub failure: PlaybackFailure,
    /// Scheduled retry attempt, when automatic recovery is pending.
    pub retry_attempt: Option<u32>,
}

fn reason_key(failure: PlaybackFailure) -> &'static str {
    match failure {
        PlaybackFailure::OutputUnavailable => "Audio output unavailable",
        PlaybackFailure::Network => "Network error",
        PlaybackFailure::Playback => "Playback failed",
        PlaybackFailure::Unknown => "Failed to play track",
    }
}

/// Builds the localized failure message reported in the player bar or toast.
pub(crate) fn failure_message(
    locale: Locale,
    failure: PlaybackFailure,
    retry_attempt: Option<u32>,
    title: &str,
) -> String {
    let reason = tr(locale, reason_key(failure));
    match retry_attempt {
        Some(attempt) => {
            let retrying = tr(locale, "Retrying");
            format!("{reason}. {retrying} ({attempt}/{MAX_PLAYBACK_RETRIES}): {title}")
        }
        None => format!("{reason}: {title}"),
    }
}

/// Builds the short player-bar variant of a failure message.
///
/// The bar is only 260px wide and the track title sits on the line above, so a scheduled retry
/// shows the transport state alone (`"Retrying (1/2)"`) and a final failure shows the reason.
/// The toast carries the full reason-plus-title message.
pub(crate) fn failure_short_message(
    locale: Locale,
    failure: PlaybackFailure,
    retry_attempt: Option<u32>,
) -> String {
    match retry_attempt {
        Some(attempt) => {
            let retrying = tr(locale, "Retrying");
            format!("{retrying} ({attempt}/{MAX_PLAYBACK_RETRIES})")
        }
        None => tr(locale, reason_key(failure)).to_string(),
    }
}

/// Synchronizes the current playback failure text into the Slint player UI.
pub(crate) fn sync_playback_error(window: &MoggytWindow, state: &SlintAppState) {
    let text = match &state.playback_error {
        Some(err) => failure_short_message(state.locale, err.failure, err.retry_attempt),
        None => String::new(),
    };
    window.global::<PlayerUi>().set_playback_error(text.into());
}

/// Reports a playback start failure, updates state, and notifies the user.
///
/// A failed gapless preload is not reported because the active track is unaffected.
pub(crate) fn report_playback_failure(
    window: &MoggytWindow,
    state: &mut SlintAppState,
    res: &PlayTrackResult,
) {
    if res.gapless {
        return;
    }
    let failure = res.failure.unwrap_or(PlaybackFailure::Unknown);
    let retry = state.schedule_playback_recovery(failure, &res.track_id, Instant::now());
    state.playback_error = Some(PlaybackErrorState { failure, retry_attempt: retry });
    sync_playback_error(window, state);
    let message = failure_message(state.locale, failure, retry, &res.title);
    notify(window, state, &message, ContextIcon::Close);
}

/// Clears any reported playback failure and resets the recovery plan.
pub(crate) fn clear_playback_failure(window: &MoggytWindow, state: &mut SlintAppState) {
    state.playback_error = None;
    state.playback_recovery = PlaybackRecovery::Idle;
    sync_playback_error(window, state);
}

impl SlintAppState {
    /// Schedules an automatic retry attempt with backoff for a failed track.
    pub fn schedule_playback_recovery(
        &mut self,
        failure: PlaybackFailure,
        track_id: &str,
        now: Instant,
    ) -> Option<u32> {
        if !failure.is_retryable() {
            self.playback_recovery = PlaybackRecovery::Idle;
            return None;
        }
        let attempt = match &self.playback_recovery {
            PlaybackRecovery::Scheduled { track_id: current_id, attempt, .. }
                if current_id == track_id =>
            {
                attempt + 1
            }
            _ => 1,
        };
        let Some(delay) = failure.retry_delay(attempt) else {
            self.playback_recovery = PlaybackRecovery::Idle;
            return None;
        };
        self.playback_recovery = PlaybackRecovery::Scheduled {
            track_id: track_id.to_string(),
            attempt,
            due_at: now + delay,
        };
        Some(attempt)
    }

    /// Polls pending recovery and dispatches retry if due.
    pub fn poll_playback_recovery(&mut self, now: Instant) -> bool {
        if self.playback_phase == PlaybackPhase::Resolving {
            return false;
        }
        let PlaybackRecovery::Scheduled { due_at, .. } = self.playback_recovery else {
            return false;
        };
        if now < due_at {
            return false;
        }
        let dispatched = self.request_current_play();
        if !dispatched {
            self.playback_recovery = PlaybackRecovery::Idle;
        }
        dispatched
    }

    /// Triggers immediate manual retry of the current failed track.
    pub fn retry_playback_failure(&mut self) {
        self.playback_recovery = PlaybackRecovery::Idle;
        self.request_current_play();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

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
    fn failure_message_localizes_reasons_with_and_without_retry() {
        let reasons = [
            (
                PlaybackFailure::OutputUnavailable,
                "Audio output unavailable: Caprichosa",
                "Salida de audio no disponible: Caprichosa",
            ),
            (PlaybackFailure::Network, "Network error: Caprichosa", "Error de red: Caprichosa"),
            (
                PlaybackFailure::Playback,
                "Playback failed: Caprichosa",
                "Error de reproducción: Caprichosa",
            ),
            (
                PlaybackFailure::Unknown,
                "Failed to play track: Caprichosa",
                "Error al reproducir la canción: Caprichosa",
            ),
        ];

        for (failure, expected_en, expected_es) in reasons {
            assert_eq!(failure_message(Locale::English, failure, None, "Caprichosa"), expected_en);
            assert_eq!(failure_message(Locale::Spanish, failure, None, "Caprichosa"), expected_es);
        }

        assert_eq!(
            failure_message(Locale::English, PlaybackFailure::Network, Some(1), "Caprichosa"),
            "Network error. Retrying (1/2): Caprichosa"
        );
        assert_eq!(
            failure_message(Locale::Spanish, PlaybackFailure::Network, Some(1), "Caprichosa"),
            "Error de red. Reintentando (1/2): Caprichosa"
        );
        assert_eq!(
            failure_message(Locale::English, PlaybackFailure::Playback, Some(2), "Caprichosa"),
            "Playback failed. Retrying (2/2): Caprichosa"
        );
        assert_eq!(
            failure_message(Locale::Spanish, PlaybackFailure::Playback, Some(2), "Caprichosa"),
            "Error de reproducción. Reintentando (2/2): Caprichosa"
        );

        assert_eq!(
            failure_short_message(Locale::English, PlaybackFailure::Network, Some(1)),
            "Retrying (1/2)"
        );
        assert_eq!(
            failure_short_message(Locale::Spanish, PlaybackFailure::Network, Some(1)),
            "Reintentando (1/2)"
        );
        assert_eq!(
            failure_short_message(Locale::English, PlaybackFailure::Playback, None),
            "Playback failed"
        );
        assert_eq!(
            failure_short_message(Locale::Spanish, PlaybackFailure::OutputUnavailable, None),
            "Salida de audio no disponible"
        );
    }

    #[test]
    fn schedule_playback_recovery_lifecycle() {
        let mut state = test_wired();
        let now = Instant::now();

        // OutputUnavailable is never retryable
        assert_eq!(
            state.schedule_playback_recovery(PlaybackFailure::OutputUnavailable, "t1", now),
            None
        );
        assert_eq!(state.playback_recovery, PlaybackRecovery::Idle);

        // First attempt for t1
        assert_eq!(state.schedule_playback_recovery(PlaybackFailure::Network, "t1", now), Some(1));
        assert_eq!(
            state.playback_recovery,
            PlaybackRecovery::Scheduled {
                track_id: "t1".into(),
                attempt: 1,
                due_at: now + Duration::from_secs(2),
            }
        );

        // Second attempt for t1 increments attempt and delay
        assert_eq!(state.schedule_playback_recovery(PlaybackFailure::Network, "t1", now), Some(2));
        assert_eq!(
            state.playback_recovery,
            PlaybackRecovery::Scheduled {
                track_id: "t1".into(),
                attempt: 2,
                due_at: now + Duration::from_secs(5),
            }
        );

        // Exceeding MAX_PLAYBACK_RETRIES resets to Idle
        assert_eq!(state.schedule_playback_recovery(PlaybackFailure::Network, "t1", now), None);
        assert_eq!(state.playback_recovery, PlaybackRecovery::Idle);

        // Switching track resets attempt count to 1
        assert_eq!(state.schedule_playback_recovery(PlaybackFailure::Network, "t1", now), Some(1));
        assert_eq!(state.schedule_playback_recovery(PlaybackFailure::Network, "t2", now), Some(1));
        assert_eq!(
            state.playback_recovery,
            PlaybackRecovery::Scheduled {
                track_id: "t2".into(),
                attempt: 1,
                due_at: now + Duration::from_secs(2),
            }
        );
    }

    #[test]
    fn poll_playback_recovery_timing() {
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Track 1", "Artist 1"));
        let now = Instant::now();

        state.schedule_playback_recovery(PlaybackFailure::Network, "t1", now);
        // Before due_at: does not dispatch
        assert!(!state.poll_playback_recovery(now + Duration::from_secs(1)));

        // At or after due_at: dispatches retry
        assert!(state.poll_playback_recovery(now + Duration::from_secs(2)));

        // While Resolving: does not dispatch duplicate
        state.set_playback_phase(PlaybackPhase::Resolving);
        assert!(!state.poll_playback_recovery(now + Duration::from_secs(3)));
    }

    #[test]
    fn failed_completion_leaves_error_state_and_success_clears_it() {
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
                failure: Some(PlaybackFailure::Network),
                lyrics_lines: Vec::new(),
                cover_data: None,
                presence: PresenceSnapshot::new("Caprichosa", "Santa Fe Klan", false),
                gapless: false,
            },
        );

        assert_eq!(window.get_active_track_title().as_str(), "Caprichosa");
        assert!(!window.global::<PlayerUi>().get_is_playing());
        let err = window.global::<PlayerUi>().get_playback_error();
        assert_eq!(
            err.as_str(),
            failure_short_message(state.locale, PlaybackFailure::Network, Some(1))
        );
        assert!(!err.contains("Caprichosa"));
        assert!(matches!(state.playback_recovery, PlaybackRecovery::Scheduled { .. }));

        let toasts = state.toasts.visible();
        assert_eq!(toasts.len(), 1);

        // Next successful completion clears error
        apply_play_track_completion(
            &window,
            &mut state,
            PlayTrackResult {
                generation: 2,
                track_id: "t1".into(),
                title: "Caprichosa".into(),
                artist: "Santa Fe Klan".into(),
                success: true,
                failure: None,
                lyrics_lines: Vec::new(),
                cover_data: None,
                presence: PresenceSnapshot::new("Caprichosa", "Santa Fe Klan", true),
                gapless: false,
            },
        );

        assert_eq!(window.global::<PlayerUi>().get_playback_error().as_str(), "");
        assert_eq!(state.playback_recovery, PlaybackRecovery::Idle);
        assert_eq!(state.playback_error, None);
    }

    #[test]
    fn poll_playback_recovery_clears_the_plan_when_no_request_is_possible() {
        let mut state = test_wired();
        let now = Instant::now();
        state.schedule_playback_recovery(PlaybackFailure::Network, "t1", now);

        assert!(!state.poll_playback_recovery(now + Duration::from_secs(10)));
        assert_eq!(state.playback_recovery, PlaybackRecovery::Idle);
    }

    #[test]
    fn pausing_cancels_a_scheduled_retry() {
        let mut state = test_wired();
        let now = Instant::now();
        state.schedule_playback_recovery(PlaybackFailure::Network, "t1", now);

        state.set_playback_phase(PlaybackPhase::Paused);

        assert_eq!(state.playback_recovery, PlaybackRecovery::Idle);
    }

    #[test]
    fn failed_gapless_completion_does_not_surface_error_or_schedule_recovery() {
        let Ok(window) = MoggytWindow::new() else {
            return;
        };
        let mut state = test_wired();
        state.queue.push(Track::new("t1", "Current Track", "Artist"));
        state.queue.push(Track::new("t2", "Preloaded Track", "Artist"));

        apply_play_track_completion(
            &window,
            &mut state,
            PlayTrackResult {
                generation: 1,
                track_id: "t2".into(),
                title: "Preloaded Track".into(),
                artist: "Artist".into(),
                success: false,
                failure: Some(PlaybackFailure::Network),
                lyrics_lines: Vec::new(),
                cover_data: None,
                presence: PresenceSnapshot::new("Preloaded Track", "Artist", false),
                gapless: true,
            },
        );

        assert_eq!(state.playback_error, None);
        assert_eq!(state.playback_recovery, PlaybackRecovery::Idle);
        assert_eq!(state.toasts.visible().len(), 0);
        assert_eq!(window.global::<PlayerUi>().get_playback_error().as_str(), "");
    }
}
