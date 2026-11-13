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
├── main.rs            # Composition root: tracing, tokio runtime, adapter instantiation, Slint bootstrap
├── lib.rs             # Public root re-exporting domain, i18n, infra, slint_shell
├── i18n.rs            # Interface localization catalog, locale detection, and key lookup
├── slint_shell/       # Slint UI shell: window controller, UI state, callbacks, and worker synchronization
│   ├── app.rs         # Shell composition root: SlintAppState, event handlers, and window wiring
│   ├── callbacks.rs   # Global event handlers (navigation, search, queue, playback control)
│   ├── detail*.rs     # Detail view models, sections, callbacks, and track actions
│   ├── import*.rs     # Spotify/CSV import session, worker thread, searcher, and callbacks
│   ├── playback*.rs   # Playback worker, request dispatch, auto-skip, and player bar sync
│   ├── state*.rs      # SlintAppState definition, initialization, lyrics, search, and session state
│   ├── sync.rs        # Bi-directional sync between domain models and Slint UI properties
│   ├── theme.rs       # Theme palette application, accent overrides, and Matugen watcher
│   ├── window.rs      # Window event handlers, shortcuts, and platform window bindings
│   └── ...            # Subsystem adapters (lastfm_connect, discord, shortcuts, stats, etc.)
├── domain/            # Pure domain: immutable types, algorithms, and business rules (zero I/O)
│   ├── library.rs     # Library and catalog models (Album, Artist, Playlist, Shelf)
│   ├── local_playlist.rs # Local playlist and track models with confidence scoring
│   ├── matching.rs    # Multi-factor track matching heuristics (Levenshtein, Jaccard, Prism penalties)
│   ├── playback_phase.rs # Explicit playback lifecycle state machine and event modeling
│   ├── playlist_import.rs # Spotify URL and identifier reference parsing
│   ├── play_log.rs    # Listening history models, play records, and duration clamping
│   ├── queue.rs       # Pure playback queue (push, advance, retreat, shuffle, repeat)
│   ├── search.rs      # Search models (query, filter categories, track models, recent searches)
│   ├── segments.rs    # SponsorBlock and silence skip segment models
│   ├── shuffle.rs     # Smart shuffle ordering (artist adjacency, recency window)
│   ├── sleep_timer.rs # Sleep timer state, presets, and faded volume calculation
│   ├── theme.rs       # Theme identifiers, RGB palettes, and player bar presentation modes
│   ├── top_result.rs  # Ranked top-result selection for catalog search
│   ├── version_override.rs # Recording alternative overrides (catalog ID to replacement track)
│   └── videos.rs      # Music-video ordering and visibility policy for catalog results
└── infra/             # Ports and adapters: real I/O and deterministic test stubs
    ├── atomic_file.rs # Crash-safe atomic file persistence and stale temp file cleanup
    ├── audio*.rs      # AudioBackend port, Rodio dual-sink crossfade, rubato resampler, DSP filters
    ├── catalog*.rs    # Catalog port, InnerTube client, endpoint parsers (artist, detail, explore, home, search)
    ├── cover*.rs      # CoverCache port, memory LRU, and disk-backed cache
    ├── innertube_stream.rs # Direct audio stream URL resolution (VISIONOS client, Opus/AAC itags)
    ├── lyrics*.rs     # LyricsSource port, LRCLIB client, lrc.red TTML client, and cascade fallback
    ├── playlist_store*.rs # PlaylistStore port and atomic JSON disk storage (playlists.json)
    ├── presence*.rs   # DesktopPresence (MPRIS/SMTC) and Discord Rich Presence
    ├── scrobble*.rs   # Scrobbler port, Last.fm client, credentials, and API signatures
    ├── session*.rs    # SessionStore port and OS keyring cookie persistence (Secret Service / Win CredMgr)
    ├── settings*.rs   # SettingsStore port and atomic JSON configuration (settings.json)
    ├── stream*.rs     # StreamResolver port, layered StreamChain, block pool worker, prefetch & disk cache
    ├── tray.rs        # System tray icon and platform action loop
    ├── updater*.rs    # Updater port and GitHub Releases update checker
    └── ytdlp.rs       # Standalone yt-dlp sidecar download and stream resolution fallback

