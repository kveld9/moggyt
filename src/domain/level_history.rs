//! Rolling history of audible levels for the Now Playing meter.
//!
//! Invariant: no I/O and no clock; callers push one sample per UI tick.

use std::collections::VecDeque;

/// Number of samples kept: 60 ticks at 500 ms each is 30 seconds of history.
pub const LEVEL_HISTORY_SAMPLES: usize = 60;

/// Fixed-capacity rolling history of level samples in `0.0..=1.0`, oldest first.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LevelHistory {
    samples: VecDeque<f32>,
}

impl LevelHistory {
    /// Pushes one sample, clamping it into `0.0..=1.0` and dropping the oldest past capacity.
    pub fn push(&mut self, level: f32) {
        if self.samples.len() == LEVEL_HISTORY_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(if level.is_finite() { level.clamp(0.0, 1.0) } else { 0.0 });
    }

    /// Samples ordered oldest first.
    pub fn samples(&self) -> impl Iterator<Item = f32> + '_ {
        self.samples.iter().copied()
    }

    /// Latest sample, or `0.0` when the history is empty.
    pub fn latest(&self) -> f32 {
        self.samples.back().copied().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_history_returns_zero_samples_and_latest_zero() {
        let history = LevelHistory::default();
        assert_eq!(history.samples().count(), 0);
        assert_eq!(history.latest(), 0.0);
    }

    #[test]
    fn pushing_seventy_samples_keeps_sixty_and_drops_oldest() {
        let mut history = LevelHistory::default();
        for i in 0..70 {
            history.push(i as f32 / 100.0);
        }
        assert_eq!(history.samples().count(), LEVEL_HISTORY_SAMPLES);
        let samples: Vec<f32> = history.samples().collect();
        assert_eq!(samples.len(), 60);
        assert!((samples[0] - 0.10).abs() < 1e-5);
        assert!((samples[59] - 0.69).abs() < 1e-5);
        assert_eq!(history.latest(), samples[59]);
    }

    #[test]
    fn values_clamp_to_zero_to_one() {
        let mut history = LevelHistory::default();
        history.push(-0.5);
        assert_eq!(history.latest(), 0.0);
        history.push(1.5);
        assert_eq!(history.latest(), 1.0);
    }

    #[test]
    fn nan_becomes_zero() {
        let mut history = LevelHistory::default();
        history.push(f32::NAN);
        assert_eq!(history.latest(), 0.0);
        history.push(f32::INFINITY);
        assert_eq!(history.latest(), 0.0);
        history.push(f32::NEG_INFINITY);
        assert_eq!(history.latest(), 0.0);
    }

    #[test]
    fn latest_tracks_last_push() {
        let mut history = LevelHistory::default();
        history.push(0.2);
        assert_eq!(history.latest(), 0.2);
        history.push(0.8);
        assert_eq!(history.latest(), 0.8);
    }
}
