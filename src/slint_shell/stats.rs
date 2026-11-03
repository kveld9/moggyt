//! Listening statistics view model generation, time formatting, and Slint callback wiring.
//!
//! Hexagonal invariant: pure statistical formatting and view model builders separated
//! from play log persistence and Slint global property dispatch.

use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::domain::{
    ListeningStats, RankedEntry, STATS_TOP_LIMIT, StatsPeriod, compute_listening_stats,
};
use crate::i18n::{Locale, tr};
use crate::slint_shell::state::{SharedState, SlintAppState};
use crate::slint_shell::{MoggytWindow, StatsData, StatsEntryData};

/// In-memory view row model representing a top-ranked artist or track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatsRowModel {
    pub rank: String,
    pub name: String,
    pub detail: String,
    pub plays: String,
}

/// In-memory view model containing all summary and top-ranked statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatsViewModel {
    pub is_empty: bool,
    pub total_plays: String,
    pub total_time: String,
    pub unique_tracks: String,
    pub unique_artists: String,
    pub top_artists: Vec<StatsRowModel>,
    pub top_tracks: Vec<StatsRowModel>,
}

/// Formats cumulative listening seconds into human-readable duration strings.
pub(crate) fn format_listening_time(_locale: Locale, secs: u64) -> String {
    let total_minutes = secs / 60;
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours >= 1 { format!("{hours} h {minutes} min") } else { format!("{minutes} min") }
}

/// Maps ranked domain entries into localized view row models with 1-based ranks.
pub(crate) fn entry_rows(entries: &[RankedEntry], locale: Locale) -> Vec<StatsRowModel> {
    let plays_unit = tr(locale, "plays");
    entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| StatsRowModel {
            rank: (idx + 1).to_string(),
            name: entry.name.clone(),
            detail: entry.detail.clone(),
            plays: format!("{} {plays_unit}", entry.plays),
        })
        .collect()
}

/// Constructs a pure [`StatsViewModel`] from domain [`ListeningStats`].
pub(crate) fn build_stats_view(stats: &ListeningStats, locale: Locale) -> StatsViewModel {
    StatsViewModel {
        is_empty: stats.is_empty(),
        total_plays: stats.total_plays.to_string(),
        total_time: format_listening_time(locale, stats.total_listened_secs),
        unique_tracks: stats.unique_tracks.to_string(),
        unique_artists: stats.unique_artists.to_string(),
        top_artists: entry_rows(&stats.top_artists, locale),
        top_tracks: entry_rows(&stats.top_tracks, locale),
    }
}

fn apply_stats_data(window: &MoggytWindow, period: StatsPeriod, vm: &StatsViewModel) {
    let stats_data = window.global::<StatsData>();
    stats_data.set_period_index(period.to_index());
    stats_data.set_is_empty(vm.is_empty);
    stats_data.set_total_plays(vm.total_plays.clone().into());
    stats_data.set_total_time(vm.total_time.clone().into());
    stats_data.set_unique_tracks(vm.unique_tracks.clone().into());
    stats_data.set_unique_artists(vm.unique_artists.clone().into());

    let top_artists: Vec<StatsEntryData> = vm
        .top_artists
        .iter()
        .map(|row| StatsEntryData {
            rank: row.rank.clone().into(),
            name: row.name.clone().into(),
            detail: row.detail.clone().into(),
            plays: row.plays.clone().into(),
        })
        .collect();
    stats_data.set_top_artists(ModelRc::new(VecModel::from(top_artists)));

    let top_tracks: Vec<StatsEntryData> = vm
        .top_tracks
        .iter()
        .map(|row| StatsEntryData {
            rank: row.rank.clone().into(),
            name: row.name.clone().into(),
            detail: row.detail.clone().into(),
            plays: row.plays.clone().into(),
        })
        .collect();
    stats_data.set_top_tracks(ModelRc::new(VecModel::from(top_tracks)));
}

/// Refreshes listening statistics for the specified period and pushes results to Slint.
// NOTE(clippy): tracing macro expansion inflates cognitive complexity.
#[allow(clippy::cognitive_complexity)]
pub(crate) fn refresh_stats(
    window: &MoggytWindow,
    state: &SlintAppState,
    period: StatsPeriod,
    now_unix: u64,
) {
    let records = match state.play_log_store.load() {
        Ok(records) => records,
        Err(err) => {
            tracing::warn!("failed to load play log for stats calculation: {err:#}");
            Vec::new()
        }
    };
    let cutoff = period.since_unix(now_unix);
    let stats = compute_listening_stats(&records, cutoff, STATS_TOP_LIMIT);
    let vm = build_stats_view(&stats, state.locale);
    apply_stats_data(window, period, &vm);
}