ui/
├── shell.slint        # Thin import-only UI entry point (exempt from modularity cap)
├── labels.slint       # Static chrome translation keys and labels global
├── theme.slint        # Native Theme global, palette slots, and geometry tokens
├── components/        # Reusable UI controls and widgets
│   ├── cards.slint    # Shelf and category cards
│   ├── controls.slint # Buttons, sliders, switches, and text inputs
│   ├── player_bar.slint # Persistent transport player bar (classic & floating modes)
│   ├── queue_panel.slint # Collapsible upcoming queue panel
│   ├── settings/      # Settings panels (playback, theme, session, integrations, about)
│   └── ...            # Special widgets (alternative_picker, vinyl_deck, shortcut_scope)
├── icons/             # Custom SVG vector path icons (audio, navigation, playback)
└── views/             # Top-level page views
    ├── content_area.slint # Header, breadcrumbs, update notification, and view switcher
    ├── detail_view.slint  # Album, artist, and playlist tracklistings
    ├── explore_view.slint # Catalog explore categories and mood/genre shelves
    ├── import_*.slint     # Spotify/CSV import modal, progress overlay, and review screen
    ├── library_view.slint # User playlists, liked songs, and saved albums
    ├── now_playing_view.slint # Dual-column playback dashboard with synced lyrics
    ├── search_view.slint  # Search input, recent-search chips, and result shelves
    ├── settings_view.slint # Application settings tabs and preferences
    ├── stats_view.slint   # Wrapped-style listening statistics with period chips
    └── window.slint       # MoggytWindow root component definition
```

## Stream resolution order

Playback stream resolution is layered (`stream_chain.rs`):

1. InnerTube tier cache entries for the track and quality (20 min TTL, 8 entries).
2. yt-dlp tier cache entries for the same track and quality.
3. InnerTube `player` endpoint with the `VISIONOS` client (`innertube_stream.rs`):
   direct stream URLs (itag 251 Opus in WebM preferred for High with AAC fallback;
   itag 140 AAC for Standard; itag 139 AAC for Compact) with the provider
   `loudnessDb` per format.
4. yt-dlp sidecar (`ytdlp.rs`) whenever InnerTube fails: HTTP error, timeout, a
   `playabilityStatus` other than `OK`, no compatible audio format, a ciphered
   (`signatureCipher`/`cipher`) entry, an invalid payload, or a bot check
   ("Sign in to confirm you're not a bot"). The failure reason is logged once per
   cause and playback continues through the sidecar.

Opus in WebM (itag 251, ~136 kbps, 48 kHz) is decoded via libopus through `opus_source.rs`
using Symphonia for container demuxing. For `High` quality, InnerTube resolution prefers
itag 251 and falls back to AAC formats (141, 140, 250, 139) when Opus is unavailable;
the yt-dlp sidecar selector for `High` similarly prefers WebM Opus before AAC and
unconstrained audio. The yt-dlp sidecar and its `deno` requirement remain mandatory fallbacks.

Player requests include the `Cookie` header and, when `SAPISID` is present in the cookies,
an `Authorization: SAPISIDHASH <unix-seconds>_<sha1>` header from the stored user session,
enabling Premium formats (such as itag 141 AAC ~256 kbps) and age-restricted content;
requests remain anonymous when no session is present.

## Loudness normalization

`loudness_normalization` selects exactly one normalization stage per track:

1. When the resolver reports a provider `loudnessDb` (InnerTube VISIONOS path),
   the raw value is shifted by +7 dB (`PROVIDER_LOUDNESS_OFFSET_DB`, matching
   ViMusic's `loudnessDb + 7`) because YouTube's reference loudness sits ~7 dB
   below the -14 LUFS target. The track is then scaled by the static ReplayGain
   factor `10^(-effective_db/20)`; tracks with effective loudness at or below
   0 dB (raw loudness <= -7 dB) use unity gain. For example, a measured live
   `loudnessDb` of +0.99 dB shifts to +7.99 dB and yields a real attenuation of
   ~0.40 linear gain (-7.99 dB) instead of near-unity gain. The dynamic
   `LoudnessNormalizer` is not chained in this case.
2. When no `loudnessDb` is available (yt-dlp fallback path), the dynamic
   `LoudnessNormalizer` keeps its existing behavior.

Signal order is decoder -> optional `SkipSilence` -> static gain or dynamic
normalizer -> sink. The static gain sits inside the source graph, so the sink
volume and the crossfade/fade-in ramps multiply it once and never attenuate the
same signal twice.

Decoded sources whose rate differs from the output stream are resampled with
rubato `FftFixedIn` (BlackmanHarris2 window), which avoids rodio's linear
interpolation; AAC itag 140 is 44.1 kHz and Opus itag 251 is 48 kHz, so this
matters on both native-rate devices.

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
pool with those bytes and with the remote total length when the range response
reported one, so the seeded reader can seek relative to its end and the WebM
probe runs with container cues; a skipped track keeps at most two prefetched
prefixes resident.

Range requests use a single 60 s total timeout that covers connection and transfer,
so a stalled connection fails that block fetch instead of blocking the worker pool,
and `BlockPool::drop` can join its workers within that bound.

Next-track prefix prefetch is skipped when the track is already fully cached on
disk, so a cached track never pays for a redundant 256 KiB download.

The DSP wrappers (`SkipSilence`, `StaticGain`, `LoudnessNormalizer`) forward `try_seek`
to the inner source and reset their processing state, so seeking works with
skip-silence and normalization enabled.

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
