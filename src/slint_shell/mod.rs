//! Slint UI shell foundation: page navigation state and window bindings.
//!
//! Hexagonal architecture invariant: this module operates purely in-memory
//! with zero I/O, zero network, zero audio, and zero async runtimes.
//!
//! Page wiring points:
//! - SlintPage::Search:
//!     - crate::domain::Shelf (catalog home shelves and carousels)
//!     - crate::domain::Track (search query result items)
//!     - crate::domain::SearchFilter (catalog filter categories)
//! - SlintPage::NowPlaying:
//!     - crate::domain::Track (active track metadata, duration, cover art URL)
//!     - crate::domain::SkipRanges (auto-skip sponsor and silence segments)
//! - SlintPage::Library:
//!     - crate::domain::Playlist (saved user playlists)
//!     - crate::domain::Album (saved albums)
//!     - crate::domain::Artist (followed artists)
//! - SlintPage::Detail:
//!     - crate::domain::Detail (container model for album, artist, or playlist)
//!     - crate::domain::DetailKind (Album, Artist, Playlist discriminator)
//! - SlintPage::Settings:
//!     - crate::domain::ThemeId (active UI palette)
//!     - crate::domain::PlayerBarMode (player bar presentation mode)
//! - SlintPage::Explore:
//!     - crate::domain::Shelf (explore category carousels and genre items)
//! - SlintPage::Stats:
//!     - crate::domain::ListeningStats (summary totals and top artist/track rankings)

#[allow(clippy::cognitive_complexity)]
mod generated {
    slint::include_modules!();
}
pub use generated::*;

pub use MoggytWindow as AppWindow;

pub mod app;
pub mod audio_snapshot;
mod auto_skip;
pub mod callbacks;
pub mod category;
pub mod context_menu;
pub mod cover_pool;
pub mod covers;
pub mod detail;
mod detail_callbacks;
mod detail_sections;
pub mod discord;
pub mod explore;
pub mod format;
mod history;
pub mod import_callbacks;
mod import_cli;
pub mod import_open;
mod import_override;
pub mod import_searcher;
pub mod import_session;
pub mod import_state;
pub mod import_worker;
pub mod init;
pub mod labels;
pub mod lastfm_connect;
pub mod likes;
pub mod local_edit;
pub mod navigate;
mod output_device;
pub mod page;
pub mod platform;
pub mod play_log;
pub mod playback;
pub mod playback_control;
pub mod playback_failure;
#[cfg(test)]
#[path = "playback_phase_trace_tests.rs"]
mod playback_phase_trace_tests;
pub mod playback_request;
pub mod playback_worker;
pub mod player_bar;
mod scrobble_control;
pub mod search;
pub mod search_submit;
pub mod search_top;
mod settings_callbacks;
pub mod shortcuts;
mod sleep_fade;
pub mod spotify_connect;
pub mod state;
#[cfg(test)]
#[path = "state_artist_tests.rs"]
mod state_artist_tests;
pub mod state_init;
pub mod state_lyrics;
pub mod state_search;
pub mod state_session;
pub mod stats;
pub mod stream_prefetch;
pub mod sync;
#[cfg(test)]
pub mod test_support;
pub mod theme;
pub mod toast;
pub mod version_fix;
pub mod video_policy;
pub mod view_phase;
pub mod window;
pub mod worker_catalog;

