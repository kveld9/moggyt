# 04 - Software Design Document

Status: accepted draft. Date: 2026-10-24.
Scope: native YouTube Music client in Rust (`moggyt`), releases for Linux and Windows only.
Locked decisions: streaming via a `yt-dlp` sidecar; anonymous use plus cookie import for sign-in;
LRCLIB-only synced lyrics in v1.

## 1 Context and goals

`moggyt` is a native YouTube Music client written in Rust, with a Slint interface.
The repository already provides a hexagonal scaffold: the UI shell (`src/slint_shell`) delegates to a
pure domain (`src/domain/`), all I/O hides behind ports implemented in `src/infra/`, and
`src/main.rs` is the sole composition root. This document fixes what v1 means on top of that base.

Goals:

- Cold start under 1.5 s, idle RSS under 150 MB, one binary per OS, no web runtime.
- Feature parity with the useful subset of the Electron/Tauri references, without shipping a browser.
- Survive YouTube internal changes with minimal user pain by decoupling stream resolution behind a port.

Non-goals for v1: macOS support, an offline downloads manager, casting to remote devices,
video playback, store packaging (Flatpak, MSI, Snap), and a public plugin SDK.

## 2 Reference synthesis

Three existing reference architectures were used as requirements sources:

- Electron-web reference (Electron wrapper around the web player): full web parity and a proven
  feature checklist (media keys, tray icon, Discord presence, global shortcuts, themes,
  auto-update from GitHub Releases). Its architecture is rejected by definition: no Chromium.
- native-backend reference (Tauri 2 + React, catalog API direct): talks to the catalog API directly, prefetches
  with aggressive caching, resolves audio through a `yt-dlp` sidecar, and keeps a Rust backend
  for streaming proxy, cookies, tray, and media keys. Closest to the target design, minus the
  webview: `moggyt` renders its own UI in Slint instead.
- plugin-taxonomy reference (Electron, plugin system with around 40 plugins): taxonomy source
  (non-music segment skipping, `synced-lyrics`, `scrobbler`, `downloader`, `equalizer`, `crossfade`,
  `quality-changer`, `picture-in-picture`, `shortcuts`, `discord`, `taskbar-mediacontrol`,
  `notifications`, `playback-speed`, `visualizer`). Its plugin contract shape informs where
  `moggyt` ports end so a minimal plugin host stays possible in v2.

| Source                    | Reuse in `moggyt`                               | Reject                                 |
| ------------------------- | ----------------------------------------------- | -------------------------------------- |
| Electron-web reference    | Feature checklist, release cadence              | Electron shell wrapping the web player |
| native-backend reference  | Catalog client, `yt-dlp` sidecar, cache plan    | Webview shell (WebView2 / WebKitGTK)   |
| plugin-taxonomy reference | Plugin taxonomy, settings-per-feature pattern   | CSS themes, full plugin SDK in v1      |

### 2.1 Extended media backlog (playback, lyrics, library, sync)

These extended backlog requirements encompass playback DSP, multi-provider lyrics, library and
queue operations, caching tiers, and synchronization, each mapped to its owning port and target
milestone. Architectural patterns and feature specifications maintain strict hexagonal boundary
isolation, with domain logic remaining purely deterministic and free of I/O side effects.

Extended backlog requirements (milestones and port mappings):

1. **Catalog API hardening** (`Catalog` port; v1 hardening / v1.1):
   - Request slim response payloads on browse requests to limit JSON payloads to required fields.
   - Distinguish official releases from user uploads and expose a "prefer official releases" toggle.
   - Encapsulate client identity in a single configuration module to enable rapid version bumps
     when YouTube alters endpoints.
   - Retain cookie-based auth under locked decision D-2; record an alternative catalog API as a secondary
     catalog fallback, while keeping the `yt-dlp` sidecar as the primary stream resolver (D-1).
