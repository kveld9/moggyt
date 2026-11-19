//! Pure classification of a failed playback attempt and its automatic retry policy.
//!
//! Invariant: no I/O and no clock. Callers pass the current instant.

use std::time::Duration;

/// Classified reason a track could not start playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackFailure {
    /// No usable audio output (muted, missing device, backend unavailable).
    OutputUnavailable,
    /// Stream resolution or fetch failed (network, yt-dlp, region).
    Network,
    /// The stream resolved but the backend could not decode or play it.
    Playback,
    /// Unclassified failure.
    Unknown,
}

/// Maximum number of automatic retries scheduled for one track.
pub const MAX_PLAYBACK_RETRIES: u32 = 2;

/// Backoff before each retry attempt, indexed by attempt minus one.
pub const PLAYBACK_RETRY_DELAYS_SECS: [u64; 2] = [2, 5];

impl PlaybackFailure {
    /// Returns whether this failure can be automatically retried.
    pub fn is_retryable(self) -> bool {
        self != Self::OutputUnavailable
    }

    /// Computes the retry delay for the given attempt index (1-based).
    ///
    /// Returns `None` when the failure is not retryable, `attempt == 0`,
    /// or `attempt > MAX_PLAYBACK_RETRIES`.
    pub fn retry_delay(self, attempt: u32) -> Option<Duration> {
        if !self.is_retryable() || attempt == 0 || attempt > MAX_PLAYBACK_RETRIES {
            return None;
        }
        Some(Duration::from_secs(PLAYBACK_RETRY_DELAYS_SECS[(attempt - 1) as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_failure_is_retryable_table() {
        assert!(!PlaybackFailure::OutputUnavailable.is_retryable());
        assert!(PlaybackFailure::Network.is_retryable());
        assert!(PlaybackFailure::Playback.is_retryable());
        assert!(PlaybackFailure::Unknown.is_retryable());
    }

    #[test]
    fn playback_failure_retry_delay_attempts() {
        assert_eq!(PlaybackFailure::Network.retry_delay(0), None);
        assert_eq!(PlaybackFailure::Network.retry_delay(1), Some(Duration::from_secs(2)));
        assert_eq!(PlaybackFailure::Network.retry_delay(2), Some(Duration::from_secs(5)));
        assert_eq!(PlaybackFailure::Network.retry_delay(3), None);

        assert_eq!(PlaybackFailure::Playback.retry_delay(1), Some(Duration::from_secs(2)));
        assert_eq!(PlaybackFailure::Playback.retry_delay(2), Some(Duration::from_secs(5)));
        assert_eq!(PlaybackFailure::Playback.retry_delay(3), None);

        assert_eq!(PlaybackFailure::Unknown.retry_delay(1), Some(Duration::from_secs(2)));
        assert_eq!(PlaybackFailure::Unknown.retry_delay(2), Some(Duration::from_secs(5)));
        assert_eq!(PlaybackFailure::Unknown.retry_delay(3), None);

        assert_eq!(PlaybackFailure::OutputUnavailable.retry_delay(0), None);
        assert_eq!(PlaybackFailure::OutputUnavailable.retry_delay(1), None);
        assert_eq!(PlaybackFailure::OutputUnavailable.retry_delay(2), None);
        assert_eq!(PlaybackFailure::OutputUnavailable.retry_delay(3), None);
    }
}