pub use app::{
    SlintAppState, StaticLabels, build_window, format_queue_label, run, sync_static_labels,
};
pub use detail::{detail_has_artwork, detail_kind_label, resolve_detail_cover_url};
pub use import_callbacks::{
    current_store, install_import_callbacks, install_playlist_store, poll_import, with_session,
};
pub use import_cli::{
    ImportCli, ImportReport, ImportSourceArg, ReportTrack, build_report, parse_import_args,
    render_progress, report_to_json, run_headless,
};
pub use import_searcher::{
    CandidateHit, LiveTrackSearcher, MAX_ALTERNATIVES, MIN_ACCEPT_SCORE, best_candidate,
    ranked_alternatives, search_query,
};
pub use import_session::{DrainOutcome, ImportSession, ImportStage, progress_status};
pub use import_state::{
    ImportCounts, ImportReview, ImportRow, alternatives_to_slint, confidence_label_key,
    counts_text, local_detail, merge_local_library, row_state_key, rows_to_slint,
};
pub use import_worker::{
    ImportProgressEvent, ImportStats, TrackResolution, TrackSearcher, WorkerConfig,
    assemble_playlist, resolve_tracks, spawn_import,
};
pub use page::SlintPage;
pub use platform::{CoverArtData, PresenceSnapshot, dispatch_to_ui, rgba_to_image};
pub use playback::{format_position, format_track_duration};
pub use playback_worker::{
    PlayTrackRequest, PlayTrackResult, PlaybackWorkerAdapters, PlaybackWorkerCommand,
    execute_detail, execute_play_track, execute_refresh_library, execute_search,
};
pub use player_bar::{PLAYER_BAR_OPTION_COUNT, PlayerBarPrefs, mode_from_index, mode_to_index};
pub use search::format_duration;
pub use stream_prefetch::spawn_stream_prefetch;
pub use theme::{ThemeColors, apply_theme, palette_for, rgb_to_slint};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slint_page_from_index_transitions() {
        assert_eq!(SlintPage::from_index(0), SlintPage::Search);
        assert_eq!(SlintPage::from_index(1), SlintPage::NowPlaying);
        assert_eq!(SlintPage::from_index(2), SlintPage::Library);
        assert_eq!(SlintPage::from_index(3), SlintPage::Detail);
        assert_eq!(SlintPage::from_index(4), SlintPage::Settings);
        assert_eq!(SlintPage::from_index(5), SlintPage::Explore);
        assert_eq!(SlintPage::from_index(6), SlintPage::Stats);
    }

    #[test]
    fn test_slint_page_from_index_out_of_bounds_fallback() {
        assert_eq!(SlintPage::from_index(-1), SlintPage::Search);
        assert_eq!(SlintPage::from_index(7), SlintPage::Category);
        assert_eq!(SlintPage::from_index(8), SlintPage::Search);
        assert_eq!(SlintPage::from_index(100), SlintPage::Search);
        assert_eq!(SlintPage::from_index(i32::MIN), SlintPage::Search);
        assert_eq!(SlintPage::from_index(i32::MAX), SlintPage::Search);
    }

    #[test]
    fn test_slint_page_to_index_mapping() {
        assert_eq!(SlintPage::Search.to_index(), 0);
        assert_eq!(SlintPage::NowPlaying.to_index(), 1);
        assert_eq!(SlintPage::Library.to_index(), 2);
        assert_eq!(SlintPage::Detail.to_index(), 3);
        assert_eq!(SlintPage::Settings.to_index(), 4);
        assert_eq!(SlintPage::Explore.to_index(), 5);
        assert_eq!(SlintPage::Stats.to_index(), 6);
    }

    #[test]
    fn test_slint_page_title_mapping() {
        assert_eq!(SlintPage::Search.title(), "Search");
        assert_eq!(SlintPage::NowPlaying.title(), "Now Playing");
        assert_eq!(SlintPage::Library.title(), "Library");
        assert_eq!(SlintPage::Detail.title(), "Detail");
        assert_eq!(SlintPage::Settings.title(), "Settings");
        assert_eq!(SlintPage::Explore.title(), "Explore");
        assert_eq!(SlintPage::Stats.title(), "Stats");

        assert_eq!(SlintPage::Search.title_with_locale(crate::i18n::Locale::Spanish), "Buscar");
        assert_eq!(
            SlintPage::NowPlaying.title_with_locale(crate::i18n::Locale::Spanish),
            "Sonando"
        );
        assert_eq!(
            SlintPage::Library.title_with_locale(crate::i18n::Locale::Spanish),
            "Biblioteca"
        );
        assert_eq!(SlintPage::Detail.title_with_locale(crate::i18n::Locale::Spanish), "Detalle");
        assert_eq!(SlintPage::Settings.title_with_locale(crate::i18n::Locale::Spanish), "Ajustes");
        assert_eq!(SlintPage::Explore.title_with_locale(crate::i18n::Locale::Spanish), "Explorar");
        assert_eq!(
            SlintPage::Stats.title_with_locale(crate::i18n::Locale::Spanish),
            "Estadísticas"
        );
    }
}