2. **Lyrics specification and timing model** (`LyricsSource` port; FR-8 extension, v1.1+):
   - Implement a lyrics provider chain: primary synced lyrics (v1), followed by transcript and secondary provider
     fallbacks (using a multi-provider fallback chain as reference; external scrapers are deferred).
   - Adopt a normalized word-timing model `{timeMs, sungUntilMs}` spanning LRC, TTML, QRC, YRC, and KRC formats.
   - Support local lyric override files keyed by `videoId` with user-adjustable millisecond timing offset.
   - Enforce confidence-scored search matching against track title, artist, and duration before attaching lyrics.
   - BYOK translation cache: SHA-256 hash of text and target language as key, GZIP-compressed, 2 MiB memory
     LRU (deferred to v1.1+).
3. **Playback DSP pipeline ordering** (`AudioBackend` port; v1.1 / V2):
   - DSP execution sequence: gapless playback -> crossfade (V2) -> skip-silence -> -14 LUFS loudness
     normalization -> 10-band EQ with EQ profile import (V2) -> playback rate adjustment (0.5x-2.0x,
     live rate-only resampling; pitch preservation out of scope)
     -> sleep timer (presets + stop-after-track) -> per-network stream quality ceiling (Wi-Fi vs metered).
   - Playback rate adjustment (0.5x-2.0x, rate-only, pitch preservation out of scope) is live; sleep
     timer, persistent queue, skip-silence, and loudness normalization are v1.1 candidates;
     remainder V2.
4. **Offline caching tiers** (`CoverCache` and future `DownloadCache` port; v1.1 / V2):
   - Stream-cache LRU: configurable stream-cache byte budget for instant offline replay (v1.1).
   - Explicit tagged downloads: export audio files with Vorbis Comment and ID3 metadata into a standalone
     music library directory, combined with Smart Prefetch for upcoming queued tracks (V2; honors the v1
     non-goal of no downloads manager).
   - Unified local-file playback with visual origin badges deferred to V2.
5. **Library and queue enhancements** (`domain/queue.rs`, `domain/library.rs`; FR-4 / FR-7 extensions, v1.1):
   - Multiple named queues (`QueueId -> Vec<Track>`) with drag-and-drop reorder and insert-at-position.
   - Persistent queue across application restarts (serializing track list, cursor index, and track position).
   - Local metadata-override table for title/artist edits stored in SQLite without altering remote data.
   - JSON backup export pattern for library and playlist preservation (v1.1).
   - Playlist URL import via URL parsing and automated catalog search matching.
   - Playlist folders, pin-to-top, grid-density settings, and a history-by-day tab (v1.1 / V2).
6. **Discovery and UI ergonomics** (`app` UI shell, `AudioBackend`; v1.1 / V2):
   - Quick-picks shelf, mood/genre explore hubs, search-by-lyrics, and rich zero-query landing recommendations.
   - On-device SQLite play-log powering personal dynamic mixes, Replay, and Wrapped-style listening statistics.
   - Waveform progress bar, playback speed selector button, and "stats for nerds" panel (codec, container, bitrate).
   - Dynamic theming: color extraction from thumbnail artwork plus portable theme JSON files.
   - Compact mini-bar vs full-window dual presentation mode.
   - Output device picker leveraging `rodio` host device enumeration.
   - Non-music segment skipping keyed by `videoId`.
   - Multi-select batch actions for queue and library rows.
   - QR code login handoff from companion device to desktop as a headless-auth companion for D-2.
   - Local LAN browser-remote / phone-as-remote, with collaborative listening rooms (self-hostable
     WebSocket architecture) as a V2 stretch.
7. **Authentication and synchronization** (`SessionStore` and `Scrobbler` ports; v1.1 / V2):
   - Token-paste login flow for headless desktop environments.
   - YouTube Music playback history write-back via authenticated catalog feedback endpoints.
   - Dual scrobbling support: primary scrobbler (already v1.1) and secondary scrobbler. Multi-account switching
     deferred to V2+.

## 3 Scope

V1 (MVP):

- Search with filters; idle state with recent-search history; artist, album, playlist detail pages.
- Transport (play, pause, next, previous, seek, volume) plus a persistent player bar.
- Playback queue with reorder, shuffle, and repeat modes; radio and autoplay queues.
- Library read access (playlists, liked tracks, saved albums) behind optional sign-in.
- Synced lyrics from LRCLIB; cover art with memory plus disk cache.
- Persistent settings; MPRIS on Linux and SMTC on Windows; tray icon with single instance.
- Update check against GitHub Releases.

