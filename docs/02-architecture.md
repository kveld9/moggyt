# 02 - Architecture

Minimal hexagonal architecture, aligned with repository governance and
modular design principles.

## Dependency direction

```text
UI (slint_shell/, Slint) --> domain (domain/) <-- ports <-- adapters (infra/)
```

- `domain/`: immutable types and pure rules. Direct `std::net`, `std::fs`, `std::process`, clocks, and randomness are forbidden.
- `infra/`: implements the ports with real I/O (HTTP to YouTube Music, audio backend).
- `main.rs` (bin) is the only *composition root*: constructs every live adapter
  and passes them as ports through `AppDependencies`; `slint_shell::app::run`
  performs wiring only.
- Time and randomness are injected as parameters or fake clocks in tests (determinism, zero `sleep` in tests).

## Module map

```text
src/
├── main.rs            # Composition root (bin): tracing, tokio runtime, Slint bootstrap
├── lib.rs             # Public contracts: slint_shell + domain + i18n + infra
├── i18n.rs            # Interface localization skeleton and translation lookup
├── slint_shell/       # Slint UI shell: window controller, bridges, and app runner
│   ├── app.rs         # Shell composition root: SlintAppState, event handlers, and window wiring
│   ├── detail_callbacks.rs # Detail view callbacks: track selection, play and shuffle of the open container
│   ├── history.rs     # Account history playlist entry projection, detail labeling, and subtitles
│   ├── lastfm_connect.rs # Last.fm desktop authentication and connect flow
│   ├── scrobble_control.rs # Scrobble submission and presence sync for the playing track
│   ├── settings_callbacks.rs # Settings callbacks: account sign-in, preferences, sleep timer and window resize
│   ├── sleep_fade.rs  # Sleep timer polling and the volume fade applied before it fires
│   ├── stats.rs       # Wrapped-style listening statistics UI view model and refresh wiring
│   ├── stream_prefetch.rs # Next-track stream URL prefetching thread spawner
│   └── bridges/       # Platform and domain integration bridges
├── domain/
│   ├── mod.rs         # Re-exports the public domain contract
│   ├── library.rs     # Library and catalog models (Album, Artist, Playlist, Shelf)
│   ├── listening_stats.rs # Wrapped-style listening statistics from play records
│   ├── play_log.rs    # Listening history models and duration clamping
│   ├── queue.rs       # Pure playback queue (add, next, previous, shuffle, repeat)
│   ├── search.rs      # Search models (immutable query, track model)
│   ├── shuffle.rs     # Smart shuffle ordering (artist adjacency, recency)
│   ├── version_override.rs # Audio version overrides (catalog id to alternative recording)
│   └── theme.rs       # Color palette data and ThemeId domain models
└── infra/
    ├── mod.rs         # Re-exports adapters and port traits
    ├── audio.rs       # AudioBackend trait definition (playback, crossfade, seeking, volume)
    ├── audio_live.rs  # Dual-mode live rodio vs. muted audio backend fallback
    ├── audio_rodio.rs # rodio playback adapter with dual-sink crossfade and stream cache support
    ├── audio_fade.rs  # Fade-out and fade-in ramps applied to rodio sinks
    ├── audio_stream_cache_worker.rs # Background download of a resolved track into the disk stream cache
    ├── browser.rs     # Platform browser opener adapter
    ├── cover*.rs      # CoverCache port and memory/disk caching adapters
    ├── custom_theme.rs # Custom theme JSON loader with file modification monitoring
    ├── catalog_client.rs # Catalog API search request builder and parser
    ├── instance.rs    # SingleInstanceGuard lock
    ├── lastfm_auth.rs # Last.fm desktop authentication (token, authorize URL, session)
    ├── lyrics*.rs     # LyricsSource port, lrc.red TTML client, LRCLIB client, and cascade
    ├── lyrics_cache.rs # LyricsSource decorator with bounded FIFO cache
    ├── play_log_store.rs # PlayLogStore port and JSON Lines file persistence
    ├── presence*.rs   # DesktopPresence (MPRIS/SMTC) and Discord Rich Presence
    ├── scrobble.rs    # Scrobbler port and Last.fm client adapter
    ├── session*.rs    # SessionStore port and Keyring cookie persistence
    ├── settings.rs    # SettingsStore port and atomic JSON persistence
    ├── stream.rs      # StreamResolver trait definition
    ├── stream_pool.rs # Prefetching 256 KiB block pool with worker threads and sliding window
    ├── stream_window.rs # Sliding window scheduling for block prefetch
    ├── stream_chain.rs # Layered stream resolution: InnerTube primary, yt-dlp fallback
    ├── stream_disk_cache.rs # Disk-backed audio stream cache with quota and LRU eviction
    ├── stream_url_cache.rs # Stream URL cache with TTL and capacity eviction
    ├── tray.rs        # System tray handle and action event loop
    ├── updater*.rs    # Updater port and GitHub Releases release checker
    ├── ytdlp.rs       # yt-dlp sidecar download and stream resolution adapter
    ├── innertube_stream.rs # InnerTube VISIONOS player adapter (direct AAC URLs, loudnessDb)
    ├── catalog.rs      # Catalog port trait definition
    ├── catalog_live.rs # Catalog API-backed live adapter
    └── catalog_stub.rs # Deterministic catalog stub for testing

ui/
├── shell.slint        # Thin import-only UI entry point
├── stats.slint        # StatsData global and StatsEntryData model definitions
└── views/
    └── stats_view.slint # Wrapped-style statistics view with period chips and rankings
```

