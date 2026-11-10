//! System output device observation and playback auto-pause decisions.
//!
//! Hexagonal invariant: the decision logic is pure and hardware free; only
//! `current_output_device_name` touches the platform audio host.

/// Action requested after observing the current output device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceWatchAction {
    /// Nothing to do for this tick.
    Idle,
    /// The device changed and playback is running: pause and report the transition.
    Pause { previous: String, current: String },
}

/// Debounced output device watcher.
#[derive(Debug, Clone, Default)]
pub struct OutputDeviceWatch {
    previous: Option<String>,
    candidate: Option<String>,
}

impl OutputDeviceWatch {
    /// Creates a new uninitialized watcher.
    pub fn new() -> Self {
        Self::default()
    }

    /// The currently confirmed previous output device name, if any.
    pub fn previous(&self) -> Option<&str> {
        self.previous.as_deref()
    }

    /// Advances the watcher state with the newly observed device and returns any requested action.
    pub fn step(&mut self, current: Option<&str>, is_playing: bool) -> DeviceWatchAction {
        let Some(current) = current else {
            self.candidate = None;
            return DeviceWatchAction::Idle;
        };
        let Some(prev) = self.previous.as_deref() else {
            self.previous = Some(current.to_string());
            self.candidate = None;
            return DeviceWatchAction::Idle;
        };
        if prev == current {
            self.candidate = None;
            return DeviceWatchAction::Idle;
        }
        self.step_transition(current, is_playing)
    }

    fn step_transition(&mut self, current: &str, is_playing: bool) -> DeviceWatchAction {
        if self.candidate.as_deref() != Some(current) {
            self.candidate = Some(current.to_string());
            return DeviceWatchAction::Idle;
        }
        self.candidate = None;
        let previous = self.previous.replace(current.to_string()).unwrap_or_default();
        if is_playing {
            DeviceWatchAction::Pause { previous, current: current.to_string() }
        } else {
            DeviceWatchAction::Idle
        }
    }
}

/// Samples the name of the system default audio output device.
///
/// Returns `None` if no default device exists, querying fails, or the device name is blank.
pub fn current_output_device_name() -> Option<String> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let raw = cpal::default_host().default_output_device()?.name().ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_observation_is_ignored() {
        let mut watch = OutputDeviceWatch::new();
        assert_eq!(watch.previous(), None);
        let action = watch.step(Some("Speakers"), true);
        assert_eq!(action, DeviceWatchAction::Idle);
        assert_eq!(watch.previous(), Some("Speakers"));
    }

    #[test]
    fn same_device_keeps_idle() {
        let mut watch = OutputDeviceWatch::new();
        watch.step(Some("Speakers"), true);
        assert_eq!(watch.step(Some("Speakers"), true), DeviceWatchAction::Idle);
        assert_eq!(watch.step(Some("Speakers"), true), DeviceWatchAction::Idle);
        assert_eq!(watch.previous(), Some("Speakers"));
    }

    #[test]
    fn first_tick_of_change_is_pending() {
        let mut watch = OutputDeviceWatch::new();
        watch.step(Some("Speakers"), true);
        let action = watch.step(Some("Headphones"), true);
        assert_eq!(action, DeviceWatchAction::Idle);
        assert_eq!(watch.previous(), Some("Speakers"));
    }

    #[test]
    fn second_consecutive_tick_pauses_when_playing() {
        let mut watch = OutputDeviceWatch::new();
        watch.step(Some("Speakers"), true);
        watch.step(Some("Headphones"), true);
        let action = watch.step(Some("Headphones"), true);
        assert_eq!(
            action,
            DeviceWatchAction::Pause {
                previous: "Speakers".to_string(),
                current: "Headphones".to_string(),
            }
        );
        assert_eq!(watch.previous(), Some("Headphones"));
    }

    #[test]
    fn second_consecutive_tick_without_playback_does_not_pause() {
        let mut watch = OutputDeviceWatch::new();
        watch.step(Some("Speakers"), false);
        watch.step(Some("Headphones"), false);
        let action = watch.step(Some("Headphones"), false);
        assert_eq!(action, DeviceWatchAction::Idle);
        assert_eq!(watch.previous(), Some("Headphones"));
    }

    #[test]
    fn revert_before_confirmation_cancels_pending_change() {
        let mut watch = OutputDeviceWatch::new();
        watch.step(Some("Speakers"), true);
        assert_eq!(watch.step(Some("Headphones"), true), DeviceWatchAction::Idle);
        assert_eq!(watch.step(Some("Speakers"), true), DeviceWatchAction::Idle);
        assert_eq!(watch.previous(), Some("Speakers"));
        assert_eq!(watch.step(Some("Speakers"), true), DeviceWatchAction::Idle);
        assert_eq!(watch.previous(), Some("Speakers"));
    }

    #[test]
    fn none_current_device_is_ignored() {
        let mut watch = OutputDeviceWatch::new();
        watch.step(Some("Speakers"), true);
        watch.step(Some("Headphones"), true);
        assert_eq!(watch.step(None, true), DeviceWatchAction::Idle);
        assert_eq!(watch.previous(), Some("Speakers"));
        assert_eq!(watch.step(Some("Headphones"), true), DeviceWatchAction::Idle);
        assert_eq!(watch.previous(), Some("Speakers"));
    }

    #[test]
    fn blank_device_names_are_not_special_cased() {
        let mut watch = OutputDeviceWatch::new();
        assert_eq!(watch.step(Some(""), true), DeviceWatchAction::Idle);
        assert_eq!(watch.previous(), Some(""));
    }
}
