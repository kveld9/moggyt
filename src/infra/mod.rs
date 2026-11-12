//! Adapters: catalog and audio ports plus deterministic stubs.
//!
//! Ports are the only seam the UI and domain see. Real integrations
//! (catalog API, rodio/gstreamer) implement these traits later
//! without touching `domain` or `app`.

mod atomic_file;
mod audio;
mod audio_device;
mod audio_dsp;
mod audio_fade;
mod audio_live;
pub mod audio_resample;
mod audio_rodio;
mod audio_stream_cache_worker;
mod browser;
mod catalog;
mod catalog_client;
pub mod catalog_client_impl;
mod catalog_live;
pub mod catalog_model;
pub mod catalog_parse_artist;
pub mod catalog_parse_common;
pub mod catalog_parse_detail;
pub mod catalog_parse_explore;
pub mod catalog_parse_home;
pub mod catalog_parse_library;
pub mod catalog_parse_radio;
pub mod catalog_parse_search;
pub mod catalog_requests;
mod catalog_stub;
pub mod clipboard;
mod cover;
mod cover_art;
mod cover_disk;
mod custom_theme;
mod file_picker;
mod handoff;
mod innertube_stream;
mod instance;
mod lastfm_auth;
mod lyric_overrides;
mod lyrics;
mod lyrics_cache;
mod lyrics_lrclib;
mod lyrics_lrcred;
mod lyrics_search;
pub mod opus_source;
mod play_log_store;
mod playlist_store;
mod playlist_store_disk;
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
mod skip_sponsorblock;
mod skip_store;
mod spotify_api;
mod spotify_credentials;
mod spotify_csv;
mod spotify_embed;
mod spotify_search;
mod spotify_source;
mod store_health;
mod stream;
mod stream_chain;
pub mod stream_disk_cache;
mod stream_pool;
mod stream_prefetch_cache;
mod stream_reader;
mod stream_reqwest;
mod stream_stub;
mod stream_url_cache;
mod stream_window;
mod tray;
mod updater;
mod updater_github;
mod version_store;
mod ytdlp;