## Stream resolution order

Playback stream resolution is layered (`stream_chain.rs`):

1. InnerTube tier cache entries for the track and quality (20 min TTL, 8 entries).
2. yt-dlp tier cache entries for the same track and quality.
3. InnerTube `player` endpoint with the `VISIONOS` client (`innertube_stream.rs`):
   direct AAC URLs (itag 139 for compact, 140 for standard and high) with the
   provider `loudnessDb` per format.
4. yt-dlp sidecar (`ytdlp.rs`) whenever InnerTube fails: HTTP error, timeout, a
   `playabilityStatus` other than `OK`, no compatible audio format, a ciphered
   (`signatureCipher`/`cipher`) entry, an invalid payload, or a bot check
   ("Sign in to confirm you're not a bot"). The failure reason is logged once per
   cause and playback continues through the sidecar.

Opus (itag 251) stays excluded because the rodio/symphonia stack has no Opus
decoder; AAC in MP4 is the only supported stream format. The yt-dlp sidecar and
its `deno` requirement remain mandatory fallbacks.

Unauthenticated playback also has an AAC ceiling: itag 140 (~130 kbps) is the highest
adaptive format served, while itag 141 (~256 kbps) requires Premium authentication. The
`High` quality preference therefore resolves to the same itag as `Standard` and shares
its format selector instead of requesting an unreachable `abr>=192` floor.

## Loudness normalization

`loudness_normalization` selects exactly one normalization stage per track:

1. When the resolver reports a provider `loudnessDb` (InnerTube VISIONOS path),
   the track is scaled by the static ReplayGain factor `10^(-db/20)`; tracks at or
   below 0 dB use unity gain. The dynamic `LoudnessNormalizer` is not chained in
   this case.
2. When no `loudnessDb` is available (yt-dlp fallback path), the dynamic
   `LoudnessNormalizer` keeps its existing behavior.

Signal order is decoder -> optional `SkipSilence` -> static gain or dynamic
normalizer -> sink. The static gain sits inside the source graph, so the sink
volume and the crossfade/fade-in ramps multiply it once and never attenuate the
same signal twice.

## Block streaming and seek behavior

Audio byte ranges are fetched by a pool of four workers in 256 KiB blocks
(`stream_pool.rs`). The read position selects the base block; workers fetch the
block at the read position first and then walk forward up to 40 blocks, with 8
blocks retained behind, so playback never performs a network read on the audio
callback thread. Blocks outside the window are always dropped, which bounds resident memory to
roughly 12 MiB regardless of stream length.

A seek re-targets the pool at the new block and discards results outside the
window; `Read + Seek` semantics, the EOF boundary at the reported total length,
and the sequential fallback for servers that ignore `Range` are unchanged.

The next queued track prefetches its first block (256 KiB) into a bounded
in-memory cache (`stream_prefetch_cache.rs`). Starting that track seeds the block
pool with those bytes, so the first read does not issue a cold range request; a
skipped track keeps at most two prefetched prefixes resident.

Range requests use a single 60 s total timeout that covers connection and transfer,
so a stalled connection fails that block fetch instead of blocking the worker pool,
and `BlockPool::drop` can join its workers within that bound.

Next-track prefix prefetch is skipped when the track is already fully cached on
disk, so a cached track never pays for a redundant 256 KiB download.

## Evolution rules

1. One concept, one owner: each domain type lives in a single module (single source of truth).
2. Acyclic graph: `domain` never depends on `infra` or `slint_shell`; `infra` never depends on `slint_shell`.
3. New adapters (real API, `rodio`/`gstreamer`) are added behind the existing port, never touching the domain.
4. Cyclomatic complexity CC <= 5 per function; guard clauses first; DRY only from the third repetition.
5. File-size modularity caps: Rust (`.rs`) files max 800 lines for every Rust file, with no exemptions;
   Slint (`.slint`) modules max 400 lines per module. `ui/shell.slint` is the single designated thin entry, exempt
   only while it stays an import-only entry under ~200 lines with no component definitions; `MoggytWindow` itself
   lives in `ui/views/window.slint`. No file is exempt from these caps.
   Remediation path: split oversized
   files by concept (one view or endpoint-domain per module), landing each extracted unit via one small PR per module
   (< 400 lines diff).