V1.1: Discord presence, Last.fm scrobbling, hi-res cover upgrade, persistent queue, sleep timer,
skip-silence, loudness normalization, stream-cache budget, JSON backup export, search-by-lyrics,
non-music segment skip, QR token-login helper.
V2: equalizer (10-band EQ detail), crossfade, quality selector (per-network quality ceiling), minimal
plugin host, multi-queue, tagged downloads + Smart Prefetch, local-file unified library, collaborative
listening rooms, translation cache, stats/Replay dashboard.
Explicitly not v1: downloads manager, casting, video, podcasts, plugin SDK.

## 4 Functional requirements

- FR-1: search the catalog with type filters (tracks, albums, artists, playlists), featuring an idle state with
  persistent recent-search history (capped at 10 entries, case-insensitive deduplication, per-entry deletion, and full
  clear).
- FR-2: render search result shelves for active queries; prefetch detail content on hover or focus intent.
- FR-3: show track, album, artist, and playlist detail views from cached plus fresh data.
- FR-4: queue operations (add, remove, reorder, clear, shuffle, repeat off / all / one).
- FR-5: transport controls with seek and volume, including keyboard shortcuts.
- FR-6: start radio from any track and keep an autoplay continuation queue.
- FR-7: read the signed-in library (playlists, likes, saved albums); like or dislike tracks.
  Note: authenticated library endpoints are pending; interim implementation operates as anonymous empty readers.
- FR-8: display synced lyrics with a user-adjustable offset when LRCLIB has a match.
- FR-9: load cover art progressively with placeholder, memory LRU, and disk cache.
- FR-10: persist settings (volume, theme, quality preference, shortcuts) across restarts.
- FR-11: single instance; tray icon with play/pause/next and show/hide actions.
- FR-12: check GitHub Releases for updates and offer the download page (no silent install).
  Note: unauthenticated checks return 404 on private repo and soft-fail to CheckFailed (see ADR-001).
- FR-13 (v1.1): persistent playback queue across application restarts (serializes track list and position).
- FR-14 (v1.1): sleep timer with standard interval presets and stop-after-current-track option.
- FR-15 (v1.1): DSP settings for skip-silence and -14 LUFS loudness normalization in playback.
- FR-16 (v1.1): stream-cache LRU with user-sized storage budget and JSON library backup import/export.
- FR-17 (v1.1): search-by-lyrics lookup and non-music segment-skip by video id.
- FR-18: import Spotify playlists and albums from web links, embed URLs, Spotify URIs, Exportify CSV files,
  and JSON backups into local playlists with automated matching and interactive review.

Anonymous use covers FR-1 through FR-6, FR-8 through FR-12, FR-13 through FR-17, and FR-18. FR-7 requires sign-in.

## 5 Non-functional requirements

- NF-1: cold start under 1.5 s on a reference quad-core machine with a warm disk cache.
- NF-2: search-to-first-paint under 300 ms on cache hit; track skip under 200 ms locally.
- NF-3: idle RSS under 150 MB without cover cache pressure.
- NF-4: `domain/` and `app.rs` stay free of I/O; time and randomness enter as parameters.
- NF-5: cyclomatic complexity CC <= 5 per function (hard limit 10); guard clauses first.
- NF-6: every PR under 400 lines; one logical change per commit; Conventional Commits in English.
- NF-7: CI gates stay green: `fmt`, `clippy -D warnings`, `test`, markdownlint, standards audit.

## 6 Architecture

Dependency direction is unchanged:

```text
slint_shell (Slint shell) --> domain (pure) <-- ports <-- infra (adapters)
```

Ports (each a trait in `infra/`, with in-memory fakes in tests):

