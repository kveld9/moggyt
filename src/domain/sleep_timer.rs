//! Pure sleep timer domain models: presets, countdowns, and track-change triggers.
//!
//! Invariant: this module performs no I/O and reads no clocks. All elapsed durations
//! and track identifiers enter as injected parameters.

/// Standard sleep timer presets including interval countdowns and stop-after-track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimerPreset {
    /// Timer is disarmed.
    #[default]
    Off,
    /// Stop playback after 15 minutes.
    Minutes15,
    /// Stop playback after 30 minutes.
    Minutes30,
    /// Stop playback after 45 minutes.
    Minutes45,
    /// Stop playback after 60 minutes.
    Minutes60,
    /// Stop playback when the currently playing track finishes or changes.
    StopAfterTrack,
}

impl TimerPreset {
    /// Duration in seconds for timed interval presets, or None for Off / StopAfterTrack.
    pub fn duration_secs(&self) -> Option<u64> {
        match self {
            Self::Off | Self::StopAfterTrack => None,
            Self::Minutes15 => Some(15 * 60),
            Self::Minutes30 => Some(30 * 60),
            Self::Minutes45 => Some(45 * 60),
            Self::Minutes60 => Some(60 * 60),
        }
    }

    /// User-facing short label for the preset.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Minutes15 => "15m",
            Self::Minutes30 => "30m",
            Self::Minutes45 => "45m",
            Self::Minutes60 => "60m",
            Self::StopAfterTrack => "After track",
        }
    }

    /// User-facing display text including timer prefix.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Off => "Timer: Off",
            Self::Minutes15 => "Timer: 15m",
            Self::Minutes30 => "Timer: 30m",
            Self::Minutes45 => "Timer: 45m",
            Self::Minutes60 => "Timer: 60m",
            Self::StopAfterTrack => "Timer: After track",
        }
    }
}

/// Pure sleep timer state: tracks presets, countdown expiry, and track change triggers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SleepTimer {
    preset: TimerPreset,
    elapsed_secs: u64,
    fired: bool,
    last_track_id: Option<String>,
}

impl SleepTimer {
    /// Creates a disarmed sleep timer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Currently active preset.
    pub fn preset(&self) -> TimerPreset {
        self.preset
    }

    /// Returns true if the timer is armed and waiting to fire.
    pub fn is_armed(&self) -> bool {
        self.preset != TimerPreset::Off && !self.fired
    }

    /// Returns true if the timer has fired.
    pub fn is_fired(&self) -> bool {
        self.fired
    }

    /// Configures the active preset and resets timer progress.
    pub fn set_preset(&mut self, preset: TimerPreset) {
        self.set_preset_with_track(preset, None);
    }

    /// Configures the active preset with an optional initial track identifier.
    pub fn set_preset_with_track(&mut self, preset: TimerPreset, current_track_id: Option<&str>) {
        self.preset = preset;
        self.elapsed_secs = 0;
        self.fired = false;
        self.last_track_id = current_track_id.map(str::to_owned);
    }

    /// Disarms the timer and resets all internal state.
    pub fn cancel(&mut self) {
        self.preset = TimerPreset::Off;
        self.elapsed_secs = 0;
        self.fired = false;
        self.last_track_id = None;
    }

    /// Remaining seconds until countdown expiry, or None if disarmed or not a timed preset.
    pub fn remaining_secs(&self) -> Option<u64> {
        let target = self.preset.duration_secs()?;
        if self.fired {
            return Some(0);
        }
        Some(target.saturating_sub(self.elapsed_secs))
    }

    /// Updates elapsed playtime and returns true exactly once when the timer reaches expiry.
    pub fn poll_elapsed(&mut self, elapsed_secs: u64) -> bool {
        let Some(target) = self.preset.duration_secs() else {
            return false;
        };
        self.elapsed_secs = elapsed_secs;
        if self.fired {
            return false;
        }
        if self.elapsed_secs >= target {
            self.fired = true;
            return true;
        }
        false
    }