pub use atomic_file::{TEMP_FILE_MAX_AGE, sweep_stale_temp_files, write_atomic};
pub use audio::{AudioBackend, AudioError, AudioStub, NullAudio, clamp_speed};
pub use audio_device::{
    DEVICE_MONITOR_INTERVAL, DeviceIdentityMonitor, DeviceWatchAction, OutputDeviceWatch,
    current_output_device_identity, current_output_device_name, parse_default_sink,
};
pub use audio_dsp::{
    DEFAULT_MIN_TRAILING_SILENCE_SECS, DEFAULT_SILENCE_THRESHOLD_AMPLITUDE, LoudnessNormalizer,
    NormalizationStage, PROVIDER_LOUDNESS_OFFSET_DB, SkipSilence, StaticGain, TARGET_LUFS_DB,
    append_normalized, calculate_rms, compute_target_gain, db_to_linear, is_silent_frame,
    is_silent_sample, linear_to_db, normalization_stage, replaygain_linear, rms_to_db,
};
pub use audio_live::LiveAudio;
pub use audio_resample::ResamplingSource;
pub use audio_rodio::{RodioBackend, volume_to_amplitude};
pub use browser::{open_url, opener_command};
pub use catalog::{Catalog, CatalogError};
pub use catalog_client::{CatalogClient, RawTrack, search_body, search_url};
pub use catalog_live::LiveCatalog;
pub use catalog_requests::{
    EXPLORE_BROWSE_ID, HISTORY_BROWSE_ID, HOME_BROWSE_ID, LIBRARY_ALBUMS_BROWSE_ID,
    LIBRARY_PLAYLISTS_BROWSE_ID,
};
pub use catalog_stub::CatalogStub;
pub use clipboard::{Clipboard, MemoryClipboard, SystemClipboard, copy_text};
pub use cover::{CAPACITY, CoverCache, MemoryCoverCache};
pub use cover_art::{COVER_HIRES_MAX_PX, COVER_MAX_PX, decode_cover, fetch_artwork};
pub use cover_disk::{DiskCoverCache, cache_file_name, fnv1a_hex};
pub use custom_theme::{CUSTOM_THEME_FILE_NAME, CustomThemeFile};
pub use file_picker::{
    FilePicker, IMPORT_FILE_EXTENSIONS, RfdFilePicker, is_supported_import_file,
};
pub use handoff::{
    CHECKSUM_HEX_LEN, HANDOFF_PREFIX, MAX_HANDOFF_COOKIES, decode_handoff, encode_handoff,
};
pub use innertube_stream::{
    AudioFormat, CLIENT_ID, CLIENT_NAME, CLIENT_VERSION, DEVICE_MAKE, DEVICE_MODEL, FallbackReason,
    ITAG_AAC_COMPACT, ITAG_AAC_STANDARD, ITAG_OPUS_HIGH, InnertubeResolver, OS_NAME, OS_VERSION,
    PLAYER_ENDPOINT, REQUEST_TIMEOUT_SECS, USER_AGENT, parse_audio_formats, player_body,
    select_itag, select_stream,
};
pub use instance::{SingleInstanceGuard, instance_lock_name};
pub use lastfm_auth::{
    LASTFM_API_KEY_ENV, LASTFM_AUTH_URL, LASTFM_SECRET_ENV, LastFmAppKeys, LastFmSession,
    SessionPoll, authorize_url, parse_session_response, parse_token_response, request_session,
    request_token, resolve_app_keys, session_request_params, token_request_params,
};
pub use lyric_overrides::{
    LyricOverrideDir, MAX_OVERRIDE_BYTES, default_lyrics_dir, sanitize_id, sanitize_video_id,
};
pub use lyrics::{LrclibStub, LyricLine, LyricsCascade, LyricsSource, SyncedLyrics};
pub use lyrics_cache::{CachedLyrics, LYRICS_CACHE_CAPACITY};
pub use lyrics_lrclib::{
    CONFIDENCE_THRESHOLD, DURATION_NEUTRAL_SCORE, DURATION_TOLERANCE_SECS, LrclibClient,
    LyricsCandidate, PARTIAL_MATCH_SCORE, WEIGHT_ARTIST, WEIGHT_DURATION, WEIGHT_TITLE,
    duration_similarity, lrclib_get_url, match_confidence, normalize_text, parse_lrc,
    payload_matches, text_similarity,
};
pub use lyrics_lrcred::{
    LRC_RED_RELEASE_BASE, LRC_RED_SEARCH_URL, LRC_RED_TIMEOUT_SECS, LrcRedClient, LrcRedHit,
    build_search_query, parse_hits, parse_ttml_lines, select_hit, ttml_url,
};
pub use lyrics_search::{
    LYRICS_HIT_LIMIT, LYRICS_MATCH_THRESHOLD, LYRICS_RESULT_LIMIT, lyrics_match_score,
    rank_lyrics_candidates,
};
pub use opus_source::{OpusWebmSource, is_opus_webm};
pub use play_log_store::{DiskPlayLogStore, MemoryPlayLogStore, PLAY_LOG_FILE_NAME, PlayLogStore};
pub use playlist_store::{MemoryPlaylistStore, PlaylistStore};
pub use playlist_store_disk::{
    DiskPlaylistStore, PLAYLISTS_FILE, PLAYLISTS_SCHEMA_VERSION, PlaylistFile,
};
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
    DiskSettingsStore, MAX_CROSSFADE_SECONDS, MemorySettingsStore, Settings, SettingsStore,
    clamp_crossfade_seconds, default_config_dir,
};
pub use skip_sponsorblock::{
    SPONSORBLOCK_API_BASE, SPONSORBLOCK_DURATION_TOLERANCE_SECS, SkipProvider, SponsorBlockClient,
    hash_prefix, parse_response, skip_segments_url,
};
pub use skip_store::{DiskSkipStore, MemorySkipStore, SkipStore};
pub use spotify_api::SpotifyApiClient;
pub use spotify_credentials::{
    KeyringSpotifyCredentialStore, MemorySpotifyCredentialStore, SpotifyCredentialStore,
    SpotifyCredentials,
};
pub use spotify_csv::{
    REQUIRED_CSV_COLUMNS, load_playlist_file, parse_backup_json, parse_csv_records,
    parse_exportify_csv,
};
pub use spotify_embed::{
    EMBED_BASE_URL, EMBED_MAX_TRACKS, EMBED_TIMEOUT_SECS, SpotifyEmbedClient, embed_url,
    extract_next_data, parse_embed_html,
};
pub use spotify_search::{
    SEARCH_TIMEOUT_SECS, SONGS_FILTER_PARAMS, SearchFailure, SearchStatus, SongCandidate,
    SpotifySongSearch, classify_failure, classify_status, parse_song_candidates, songs_search_body,
    video_type_from,
};
pub use spotify_source::load_spotify_link;
pub use store_health::{HealthFlag, StoreHealth, quarantine};
pub use stream::{AudioQuality, ResolvedStream, StreamResolver, StreamStub};
pub use stream_chain::{PrefixFetcher, PrimaryStreamResolver, StreamChain};
pub use stream_disk_cache::StreamDiskCache;
pub use stream_pool::{BlockPool, STREAM_BLOCK_BYTES, STREAM_WORKERS};
pub use stream_prefetch_cache::{
    PREFETCH_BLOCK_COUNT, PREFETCH_BYTES, PREFETCH_CAPACITY, StreamPrefetchCache,
};
pub use stream_url_cache::{
    ResolverSource, STREAM_URL_CACHE_CAPACITY, STREAM_URL_TTL, StreamUrlCache,
};
pub use stream_window::{
    STREAM_WINDOW_AHEAD_BLOCKS, STREAM_WINDOW_BEHIND_BLOCKS, block_index, block_is_in_window,
    next_block_to_fetch,
};
pub use tray::{TrayAction, TrayHandle};
pub use updater::{ReleaseInfo, StaticUpdater, Updater};
pub use updater_github::{
    GithubUpdater, UPDATE_OWNER_REPO, is_newer, latest_url, parse_release_payload,
};
pub use version_store::{
    DiskVersionOverrideStore, MemoryVersionOverrideStore, VERSION_OVERRIDES_FILE,
    VERSION_OVERRIDES_SCHEMA_VERSION, VersionOverrideFile, VersionOverrideStore,
};
pub use ytdlp::{
    YTDLP_AUDIO_FORMAT, YTDLP_UPDATE_INTERVAL_SECS, YtDlpResolver, format_args, is_valid_sidecar,
    parse_version, provision_sidecar, release_asset_name, sidecar_needs_update,
    spawn_sidecar_refresh, watch_url,
};