- `Catalog`: catalog API search, shelves, detail, library, and radio (FR-1, FR-2, FR-3, FR-6, FR-7).
- `StreamResolver`: maps a video id to a playable audio URL (FR-5 transport input).
- `AudioBackend`: play, pause, seek, volume, and position queries over `rodio`.
- `SessionStore`: session cookies in the OS credential store, never in logs or plain files.
- `LyricsSource`: LRCLIB lookup returning parsed LRC lines (FR-8).
- `CoverCache`: memory LRU plus disk cache keyed by artwork URL hash (FR-9).
- `Presence`: Discord status updates; a no-op implementation when disabled.
- `Updater`: GitHub Releases check returning the latest version and asset URLs (FR-12).
- `TrackSearcher`: InnerTube candidate search port for songs-filtered queries (FR-18).
- `PlaylistStore`: local playlist persistence port with atomic disk storage in `playlists.json` (FR-18).

Crate layout stays `src/{main.rs, lib.rs, slint_shell/, domain/, infra/}`. `main.rs` builds the
`tokio` runtime, instantiates adapters, injects them into `SlintAppState`, and bootstraps Slint.
New adapters are added behind existing ports; `domain` never depends on `infra` or `app`.

The modular api/stream/lyrics/playback/cache/stats subsystem split maps directly to moggyt ports and
domain structures: `catalog_client` maps to `Catalog`, `stream` maps to `StreamResolver`, `lyrics` maps to
`LyricsSource`, playback DSP filters map to `AudioBackend`, `download/cache` maps to `CoverCache` and
a future `DownloadCache` port (V2), and `stats` maps to an on-device `PlayLog` domain table (v1.1).
The `DownloadCache` and plugin-source extension ports remain strictly deferred to V2 to preserve v1
hexagonal boundaries.

Locked decision D-1 (streaming): v1 resolves audio through a bundled `yt-dlp` sidecar,
downloaded on first launch into the app data directory and self-updated roughly every 3 days
(same model as the native-backend reference). Rationale: YouTube breaks stream extraction several times a year;
the fix then arrives via `yt-dlp` without an emergency `moggyt` release. The `StreamResolver`
port keeps a pure-Rust decipher implementation replaceable later without touching `domain`.

Runtime caveat (live verification 2026-10-24): upstream `yt-dlp` (2026.10.21) extracts stream URLs successfully but
warns that YouTube extraction without a supported JavaScript runtime is deprecated. Full long-term extraction
robustness may require bundling or documenting a JS runtime dependency (e.g. `deno`) alongside the sidecar.

Locked decision D-2 (auth): anonymous by default; library features unlock by importing
`music.youtube.com` cookies from the user's browser into the OS keyring (Secret Service or
a compatible provider on Linux, Credential Manager on Windows). Streaming stays anonymous and
never sends account cookies. Rationale: there is no webview for an embedded login page, and
cookie import is the proven reference flow for unofficial clients.

Locked decision D-3 (lyrics): v1 integrates LRCLIB only; the `LyricsSource` trait is designed
as a chain so additional lyrics providers can be appended in v1.1. Rationale: one integration with
no API keys covers synced lyrics for the popular catalog; the other sources add rate limits,
scraping fragility, and mostly unsynchronized text.

Audio stack: `rodio` with `symphonia` decoding (single binary; WASAPI on Windows,
ALSA/PulseAudio on Linux). No GStreamer dependency in v1.

### 6.1 Spotify import subsystem

The Spotify import subsystem imports playlists and albums into locally stored playlists without requiring Spotify
developer API credentials or user accounts.

#### 6.1.1 Ingestion adapters

- **Embed Ingestion (`SpotifyEmbedClient`)**: Fetches public playlist or album embed HTML
  (`https://open.spotify.com/embed/playlist/<id>` or `/album/<id>`) and extracts the embedded `__NEXT_DATA__` JSON
  payload, parsing track title, artist (subtitle), duration, and metadata without requiring Spotify Web API keys.
- **File Ingestion (`parse_exportify_csv`, `parse_spotify_backup_json`)**: Pure, allocation-light parsers. The CSV
  adapter parses RFC 4180 files exported by Exportify, requiring `Track Name`, `Artist Name(s)`, `Album Name`,
  `Track Duration (ms)`, `ISRC`, and `Track URI` columns. The JSON adapter processes standard Spotify JSON backup dumps.

