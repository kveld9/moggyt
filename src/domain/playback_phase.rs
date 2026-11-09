//! Pure playback phase state machine and observable fact mapping with no I/O.

/// Observable lifecycle phase of audio playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackPhase {
    #[default]
    Empty,
    Resolving,
    Playing,
    Paused,
    Ended,
    Failed,
}

/// Domain playback events that trigger phase transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackEvent {
    Resolve,
    Started,
    Pause,
    Resume,
    Ended,
    Failed,
    Reset,
}

/// Tolerance window in seconds where a track counts as ended once
/// `position_secs + END_TOLERANCE_SECS >= duration_secs`.
pub const END_TOLERANCE_SECS: u64 = 1;

const TRANSITIONS: [[PlaybackPhase; 7]; 6] = [
    // Empty: Resolve->Resolving, Reset->Empty, (all others -> Empty)
    [
        PlaybackPhase::Resolving, // Resolve
        PlaybackPhase::Empty,     // Started
        PlaybackPhase::Empty,     // Pause
        PlaybackPhase::Empty,     // Resume
        PlaybackPhase::Empty,     // Ended
        PlaybackPhase::Empty,     // Failed
        PlaybackPhase::Empty,     // Reset
    ],
    // Resolving: Started->Playing, Failed->Failed, Reset->Empty, Resolve->Resolving,
    //            Pause->Resolving, Resume->Resolving, Ended->Resolving
    [
        PlaybackPhase::Resolving, // Resolve
        PlaybackPhase::Playing,   // Started
        PlaybackPhase::Resolving, // Pause
        PlaybackPhase::Resolving, // Resume
        PlaybackPhase::Resolving, // Ended
        PlaybackPhase::Failed,    // Failed
        PlaybackPhase::Empty,     // Reset
    ],
    // Playing: Pause->Paused, Ended->Ended, Failed->Failed, Reset->Empty,
    //          Resolve->Resolving, Started->Playing, Resume->Playing
    [
        PlaybackPhase::Resolving, // Resolve
        PlaybackPhase::Playing,   // Started
        PlaybackPhase::Paused,    // Pause
        PlaybackPhase::Playing,   // Resume
        PlaybackPhase::Ended,     // Ended
        PlaybackPhase::Failed,    // Failed
        PlaybackPhase::Empty,     // Reset
    ],
    // Paused: Resume->Playing, Ended->Ended, Failed->Failed, Reset->Empty,
    //         Resolve->Resolving, Pause->Paused, Started->Paused
    [
        PlaybackPhase::Resolving, // Resolve
        PlaybackPhase::Paused,    // Started
        PlaybackPhase::Paused,    // Pause
        PlaybackPhase::Playing,   // Resume
        PlaybackPhase::Ended,     // Ended
        PlaybackPhase::Failed,    // Failed
        PlaybackPhase::Empty,     // Reset
    ],
    // Ended: Resolve->Resolving, Resume->Resolving (default restart of the current track),
    //        Failed->Failed, Reset->Empty, Pause->Ended, Ended->Ended, Started->Ended
    [
        PlaybackPhase::Resolving, // Resolve
        PlaybackPhase::Ended,     // Started
        PlaybackPhase::Ended,     // Pause
        PlaybackPhase::Resolving, // Resume
        PlaybackPhase::Ended,     // Ended
        PlaybackPhase::Failed,    // Failed
        PlaybackPhase::Empty,     // Reset
    ],
    // Failed: Resolve->Resolving, Reset->Empty, (all others -> Failed)
    [
        PlaybackPhase::Resolving, // Resolve
        PlaybackPhase::Failed,    // Started
        PlaybackPhase::Failed,    // Pause
        PlaybackPhase::Failed,    // Resume
        PlaybackPhase::Failed,    // Ended
        PlaybackPhase::Failed,    // Failed
        PlaybackPhase::Empty,     // Reset
    ],
];

impl PlaybackPhase {
    /// Advances to the next phase given an observable playback event.
    pub fn next(self, event: PlaybackEvent) -> Self {
        TRANSITIONS[self as usize][event as usize]
    }
}

/// Raw observable playback facts sampled from the audio backend and queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackFacts {
    pub is_empty: bool,
    pub is_paused: bool,
    pub is_playing: bool,
    pub position_secs: u64,
    pub duration_secs: u64,
}

impl PlaybackFacts {
    /// Maps raw observable playback facts to a domain playback event.
    ///
    /// Evaluated in priority order:
    /// 1. `is_empty` -> `Some(PlaybackEvent::Reset)`
    /// 2. `is_playing` -> `Some(PlaybackEvent::Started)`
    /// 3. `is_paused` -> `Some(PlaybackEvent::Pause)`
    /// 4. `duration_secs > 0 && position_secs + END_TOLERANCE_SECS >= duration_secs` -> `Some(PlaybackEvent::Ended)`
    /// 5. otherwise `None` (unknown/stalled state must not change the phase).
    ///
    /// An unknown duration (`0`) never reports `Ended`.
    pub fn event(self) -> Option<PlaybackEvent> {
        if self.is_empty {
            return Some(PlaybackEvent::Reset);
        }
        if self.is_playing {
            return Some(PlaybackEvent::Started);
        }
        if self.is_paused {
            return Some(PlaybackEvent::Pause);
        }
        if self.is_ended() {
            return Some(PlaybackEvent::Ended);
        }
        None
    }