/// Helper that reads the active period from Slint and refreshes stats with current wall clock.
pub(crate) fn refresh_active_stats(window: &MoggytWindow, state: &SlintAppState) {
    let stats_data = window.global::<StatsData>();
    let period = StatsPeriod::from_index(stats_data.get_period_index());
    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    refresh_stats(window, state, period, now_unix);
}

/// Wires the period selection callback from the Slint StatsData global.
pub(crate) fn wire_stats_callbacks(window: &MoggytWindow, state_rc: &SharedState) {
    let stats_data = window.global::<StatsData>();
    let w = window.as_weak();
    let s = Rc::clone(state_rc);
    stats_data.on_period_selected(move |idx| {
        let Some(win) = w.upgrade() else {
            return;
        };
        let period = StatsPeriod::from_index(idx);
        let now_unix =
            SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let st = s.borrow();
        refresh_stats(&win, &st, period, now_unix);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_listening_time_cases() {
        assert_eq!(format_listening_time(Locale::English, 0), "0 min");
        assert_eq!(format_listening_time(Locale::English, 59), "0 min");
        assert_eq!(format_listening_time(Locale::English, 59 * 60), "59 min");
        assert_eq!(format_listening_time(Locale::English, 65 * 60), "1 h 5 min");
        assert_eq!(format_listening_time(Locale::English, 100 * 3600 + 12 * 60), "100 h 12 min");
        assert_eq!(format_listening_time(Locale::Spanish, 100 * 3600 + 12 * 60), "100 h 12 min");
    }

    #[test]
    fn test_entry_rows_formatting_english_and_spanish() {
        let entries = vec![
            RankedEntry {
                name: "Song A".to_string(),
                detail: "Artist 1".to_string(),
                plays: 12,
                listened_secs: 360,
            },
            RankedEntry {
                name: "Song B".to_string(),
                detail: "Artist 2".to_string(),
                plays: 7,
                listened_secs: 210,
            },
        ];

        let en_rows = entry_rows(&entries, Locale::English);
        assert_eq!(en_rows.len(), 2);
        assert_eq!(en_rows[0].rank, "1");
        assert_eq!(en_rows[0].name, "Song A");
        assert_eq!(en_rows[0].detail, "Artist 1");
        assert_eq!(en_rows[0].plays, "12 plays");

        assert_eq!(en_rows[1].rank, "2");
        assert_eq!(en_rows[1].name, "Song B");
        assert_eq!(en_rows[1].detail, "Artist 2");
        assert_eq!(en_rows[1].plays, "7 plays");

        let es_rows = entry_rows(&entries, Locale::Spanish);
        assert_eq!(es_rows[0].plays, "12 reproducciones");
        assert_eq!(es_rows[1].plays, "7 reproducciones");
    }

    #[test]
    fn test_build_stats_view_empty_and_populated() {
        let empty_stats = ListeningStats::default();
        let empty_vm = build_stats_view(&empty_stats, Locale::English);
        assert!(empty_vm.is_empty);
        assert_eq!(empty_vm.total_plays, "0");
        assert_eq!(empty_vm.total_time, "0 min");
        assert_eq!(empty_vm.unique_tracks, "0");
        assert_eq!(empty_vm.unique_artists, "0");
        assert!(empty_vm.top_artists.is_empty());
        assert!(empty_vm.top_tracks.is_empty());

        let populated_stats = ListeningStats {
            total_plays: 42,
            total_listened_secs: 7300,
            unique_tracks: 15,
            unique_artists: 8,
            top_artists: vec![RankedEntry {
                name: "Artist 1".to_string(),
                detail: String::new(),
                plays: 20,
                listened_secs: 3600,
            }],
            top_tracks: vec![RankedEntry {
                name: "Track 1".to_string(),
                detail: "Artist 1".to_string(),
                plays: 10,
                listened_secs: 1800,
            }],
        };

        let vm = build_stats_view(&populated_stats, Locale::English);
        assert!(!vm.is_empty);
        assert_eq!(vm.total_plays, "42");
        assert_eq!(vm.total_time, "2 h 1 min");
        assert_eq!(vm.unique_tracks, "15");
        assert_eq!(vm.unique_artists, "8");
        assert_eq!(vm.top_artists.len(), 1);
        assert_eq!(vm.top_artists[0].name, "Artist 1");
        assert_eq!(vm.top_artists[0].plays, "20 plays");
        assert_eq!(vm.top_tracks.len(), 1);
        assert_eq!(vm.top_tracks[0].name, "Track 1");
        assert_eq!(vm.top_tracks[0].plays, "10 plays");
    }
}