#### 6.1.2 Link and identifier parsing

`parse_spotify_ref` parses input links into `SpotifyRef::Playlist(id)` or `SpotifyRef::Album(id)`. Supported formats:

- Standard URLs: `https://open.spotify.com/playlist/<id>` and `/album/<id>`.
- Localized URLs: `https://open.spotify.com/intl-<country>/playlist/<id>`.
- Embed URLs: `https://open.spotify.com/embed/playlist/<id>` and `/embed/album/<id>`.
- Legacy user playlist URLs: `https://open.spotify.com/user/<user>/playlist/<id>`.
- Spotify URIs: `spotify:playlist:<id>` and `spotify:album:<id>`.

Identifiers must be 22-character base62 (ASCII alphanumeric) strings. Malformed links or unsupported schemes return
`None`.

#### 6.1.3 Title sanitization and normalization

Before candidate search and scoring, `clean_track_title` and `normalize_for_match` process track titles:

- **Smart Punctuation**: Replaces curly quotes, dashes, and non-breaking spaces with standard ASCII equivalents.
- **Featured Clauses**: Splits featuring clauses (`feat.`, `ft.`, `featuring` in parentheses, brackets, after hyphens,
  or inline) into a separate artist token for multi-artist matching.
- **Noise Stripping**: Removes version noise, remasters, deluxe qualifiers, bonus tracks, explicit tags, and 4-digit
  year mixes (`(2011 Remaster)`, `[Remastered]`, `- 2020 Mix`).
- **Preserved Qualifiers**: Retains version qualifiers essential to track identity: `live`, `acoustic`, `instrumental`,
  `demo`, and `radio edit`.
- **Fuzzy Normalization**: Folds common Latin diacritics to ASCII, strips non-alphanumeric punctuation, and collapses
  whitespace. Non-Latin scripts pass through unaltered.

#### 6.1.4 Multi-factor matching heuristics

Candidates returned by InnerTube search are evaluated using multi-factor scoring (`score_candidate`):

- **Title Similarity (`WEIGHT_TITLE = 0.40`)**: Combines Levenshtein character edit distance and token Jaccard
  similarity on normalized strings.
- **Artist Matching**: Channel attribution matching against primary artist (`WEIGHT_ARTIST_PRIMARY = 0.30`) or
  featured artist (`WEIGHT_ARTIST_SECONDARY = 0.15`), handling `- Topic` channel suffixes.
- **Duration Delta**: Scores duration difference between candidate and source track:
  - `<= 2s`: `+0.15` exact match bonus.
  - `<= 5s`: `0.0` acceptable tolerance.
  - `<= 15s`: `-0.20` suspect duration penalty.
  - `<= 30s`: `-0.40` wrong edit penalty.
  - `> 30s`: `-0.60` music video duration penalty.
- **Prism-Style Video Penalty**: Distinguishes official audio tracks from music videos and user uploads:
  - Auto-generated topic channels: `+0.15` bonus.
  - Audio Track Video (ATV): `+0.15` bonus.
  - Official Music Video (OMV): `-0.40` penalty.
  - User Generated Content (UGC): `-0.50` penalty.
  - Title keyword penalties (`official video`, `music video`, `cover`, `reaction`): `-0.35` per keyword.
  - Total Prism adjustment is clamped to `-0.50..=0.15`.
- **Composite Score and Confidence Buckets**: Total composite score is clamped to `0.0..=1.0`:
  - `High`: Score `>= 0.85`.
  - `Medium`: Score in `[0.60, 0.85)`.
  - `Low`: Score `< 0.60` or NaN.
  Candidates below `MIN_ACCEPT_SCORE = 0.50` are filtered out.

#### 6.1.5 Concurrency, pacing, and worker pipeline

The matching worker (`import_worker.rs`, `import_searcher.rs`) executes in background threads:

- **Worker Concurrency**: Uses 2 worker threads for search execution.
- **Request Pacing**: Paced at 200 ms between network queries to avoid triggering upstream rate limits.
- **Query Cache**: Per-run query cache avoids redundant network requests for duplicate tracks within a playlist.
- **Backoff and Jitter**: Retries HTTP 429 and 503 rate-limit responses using exponential backoff with pseudorandom
  jitter (base 500 ms, up to 3 retries).
