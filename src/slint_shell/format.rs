//! Pure formatting, time, and cycle helpers for UI state and chrome labels.
//!
//! Hexagonal invariant: pure in-memory functions with zero I/O and zero async runtimes.

use crate::domain::TimerPreset;
use crate::i18n::{Locale, tr};

/// Returns current UTC timestamp in seconds since UNIX epoch, or 0 on clock failure.
pub fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Standard playback speed presets for the settings cycle button.
pub const PLAYBACK_SPEED_PRESETS: [f32; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];

/// Cycles to the next playback speed preset in the sequence:
/// 0.5 -> 0.75 -> 1.0 -> 1.25 -> 1.5 -> 2.0 -> 0.5.
pub fn cycle_playback_speed_preset(current: f32) -> f32 {
    let presets = PLAYBACK_SPEED_PRESETS;
    for (i, &preset) in presets.iter().enumerate() {
        if (current - preset).abs() < 0.05 {
            return presets[(i + 1) % presets.len()];
        }
    }
    1.0
}

/// Formats playback speed factor for UI display (e.g., "1.00x", "1.25x").
pub fn format_playback_speed(speed: f32) -> String {
    format!("{:.2}x", speed)
}

/// Formats update status string for the About settings section.
pub fn format_update_status(
    locale: Locale,
    has_update: bool,
    latest_version: Option<&str>,
) -> String {
    if !has_update {
        return tr(locale, "Up to date").to_string();
    }
    match latest_version {
        Some(ver) => tr(locale, "Update available: {ver}").replace("{ver}", ver),
        None => tr(locale, "Update available").to_string(),
    }
}

/// Formats queue count button label for the player bar (e.g., "Queue", "Queue (3)").
pub fn format_queue_label(locale: Locale, count: usize) -> String {
    if count == 0 {
        tr(locale, "Queue").to_string()
    } else {
        tr(locale, "Queue ({count})").replace("{count}", &count.to_string())
    }
}

/// Maps an integer index (0..=5) from Slint UI selection to standard `TimerPreset`.
pub fn timer_preset_from_index(idx: i32) -> TimerPreset {
    match idx {
        1 => TimerPreset::Minutes15,
        2 => TimerPreset::Minutes30,
        3 => TimerPreset::Minutes45,
        4 => TimerPreset::Minutes60,
        5 => TimerPreset::StopAfterTrack,
        _ => TimerPreset::Off,
    }
}

/// Maps a `TimerPreset` to its corresponding Slint UI integer index (0..=5).
pub fn timer_preset_to_index(preset: TimerPreset) -> i32 {
    match preset {
        TimerPreset::Off => 0,
        TimerPreset::Minutes15 => 1,
        TimerPreset::Minutes30 => 2,
        TimerPreset::Minutes45 => 3,
        TimerPreset::Minutes60 => 4,
        TimerPreset::StopAfterTrack => 5,
    }
}

