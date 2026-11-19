//! Track preview session state.

/// Duration of a single track preview, in seconds.
pub const PREVIEW_SECS: u64 = 20;

/// Preview session: at most one preview exists at a time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PreviewState {
    /// No preview is running.
    #[default]
    Idle,
    /// The backend is resolving and starting the preview off the UI thread.
    Starting { track_id: String, title: String },
    /// The preview is audible.
    Playing { track_id: String },
}

impl PreviewState {
    /// Returns whether a preview is running.
    pub fn is_playing(&self) -> bool {
        matches!(self, Self::Playing { .. })
    }

    /// Returns the track being previewed, when any.
    pub fn track_id(&self) -> Option<&str> {
        match self {
            Self::Idle => None,
            Self::Starting { track_id, .. } | Self::Playing { track_id } => Some(track_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_idle() {
        assert_eq!(PreviewState::default(), PreviewState::Idle);
    }

    #[test]
    fn track_id_returns_id_for_starting_and_playing() {
        assert_eq!(PreviewState::Idle.track_id(), None);

        let starting = PreviewState::Starting {
            track_id: "trk-start".to_string(),
            title: "Title One".to_string(),
        };
        assert_eq!(starting.track_id(), Some("trk-start"));

        let playing = PreviewState::Playing { track_id: "trk-play".to_string() };
        assert_eq!(playing.track_id(), Some("trk-play"));
    }

    #[test]
    fn is_playing_is_true_only_for_playing_variant() {
        assert!(!PreviewState::Idle.is_playing());

        let starting = PreviewState::Starting {
            track_id: "trk-start".to_string(),
            title: "Title One".to_string(),
        };
        assert!(!starting.is_playing());

        let playing = PreviewState::Playing { track_id: "trk-play".to_string() };
        assert!(playing.is_playing());
    }

    #[test]
    fn preview_duration_constant() {
        assert_eq!(PREVIEW_SECS, 20);
    }
}
