//! Pure buffering watchdog for the audible playback position.
//!
//! Invariant: no I/O and no clock. Callers sample the position once per UI tick.

/// Consecutive position samples without progress that count as buffering.
pub const BUFFERING_STALL_SAMPLES: u32 = 4;

/// Watchdog over the audible playback position of the active track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BufferingWatch {
    /// The position advanced between samples (or the previous sample is unknown).
    #[default]
    Flowing,
    /// The position stayed at `position_secs` for `stalled_samples` samples.
    Stalled { position_secs: u64, stalled_samples: u32 },
}

impl BufferingWatch {
    /// Samples the current position and advances the watchdog.
    pub fn observe(self, position_secs: u64) -> Self {
        match self {
            Self::Flowing => Self::Stalled { position_secs, stalled_samples: 1 },
            Self::Stalled { position_secs: previous, stalled_samples }
                if previous == position_secs =>
            {
                Self::Stalled { position_secs, stalled_samples: stalled_samples.saturating_add(1) }
            }
            Self::Stalled { .. } => Self::Stalled { position_secs, stalled_samples: 1 },
        }
    }

    /// Resets the watchdog to `Flowing`.
    pub fn reset(self) -> Self {
        Self::Flowing
    }

    /// Returns whether the stall is long enough to be reported as buffering.
    pub fn is_buffering(self) -> bool {
        match self {
            Self::Flowing => false,
            Self::Stalled { stalled_samples, .. } => stalled_samples >= BUFFERING_STALL_SAMPLES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progressing_positions_keep_buffering_false() {
        let mut watch = BufferingWatch::default();
        assert!(!watch.is_buffering());
        for pos in 1..=10 {
            watch = watch.observe(pos);
            assert!(!watch.is_buffering());
        }
    }

    #[test]
    fn test_frozen_position_reaches_buffering_at_stall_threshold() {
        let mut watch = BufferingWatch::Flowing;
        watch = watch.observe(42);
        assert_eq!(watch, BufferingWatch::Stalled { position_secs: 42, stalled_samples: 1 });
        assert!(!watch.is_buffering());

        watch = watch.observe(42);
        assert_eq!(watch, BufferingWatch::Stalled { position_secs: 42, stalled_samples: 2 });
        assert!(!watch.is_buffering());

        watch = watch.observe(42);
        assert_eq!(watch, BufferingWatch::Stalled { position_secs: 42, stalled_samples: 3 });
        assert!(!watch.is_buffering());

        watch = watch.observe(42);
        assert_eq!(watch, BufferingWatch::Stalled { position_secs: 42, stalled_samples: 4 });
        assert!(watch.is_buffering());

        watch = watch.observe(42);
        assert_eq!(watch, BufferingWatch::Stalled { position_secs: 42, stalled_samples: 5 });
        assert!(watch.is_buffering());
    }

    #[test]
    fn test_resumed_position_resets_stall_counter() {
        let mut watch = BufferingWatch::Flowing;
        for _ in 0..BUFFERING_STALL_SAMPLES {
            watch = watch.observe(10);
        }
        assert!(watch.is_buffering());

        watch = watch.observe(11);
        assert_eq!(watch, BufferingWatch::Stalled { position_secs: 11, stalled_samples: 1 });
        assert!(!watch.is_buffering());
    }

    #[test]
    fn test_reset_returns_to_flowing() {
        let mut watch = BufferingWatch::Flowing;
        for _ in 0..BUFFERING_STALL_SAMPLES {
            watch = watch.observe(50);
        }
        assert!(watch.is_buffering());

        watch = watch.reset();
        assert_eq!(watch, BufferingWatch::Flowing);
        assert!(!watch.is_buffering());
        assert_eq!(watch.reset(), BufferingWatch::Flowing);
    }
}