- **Atomic Cancellation**: Supports cancellation via `AtomicBool`, cleanly stopping worker execution and returning
  partial resolutions.
- **Ranked Alternatives**: Retains up to 3 alternative candidate hits (`MAX_ALTERNATIVES = 3`) scoring at or above
  `0.50` for user review.
- **Event Streaming**: Dispatches progress events (`Started`, `Track`, `Progress`, `Finished`, `Cancelled`, `Failed`)
  to update UI overlays.

#### 6.1.6 Persistence and storage

Imported playlists are stored locally via `DiskPlaylistStore` (`playlist_store_disk.rs`):

- **Storage Location**: Saved in `playlists.json` in the configuration directory (`~/.config/moggyt/` on Linux,
  `%APPDATA%\moggyt` on Windows).
- **Schema Envelope**: Uses a versioned envelope (`version: 1`, `PlaylistFile`) containing an array of `LocalPlaylist`
  objects.
- **Atomic Persistence**: Writes to a temporary file (`playlists.json.tmp.<pid>`) and atomically renames to guarantee
  file integrity across crashes or abrupt process exits.
- **Namespace Isolation**: Identifiers use a `local:` prefix (e.g. `local:<source_id>`) to prevent collisions with
  remote catalog identifiers.
- **Offline Resolution**: Stored playlists display in the Library page and open in the Detail view without contacting
  YouTube Music for catalog lookups.

#### 6.1.7 Command-line interface (CLI)

The importer supports headless execution from the terminal:

- Import from Spotify link: `moggyt --import-spotify <URL> --headless [--json]`
- Import from CSV or JSON file: `moggyt --import-csv <PATH> --headless [--json]`
- Streams progress diagnostics to `stderr`.
- Outputs a machine-readable JSON summary report to `stdout` when `--json` is supplied, or human-readable summary
  text otherwise.

#### 6.1.8 Known limits

- **Embed 100-Track Cap**: Spotify embed HTML documents only contain the first 100 tracks of a playlist or album.
  Playlists exceeding 100 tracks require Exportify CSV or JSON file exports to import completely.
- **Public Playlists Only**: Link-based import requires public playlist accessibility; private, unlisted, or
  region-locked Spotify playlists cannot be scraped via embed and require CSV or JSON export files.
- **Region-Unavailable Tracks**: Tracks absent from the YouTube Music catalog or restricted by licensing remain
  unresolved, preserving their source title and artist in the review view.

## 7 Data model

Immutable domain types, all serializable: `Track { id, title, artist, album, duration }`,
`Album`, `Artist`, `Playlist`, `Shelf { title, items }`, `PlaybackQueue` (push, advance,
retreat, shuffle, repeat modes), and `SearchQuery` (immutable value with paginated results).
Wall-clock time enters only as injected parameters so tests stay deterministic.

Import domain models:

- `SpotifyTrack`: source track metadata (`title`, `artist`, optional `album`, `duration_ms`, optional `isrc`).
- `SpotifyPlaylist`: source playlist metadata (`id`, `title`, `author`, optional `artwork`, `tracks`).
- `SpotifyRef`: target reference enum (`Playlist(id)`, `Album(id)`).
- `MatchConfidence`: categorical confidence levels (`High`, `Medium`, `Low`).
- `MatchCandidate`: candidate metadata (`title`, `channel`, optional `duration_secs`, `video_type`).
- `MatchScore`: breakdown scores (`title`, `artist`, `duration`, `prism`, `total`).
- `LocalTrack`: resolved track (`video_id`, `title`, `artist`, optional `album`, optional `duration_secs`, `confidence`).
- `LocalPlaylist`: persisted playlist (`id`, `title`, `author`, optional `artwork`, `tracks`).

## 8 UI design

