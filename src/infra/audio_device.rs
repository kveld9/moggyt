//! System output device observation and playback auto-pause decisions.
//!
//! Hexagonal invariant: the decision logic is pure and hardware free; only
//! `current_output_device_name` touches the platform audio host.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

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

/// Parses the default sink name from `pactl get-default-sink` output.
pub fn parse_default_sink(stdout: &str) -> Option<String> {
    stdout.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_string)
}

#[cfg(target_os = "linux")]
fn linux_default_sink() -> Option<String> {
    let output = std::process::Command::new("pactl").arg("get-default-sink").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_default_sink(&stdout)
}

#[cfg(not(target_os = "linux"))]
fn linux_default_sink() -> Option<String> {
    None
}

/// Chooses the identity used for change detection: the audio-server default sink
/// when available, otherwise the cpal device name.
fn select_identity(default_sink: Option<String>, device_name: Option<String>) -> Option<String> {
    default_sink.or(device_name)
}

/// Samples the output device identity used for change detection.
///
/// On Linux the PulseAudio/PipeWire default sink is preferred because cpal
/// reports a stable ALSA alias; other platforms use the cpal device name.
pub fn current_output_device_identity() -> Option<String> {
    select_identity(linux_default_sink(), current_output_device_name())
}

/// Background sampling interval for the output device identity.
pub const DEVICE_MONITOR_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

const MONITOR_STOP_SLICE: std::time::Duration = std::time::Duration::from_millis(50);

/// Background monitor that keeps the latest output device identity.
///
/// Sampling runs on its own thread so the UI thread never spawns a process or
/// queries the audio host. Dropping the monitor stops and joins the worker.
pub struct DeviceIdentityMonitor {
    latest: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl DeviceIdentityMonitor {
    /// Starts the background monitor with the default interval and platform identity sampler.
    pub fn start() -> Self {
        Self::spawn_with(DEVICE_MONITOR_INTERVAL, current_output_device_identity)
    }

    pub(crate) fn spawn_with<F>(interval: std::time::Duration, sampler: F) -> Self
    where
        F: Fn() -> Option<String> + Send + 'static,
    {
        let initial = sampler();
        let latest = Arc::new(Mutex::new(initial));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_latest = Arc::clone(&latest);
        let thread_stop = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("moggyt-device-monitor".to_string())
            .spawn(move || {
                run_monitor_worker(&thread_stop, &thread_latest, interval, sampler);
            })
            .ok();
        Self { latest, stop, handle }
    }

    /// Returns the latest sampled output device identity.
    pub fn latest(&self) -> Option<String> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Drop for DeviceIdentityMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn sleep_until_elapsed(stop: &AtomicBool, interval: Duration) -> bool {
    let mut remaining = interval;
    while !stop.load(Ordering::Relaxed) && remaining > Duration::ZERO {
        let slice = remaining.min(MONITOR_STOP_SLICE);
        std::thread::sleep(slice);
        remaining = remaining.saturating_sub(slice);
    }
    !stop.load(Ordering::Relaxed)
}

fn run_monitor_worker<F>(
    stop: &AtomicBool,
    latest: &Mutex<Option<String>>,
    interval: Duration,
    sampler: F,
) where
    F: Fn() -> Option<String>,
{
    while sleep_until_elapsed(stop, interval) {
        let sample = sampler();
        *latest.lock().unwrap_or_else(PoisonError::into_inner) = sample;
    }
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

    #[test]
    fn parse_default_sink_reads_first_non_blank_line() {
        let stdout = "   \n\n  alsa_output.pci-0000_00_1f.3.analog-stereo  \nother_sink\n";
        assert_eq!(
            parse_default_sink(stdout),
            Some("alsa_output.pci-0000_00_1f.3.analog-stereo".to_string())
        );
    }

    #[test]
    fn parse_default_sink_rejects_blank_output() {
        assert_eq!(parse_default_sink(""), None);
        assert_eq!(parse_default_sink("   \n\t  \n  \n"), None);
    }

    #[test]
    fn select_identity_prefers_default_sink() {
        let sink = Some("alsa_output.pci.stereo".to_string());
        let device = Some("default".to_string());
        assert_eq!(select_identity(sink, device), Some("alsa_output.pci.stereo".to_string()));
    }

    #[test]
    fn select_identity_falls_back_to_device_name() {
        let device = Some("Speakers".to_string());
        assert_eq!(select_identity(None, device), Some("Speakers".to_string()));
    }

    #[test]
    fn select_identity_returns_none_when_both_missing() {
        assert_eq!(select_identity(None, None), None);
    }

    #[test]
    fn monitor_publishes_initial_sample() {
        let monitor = DeviceIdentityMonitor::spawn_with(Duration::from_millis(5), || {
            Some("Speakers".to_string())
        });
        assert_eq!(monitor.latest(), Some("Speakers".to_string()));
        drop(monitor);
    }

    #[test]
    fn monitor_returns_none_when_sampler_has_no_device() {
        let monitor = DeviceIdentityMonitor::spawn_with(Duration::from_millis(5), || None);
        assert_eq!(monitor.latest(), None);
    }

    #[test]
    fn monitor_keeps_last_sample_for_ui_readers() {
        let monitor =
            DeviceIdentityMonitor::spawn_with(Duration::from_millis(5), || Some("A".to_string()));
        assert_eq!(monitor.latest(), Some("A".to_string()));
        assert_eq!(monitor.latest(), Some("A".to_string()));
        assert_eq!(monitor.latest(), Some("A".to_string()));
    }
}