    /// Notifies the timer of a track identifier change.
    ///
    /// For StopAfterTrack, fires true exactly once when the track identifier changes from
    /// the initial observed track. Repeated identical track identifiers do not fire.
    pub fn notify_track_changed(&mut self, track_id: &str) -> bool {
        if self.preset != TimerPreset::StopAfterTrack || self.fired {
            return false;
        }
        let Some(ref current) = self.last_track_id else {
            self.last_track_id = Some(track_id.to_owned());
            return false;
        };
        if current == track_id {
            return false;
        }
        self.last_track_id = Some(track_id.to_owned());
        self.fired = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_table_durations_and_labels() {
        let cases = [
            (TimerPreset::Off, None, "Off", "Timer: Off"),
            (TimerPreset::Minutes15, Some(900), "15m", "Timer: 15m"),
            (TimerPreset::Minutes30, Some(1800), "30m", "Timer: 30m"),
            (TimerPreset::Minutes45, Some(2700), "45m", "Timer: 45m"),
            (TimerPreset::Minutes60, Some(3600), "60m", "Timer: 60m"),
            (TimerPreset::StopAfterTrack, None, "After track", "Timer: After track"),
        ];
        for (preset, expected_dur, expected_lbl, expected_display) in cases {
            assert_eq!(preset.duration_secs(), expected_dur);
            assert_eq!(preset.label(), expected_lbl);
            assert_eq!(preset.display_name(), expected_display);
        }
    }

    #[test]
    fn poll_elapsed_fires_once_at_expiry_and_stays_silent_after() {
        let mut timer = SleepTimer::new();
        timer.set_preset(TimerPreset::Minutes15);
        assert!(timer.is_armed());
        assert!(!timer.is_fired());
        assert_eq!(timer.remaining_secs(), Some(900));

        assert!(!timer.poll_elapsed(0));
        assert_eq!(timer.remaining_secs(), Some(900));

        assert!(!timer.poll_elapsed(450));
        assert_eq!(timer.remaining_secs(), Some(450));

        assert!(!timer.poll_elapsed(899));
        assert_eq!(timer.remaining_secs(), Some(1));
        assert!(timer.is_armed());

        // Fires exactly at expiry
        assert!(timer.poll_elapsed(900));
        assert!(!timer.is_armed());
        assert!(timer.is_fired());
        assert_eq!(timer.remaining_secs(), Some(0));

        // Stays silent after expiry
        assert!(!timer.poll_elapsed(901));
        assert!(!timer.poll_elapsed(1800));
        assert_eq!(timer.remaining_secs(), Some(0));
    }

    #[test]
    fn cancel_disarms_timer() {
        let mut timer = SleepTimer::new();
        timer.set_preset(TimerPreset::Minutes30);
        assert!(timer.is_armed());
        assert_eq!(timer.remaining_secs(), Some(1800));

        timer.cancel();
        assert!(!timer.is_armed());
        assert_eq!(timer.preset(), TimerPreset::Off);
        assert_eq!(timer.remaining_secs(), None);
        assert!(!timer.poll_elapsed(1800));
    }

    #[test]
    fn stop_after_track_fires_only_on_track_id_change() {
        let mut timer = SleepTimer::new();
        timer.set_preset(TimerPreset::StopAfterTrack);
        assert!(timer.is_armed());
        assert_eq!(timer.remaining_secs(), None);

        // First notification registers initial playing track, does not fire
        assert!(!timer.notify_track_changed("track-1"));
        assert!(timer.is_armed());

        // Repeated same track id does not fire
        assert!(!timer.notify_track_changed("track-1"));
        assert!(timer.is_armed());

        // Change to different track id fires
        assert!(timer.notify_track_changed("track-2"));
        assert!(!timer.is_armed());
        assert!(timer.is_fired());

        // Repeated calls stay silent after firing
        assert!(!timer.notify_track_changed("track-2"));
        assert!(!timer.notify_track_changed("track-3"));
    }

    #[test]
    fn stop_after_track_with_initial_track_fires_on_change() {
        let mut timer = SleepTimer::new();
        timer.set_preset_with_track(TimerPreset::StopAfterTrack, Some("initial-id"));
        assert!(timer.is_armed());

        // Same track does not fire
        assert!(!timer.notify_track_changed("initial-id"));

        // Different track fires
        assert!(timer.notify_track_changed("next-id"));
        assert!(!timer.is_armed());
        assert!(timer.is_fired());
    }

    #[test]
    fn stop_after_track_cancel_disarms() {
        let mut timer = SleepTimer::new();
        timer.set_preset(TimerPreset::StopAfterTrack);
        assert!(!timer.notify_track_changed("t1"));

        timer.cancel();
        assert!(!timer.is_armed());
        assert_eq!(timer.preset(), TimerPreset::Off);
        assert!(!timer.notify_track_changed("t2"));
    }
}
