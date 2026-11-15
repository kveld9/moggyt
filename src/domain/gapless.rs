//! Sample-continuous (gapless) hand-off state: no I/O, no clocks.

use super::playback_phase::PlaybackPhase;

/// Verge window, in seconds, before a track end in which the next track is
/// decoded and appended behind the current one.
pub const GAPLESS_ARM_SECS: u64 = 3;

/// Lifecycle of the next-track slot appended behind the active track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GaplessState {
    /// No track is appended behind the active one.
    #[default]
    Idle,
    /// The next track is decoded and queued; the active track is still playing.
    Armed,
}

/// Facts that drive [`GaplessState`] transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaplessFact {
    /// The next track was decoded and appended behind the active one.
    Arm,
    /// The slot was consumed, abandoned, or replaced by an explicit play request.
    Reset,
}

impl GaplessState {
    /// Advances the slot state for an observed fact.
    pub fn next(self, fact: GaplessFact) -> Self {
        match fact {
            GaplessFact::Arm => Self::Armed,
            GaplessFact::Reset => Self::Idle,
        }
    }
}

/// Track that crossed the sample boundary and became audible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedGapless {
    pub track_id: String,
    pub duration_secs: Option<u64>,
}

/// Next-track slot appended behind the active track.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GaplessSlot {
    state: GaplessState,
    track_id: Option<String>,
    duration_secs: Option<u64>,
}

impl GaplessSlot {
    /// Active slot state.
    pub fn state(&self) -> GaplessState {
        self.state
    }

    /// Arms the slot with the decoded next track.
    pub fn arm(&mut self, track_id: String, duration_secs: Option<u64>) {
        self.state = self.state.next(GaplessFact::Arm);
        self.track_id = Some(track_id);
        self.duration_secs = duration_secs;
    }

    /// Observes the live sink depth and returns the armed track once it became audible.
    ///
    /// `queued_sounds` is the number of sources still queued in the sink: two while the
    /// active source plus the armed source are loaded, one once the armed source plays.
    pub fn observe(&mut self, queued_sounds: usize) -> Option<StartedGapless> {
        if self.state != GaplessState::Armed || queued_sounds > 1 {
            return None;
        }
        let track_id = self.track_id.take()?;
        let duration_secs = self.duration_secs.take();
        self.state = self.state.next(GaplessFact::Reset);
        Some(StartedGapless { track_id, duration_secs })
    }

    /// Clears the slot without reporting a started track.
    pub fn reset(&mut self) {
        self.state = self.state.next(GaplessFact::Reset);
        self.track_id = None;
        self.duration_secs = None;
    }
}

/// Returns whether the next track should be armed behind the active one.
///
/// Gapless arming runs only for a playing track with a known remaining time, with a
/// next queue track available, and with crossfade disabled.
pub fn should_arm(
    phase: PlaybackPhase,
    remaining_secs: Option<u64>,
    crossfade_seconds: u32,
    has_next: bool,
) -> bool {
    phase == PlaybackPhase::Playing
        && crossfade_seconds == 0
        && has_next
        && remaining_secs.is_some_and(|remaining| remaining <= GAPLESS_ARM_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_arms_and_reports_started_once_at_the_boundary() {
        let mut slot = GaplessSlot::default();
        assert_eq!(slot.state(), GaplessState::Idle);
        assert_eq!(slot.observe(2), None);

        slot.arm("next".to_string(), Some(200));
        assert_eq!(slot.state(), GaplessState::Armed);
        assert_eq!(slot.observe(2), None);
        assert_eq!(slot.state(), GaplessState::Armed);

        let started = slot.observe(1).expect("armed track crossed the boundary");
        assert_eq!(started.track_id, "next");
        assert_eq!(started.duration_secs, Some(200));
        assert_eq!(slot.state(), GaplessState::Idle);
        assert_eq!(slot.observe(1), None, "the hand-off reports exactly once");
    }

    #[test]
    fn slot_reset_clears_arm_without_reporting() {
        let mut slot = GaplessSlot::default();
        slot.arm("next".to_string(), None);
        slot.reset();
        assert_eq!(slot.state(), GaplessState::Idle);
        assert_eq!(slot.observe(1), None);
    }

    #[test]
    fn state_table_matches_specification() {
        for state in [GaplessState::Idle, GaplessState::Armed] {
            assert_eq!(state.next(GaplessFact::Arm), GaplessState::Armed);
            assert_eq!(state.next(GaplessFact::Reset), GaplessState::Idle);
        }
    }

    #[test]
    fn should_arm_table() {
        assert!(should_arm(PlaybackPhase::Playing, Some(3), 0, true));
        assert!(should_arm(PlaybackPhase::Playing, Some(0), 0, true));
        assert!(!should_arm(PlaybackPhase::Playing, Some(4), 0, true));
        assert!(!should_arm(PlaybackPhase::Playing, None, 0, true));
        assert!(!should_arm(PlaybackPhase::Playing, Some(2), 5, true));
        assert!(!should_arm(PlaybackPhase::Playing, Some(2), 0, false));
        assert!(!should_arm(PlaybackPhase::Paused, Some(2), 0, true));
        assert!(!should_arm(PlaybackPhase::Ended, Some(2), 0, true));
        assert!(!should_arm(PlaybackPhase::Resolving, Some(2), 0, true));
    }
}