Pages: Search, Library, Now Playing, Detail, Settings, Explore, a persistent bottom player bar,
and a collapsible right-side queue panel. Shelf and track lists are virtualized;
image decoding runs off the UI thread with placeholders. Theming is a single native
`Theme { background, surface, accent, text }` struct (YouTube red accent), not CSS.

## 9 Desktop integration

- Linux: MPRIS via D-Bus, tray icon, single instance via socket lock, secrets via Secret Service.
- Windows: SMTC integration, tray icon with thumbnail controls, single instance via named mutex.
- Shared: media keys, desktop notifications, configurable global shortcuts (off by default
  where the compositor restricts them, with an explanatory note).
  Note: inbound MPRIS/SMTC media-key routing is live (non-blocking dispatch to UI thread).

## 10 Packaging and releases

CI builds on `ubuntu-latest` and `windows-latest` only. Artifacts: `.AppImage` and `.deb` for Linux;
NSIS `.exe` (x64) for Windows. No signing in v1, so the docs explain the expected SmartScreen and quarantine prompts.
`Cargo.lock` stays committed. V1 auto-update is check-plus-link to the Releases page; silent self-update is
deferred to v2.
Arch Linux is served via `packaging/arch/moggyt-bin` PKGBUILD (`-bin`, repackages release tarball).

## 11 Security and privacy

Unofficial-client disclaimer (no affiliation with Google or YouTube). Account cookies are used
only for library calls and live only in the OS credential store. Streaming requests are
anonymous. No telemetry in v1. Secret scanning and dependency audit run in CI.

## 12 Testing and quality gates

TDD is mandatory for `domain/` and `app.rs` intent handlers: red test on public behavior,
minimal implementation, refactor on green. All ports get in-memory fakes; catalog API parsers
get golden JSON fixtures; unit tests never touch the network or real audio. Per-OS manual
checklists cover media keys, tray, installer, and keyring behavior. Local gate before push:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
./scripts/check-repo.sh
./scripts/check-markdown.sh
```

## 13 Milestones

Each milestone ships as small PRs under 400 lines:

1. Ports plus fakes; harden queue and search domain logic.
2. Read-only catalog adapter (search, shelves, detail).
3. `yt-dlp` resolver plus `rodio` playback plus player bar.
4. Cookie-import auth with OS keyring plus library views.
5. LRCLIB lyrics plus cover cache.
6. MPRIS/SMTC plus tray plus single instance (inbound media-key routing is live).
7. Release pipeline (AppImage, deb, exe) plus update check.
8. Polish: Discord presence, Last.fm, hi-res covers.

## 14 Risks and mitigations

- YouTube changes stream extraction: mitigated by the `yt-dlp` sidecar and the port boundary.
- YouTube changes catalog API schemas: golden fixtures plus a narrow parser layer per endpoint.
- Slint UI limits for rich media lists: virtualized rows and off-thread image decoding.
- Linux keyring fragmentation: fall back to a clear error plus docs when no provider exists.
- Scope creep from the 40-plugin reference: ports anticipate extensions; the SDK stays v2.
- Catalog API client-identity rot: mitigated by a single client-identity configuration module
  combined with golden JSON response fixtures for early breakage detection.
- Stream deciphering upkeep: avoided entirely via the `yt-dlp` sidecar; in-app deciphering
  is explicitly rejected (delegated to the `yt-dlp` sidecar instead) to prevent ongoing
  maintenance churn.

## 15 Open questions

- Q-1: exact `yt-dlp` minimum version pinning and rollback policy after a bad upstream release.
- Q-2: cookie export UX (manual paste versus helper extension) to be prototyped in milestone 4.
- Q-3: quality preference mapping (which `yt-dlp` audio format selector backs each setting).
- Q-4: lyrics word-timing model: whether the canonical internal representation should normalize to an
  extended LRC structure or a TTML-style word span model `{timeMs, sungUntilMs}` for multi-source compatibility.
- Q-5: stream-cache storage budget default size (e.g. 512 MB versus 2 GB) and eviction policy (LRU by
  last access versus FIFO by creation date).
- Q-6: collaborative listening room protocol: adopting a standardized WebSocket room protocol versus
  a simpler minimal LAN remote pairing mechanism for desktop remote control.
