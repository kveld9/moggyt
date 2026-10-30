//! Adapters: catalog and audio ports plus deterministic stubs.
//!
//! Ports are the only seam the UI and domain see. Real integrations
//! (catalog API, rodio/gstreamer) implement these traits later
//! without touching `domain` or `app`.

mod audio;
mod audio_dsp;
mod audio_rodio;
mod catalog;
mod catalog_client;
pub mod catalog_client_impl;
mod catalog_live;
pub mod catalog_model;
pub mod catalog_parse_common;
pub mod catalog_parse_detail;
pub mod catalog_parse_explore;
pub mod catalog_parse_home;
pub mod catalog_parse_library;
pub mod catalog_parse_radio;
pub mod catalog_parse_search;
pub mod catalog_requests;
mod catalog_stub;
mod cover;
mod cover_art;
mod cover_disk;
mod handoff;
mod instance;
mod library_backup;
mod lyric_overrides;
mod lyrics;
mod lyrics_lrclib;
mod meta_store;
mod presence;
mod presence_desktop;
mod presence_discord;
mod queue_store;
mod scrobble;
mod scrobble_credentials;
mod scrobble_lastfm;
mod scrobble_stub;
mod session;
mod session_keyring;
mod settings;
mod skip_store;
mod stream;
mod stream_reader;
mod stream_reqwest;
mod stream_stub;
mod tray;
mod updater;
mod updater_github;
mod ytdlp;

pub use audio::{AudioBackend, AudioStub, clamp_speed};
pub use audio_dsp::{
    DEFAULT_MIN_TRAILING_SILENCE_SECS, DEFAULT_SILENCE_THRESHOLD_AMPLITUDE, LoudnessNormalizer,
    SkipSilence, TARGET_LUFS_DB, calculate_rms, compute_target_gain, db_to_linear, is_silent_frame,
    is_silent_sample, linear_to_db, rms_to_db,
};
pub use audio_rodio::{LiveAudio, RodioBackend, volume_to_amplitude};
pub use catalog::Catalog;
pub use catalog_client::{CatalogClient, RawTrack, search_body, search_url};
pub use catalog_live::LiveCatalog;
pub use catalog_stub::CatalogStub;
pub use cover::{CAPACITY, CoverCache, MemoryCoverCache};
pub use cover_art::{COVER_HIRES_MAX_PX, COVER_MAX_PX, decode_cover, fetch_artwork};
pub use cover_disk::{DiskCoverCache, cache_file_name, fnv1a_hex};
pub use handoff::{
    CHECKSUM_HEX_LEN, HANDOFF_PREFIX, MAX_HANDOFF_COOKIES, decode_handoff, encode_handoff,
};
pub use instance::{SingleInstanceGuard, instance_lock_name};
pub use library_backup::{
    BackupSummary, CURRENT_BACKUP_VERSION, LibraryBackup, apply_backup, export_backup,
    read_backup_file, write_backup_file,
};
pub use lyric_overrides::{
    LyricOverrideDir, MAX_OVERRIDE_BYTES, default_lyrics_dir, sanitize_id, sanitize_video_id,
};
pub use lyrics::{LrclibStub, LyricLine, LyricsSource, SyncedLyrics};
pub use lyrics_lrclib::{
    CONFIDENCE_THRESHOLD, DURATION_NEUTRAL_SCORE, DURATION_TOLERANCE_SECS, LrclibClient,
    LyricsCandidate, PARTIAL_MATCH_SCORE, WEIGHT_ARTIST, WEIGHT_DURATION, WEIGHT_TITLE,
    duration_similarity, lrclib_get_url, match_confidence, normalize_text, parse_lrc,
    payload_matches, text_similarity,
};
pub use meta_store::{DiskMetaStore, MemoryMetaStore, MetaStore};
pub use presence::{NoopPresence, Presence};
pub use presence_desktop::{DesktopPresence, MediaKeyAction, action_for, metadata_for, status_for};
pub use presence_discord::{DISCORD_APP_ID, DiscordPresence, activity_parts};
pub use queue_store::{DiskQueueStore, MemoryQueueStore, QueueStore};
pub use scrobble::{
    CredentialStore, KeyringCredentialStore, LastFmClient, LastFmCredentials,
    MemoryCredentialStore, Scrobbler, StubScrobbler, api_signature, client_from_credentials,
    should_scrobble,
};
pub use session::{MemorySessionStore, SessionStore};
pub use session_keyring::{KeyringSessionStore, sanitize_cookie_input};
pub use settings::{
    DiskSettingsStore, MemorySettingsStore, Settings, SettingsStore, default_config_dir,
};
pub use skip_store::{DiskSkipStore, MemorySkipStore, SkipStore};
pub use stream::{AudioQuality, StreamResolver, StreamStub};
pub use tray::{TrayAction, TrayHandle};
pub use updater::{ReleaseInfo, StaticUpdater, Updater};
pub use updater_github::{
    GithubUpdater, UPDATE_OWNER_REPO, is_newer, latest_url, parse_release_payload,
};
pub use ytdlp::{
    YTDLP_AUDIO_FORMAT, YTDLP_UPDATE_INTERVAL_SECS, YtDlpResolver, ensure_sidecar, format_args,
    parse_version, release_asset_name, sidecar_needs_update, watch_url,
};