/// Formats the sleep timer status line for display in UI settings.
pub fn format_sleep_timer_status(
    locale: Locale,
    preset: TimerPreset,
    remaining_secs: Option<u64>,
) -> String {
    match preset {
        TimerPreset::Off => tr(locale, "Timer: Off").to_string(),
        TimerPreset::StopAfterTrack => tr(locale, "Timer: End of track").to_string(),
        TimerPreset::Minutes15
        | TimerPreset::Minutes30
        | TimerPreset::Minutes45
        | TimerPreset::Minutes60 => {
            let secs = remaining_secs.unwrap_or(0);
            let mins = secs / 60;
            let rem_s = secs % 60;
            let time_str = format!("{mins}:{rem_s:02}");
            tr(locale, "Timer: {time}").replace("{time}", &time_str)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cycle_playback_speed_preset_sequence() {
        assert_eq!(cycle_playback_speed_preset(0.5), 0.75);
        assert_eq!(cycle_playback_speed_preset(0.75), 1.0);
        assert_eq!(cycle_playback_speed_preset(1.0), 1.25);
        assert_eq!(cycle_playback_speed_preset(1.25), 1.5);
        assert_eq!(cycle_playback_speed_preset(1.5), 2.0);
        assert_eq!(cycle_playback_speed_preset(2.0), 0.5);
        assert_eq!(cycle_playback_speed_preset(3.5), 1.0);
    }

    #[test]
    fn test_format_playback_speed() {
        assert_eq!(format_playback_speed(1.0), "1.00x");
        assert_eq!(format_playback_speed(0.75), "0.75x");
        assert_eq!(format_playback_speed(1.25), "1.25x");
        assert_eq!(format_playback_speed(2.0), "2.00x");
    }

    #[test]
    fn test_format_update_status() {
        assert_eq!(format_update_status(Locale::English, false, None), "Up to date");
        assert_eq!(format_update_status(Locale::English, false, Some("v2.0.0")), "Up to date");
        assert_eq!(
            format_update_status(Locale::English, true, Some("v2.0.0")),
            "Update available: v2.0.0"
        );
        assert_eq!(format_update_status(Locale::English, true, None), "Update available");

        assert_eq!(format_update_status(Locale::Spanish, false, None), "Actualizado");
        assert_eq!(format_update_status(Locale::Spanish, false, Some("v2.0.0")), "Actualizado");
        assert_eq!(
            format_update_status(Locale::Spanish, true, Some("v2.0.0")),
            "Actualización disponible: v2.0.0"
        );
        assert_eq!(format_update_status(Locale::Spanish, true, None), "Actualización disponible");
    }

    #[test]
    fn test_format_queue_label_english_and_spanish() {
        assert_eq!(format_queue_label(Locale::English, 0), "Queue");
        assert_eq!(format_queue_label(Locale::English, 1), "Queue (1)");
        assert_eq!(format_queue_label(Locale::English, 42), "Queue (42)");

        assert_eq!(format_queue_label(Locale::Spanish, 0), "Cola");
        assert_eq!(format_queue_label(Locale::Spanish, 1), "Cola (1)");
        assert_eq!(format_queue_label(Locale::Spanish, 42), "Cola (42)");
    }

    #[test]
    fn test_timer_preset_index_mapping_all_six_values() {
        let cases = [
            (0, TimerPreset::Off),
            (1, TimerPreset::Minutes15),
            (2, TimerPreset::Minutes30),
            (3, TimerPreset::Minutes45),
            (4, TimerPreset::Minutes60),
            (5, TimerPreset::StopAfterTrack),
        ];
        for (idx, preset) in cases {
            assert_eq!(timer_preset_from_index(idx), preset);
            assert_eq!(timer_preset_to_index(preset), idx);
        }
        assert_eq!(timer_preset_from_index(-1), TimerPreset::Off);
        assert_eq!(timer_preset_from_index(6), TimerPreset::Off);
    }

    #[test]
    fn test_format_sleep_timer_status() {
        // Non-zero remaining time
        assert_eq!(
            format_sleep_timer_status(Locale::English, TimerPreset::Minutes30, Some(754)),
            "Timer: 12:34"
        );
        assert_eq!(
            format_sleep_timer_status(Locale::Spanish, TimerPreset::Minutes30, Some(754)),
            "Temporizador: 12:34"
        );

        // Off
        assert_eq!(
            format_sleep_timer_status(Locale::English, TimerPreset::Off, None),
            "Timer: Off"
        );
        assert_eq!(
            format_sleep_timer_status(Locale::Spanish, TimerPreset::Off, None),
            "Temporizador: Desactivado"
        );

        // End of track
        assert_eq!(
            format_sleep_timer_status(Locale::English, TimerPreset::StopAfterTrack, None),
            "Timer: End of track"
        );
        assert_eq!(
            format_sleep_timer_status(Locale::Spanish, TimerPreset::StopAfterTrack, None),
            "Temporizador: Fin de la canción"
        );
    }
}