    fn is_ended(self) -> bool {
        self.duration_secs > 0 && self.position_secs + END_TOLERANCE_SECS >= self.duration_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_PHASES: [PlaybackPhase; 6] = [
        PlaybackPhase::Empty,
        PlaybackPhase::Resolving,
        PlaybackPhase::Playing,
        PlaybackPhase::Paused,
        PlaybackPhase::Ended,
        PlaybackPhase::Failed,
    ];

    const ALL_EVENTS: [PlaybackEvent; 7] = [
        PlaybackEvent::Resolve,
        PlaybackEvent::Started,
        PlaybackEvent::Pause,
        PlaybackEvent::Resume,
        PlaybackEvent::Ended,
        PlaybackEvent::Failed,
        PlaybackEvent::Reset,
    ];

    #[test]
    fn transition_table_matches_specification() {
        let expected: [[PlaybackPhase; 7]; 6] = [
            // Empty
            [
                PlaybackPhase::Resolving,
                PlaybackPhase::Empty,
                PlaybackPhase::Empty,
                PlaybackPhase::Empty,
                PlaybackPhase::Empty,
                PlaybackPhase::Empty,
                PlaybackPhase::Empty,
            ],
            // Resolving
            [
                PlaybackPhase::Resolving,
                PlaybackPhase::Playing,
                PlaybackPhase::Resolving,
                PlaybackPhase::Resolving,
                PlaybackPhase::Resolving,
                PlaybackPhase::Failed,
                PlaybackPhase::Empty,
            ],
            // Playing
            [
                PlaybackPhase::Resolving,
                PlaybackPhase::Playing,
                PlaybackPhase::Paused,
                PlaybackPhase::Playing,
                PlaybackPhase::Ended,
                PlaybackPhase::Failed,
                PlaybackPhase::Empty,
            ],
            // Paused
            [
                PlaybackPhase::Resolving,
                PlaybackPhase::Paused,
                PlaybackPhase::Paused,
                PlaybackPhase::Playing,
                PlaybackPhase::Ended,
                PlaybackPhase::Failed,
                PlaybackPhase::Empty,
            ],
            // Ended
            [
                PlaybackPhase::Resolving,
                PlaybackPhase::Ended,
                PlaybackPhase::Ended,
                PlaybackPhase::Resolving,
                PlaybackPhase::Ended,
                PlaybackPhase::Failed,
                PlaybackPhase::Empty,
            ],
            // Failed
            [
                PlaybackPhase::Resolving,
                PlaybackPhase::Failed,
                PlaybackPhase::Failed,
                PlaybackPhase::Failed,
                PlaybackPhase::Failed,
                PlaybackPhase::Failed,
                PlaybackPhase::Empty,
            ],
        ];

        for (p_idx, phase) in ALL_PHASES.iter().enumerate() {
            for (e_idx, event) in ALL_EVENTS.iter().enumerate() {
                assert_eq!(
                    phase.next(*event),
                    expected[p_idx][e_idx],
                    "failed transition for phase {phase:?} on event {event:?}"
                );
            }
        }
    }

    #[test]
    fn paused_never_becomes_ended_without_end_event() {
        for event in ALL_EVENTS {
            if event != PlaybackEvent::Ended {
                assert_ne!(
                    PlaybackPhase::Paused.next(event),
                    PlaybackPhase::Ended,
                    "Paused transitioned to Ended on event {event:?}"
                );
            }
        }
    }

    #[test]
    fn facts_empty_is_reset() {
        let facts = PlaybackFacts {
            is_empty: true,
            is_paused: false,
            is_playing: false,
            position_secs: 0,
            duration_secs: 0,
        };
        assert_eq!(facts.event(), Some(PlaybackEvent::Reset));
    }

    #[test]
    fn facts_playing_is_started() {
        let facts = PlaybackFacts {
            is_empty: false,
            is_paused: false,
            is_playing: true,
            position_secs: 10,
            duration_secs: 100,
        };
        assert_eq!(facts.event(), Some(PlaybackEvent::Started));
    }

    #[test]
    fn facts_pause_at_end_stays_paused() {
        let facts = PlaybackFacts {
            is_empty: false,
            is_paused: true,
            is_playing: false,
            position_secs: 199,
            duration_secs: 200,
        };
        assert_eq!(facts.event(), Some(PlaybackEvent::Pause));
    }

    #[test]
    fn facts_ended_at_tolerance() {
        let facts = PlaybackFacts {
            is_empty: false,
            is_paused: false,
            is_playing: false,
            position_secs: 199,
            duration_secs: 200,
        };
        assert_eq!(facts.event(), Some(PlaybackEvent::Ended));
    }

    #[test]
    fn facts_unknown_duration_is_not_ended() {
        let facts = PlaybackFacts {
            is_empty: false,
            is_paused: false,
            is_playing: false,
            position_secs: 5000,
            duration_secs: 0,
        };
        assert_eq!(facts.event(), None);
    }
}
