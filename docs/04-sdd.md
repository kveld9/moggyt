# 04 - Software Design Document

Status: accepted draft. Date: 2026-10-24.
Scope: native YouTube Music client in Rust (`moggyt`), releases for Linux and Windows only.
Locked decisions: streaming via a `yt-dlp` sidecar; anonymous use plus cookie import for sign-in;
LRCLIB-only synced lyrics in v1.

## 1 Context and goals

`moggyt` is a native YouTube Music client written in Rust, with an `eframe`/`egui` interface.
The repository already provides a hexagonal scaffold: the UI shell (`src/app.rs`) delegates to a
pure domain (`src/domain/`), all I/O hides behind ports implemented in `src/infra/`, and
`src/main.rs` is the sole composition root. This document fixes what v1 means on top of that base.

Goals:

- Cold start under 1.5 s, idle RSS under 150 MB, one binary per OS, no web runtime.
- Feature parity with the useful subset of the Electron/Tauri references, without shipping a browser.
- Survive YouTube internal changes with minimal user pain by decoupling stream resolution behind a port.

Non-goals for v1: macOS support, an offline downloads manager, casting to remote devices,
video playback, store packaging (Flatpak, MSI, Snap), and a public plugin SDK.

## 2 Reference synthesis

Three existing projects were used as requirements sources:

- `ytmdesktop` (Electron wrapper around `music.youtube.com`): full web parity and a proven
  feature checklist (media keys, tray icon, Discord presence, global shortcuts, themes,
  auto-update from GitHub Releases). Its architecture is rejected by definition: no Chromium.
- `YTubic` (Tauri 2 + React, InnerTube direct): talks to the InnerTube API directly, prefetches
  with aggressive caching, resolves audio through a `yt-dlp` sidecar, and keeps a Rust backend
  for streaming proxy, cookies, tray, and media keys. Closest to the target design, minus the
  webview: `moggyt` renders its own UI in `egui` instead.
- `pear-desktop` (Electron, plugin system with around 40 plugins): taxonomy source
  (`sponsorblock`, `synced-lyrics`, `scrobbler`, `downloader`, `equalizer`, `crossfade`,
  `quality-changer`, `picture-in-picture`, `shortcuts`, `discord`, `taskbar-mediacontrol`,
  `notifications`, `playback-speed`, `visualizer`). Its plugin contract shape informs where
  `moggyt` ports end so a minimal plugin host stays possible in v2.

| Source       | Reuse in `moggyt`                              | Reject                                   |
| ------------ | ---------------------------------------------- | ---------------------------------------- |
| `ytmdesktop` | Feature checklist, release cadence             | Electron shell wrapping the web player   |
| `YTubic`     | InnerTube client, `yt-dlp` sidecar, cache plan | Webview shell (WebView2 / WebKitGTK)     |
| `pear`       | Plugin taxonomy, settings-per-feature pattern  | CSS themes, full plugin SDK in v1        |

## 3 Scope

V1 (MVP):

- Search with filters; home shelves with prefetch on intent; artist, album, playlist detail pages.
- Transport (play, pause, next, previous, seek, volume) plus a persistent player bar.
- Playback queue with reorder, shuffle, and repeat modes; radio and autoplay queues.
- Library read access (playlists, liked tracks, saved albums) behind optional sign-in.
- Synced lyrics from LRCLIB; cover art with memory plus disk cache.
- Persistent settings; MPRIS on Linux and SMTC on Windows; tray icon with single instance.
- Update check against GitHub Releases.

V1.1: Discord presence, Last.fm scrobbling, hi-res cover upgrade, compact floating player.
V2: equalizer, crossfade, quality selector, minimal plugin host.
Explicitly not v1: downloads manager, casting, video, podcasts, plugin SDK.

## 4 Functional requirements

- FR-1: search the catalog with type filters (tracks, albums, artists, playlists).
- FR-2: render home shelves; prefetch detail content on hover or focus intent.
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

Anonymous use covers FR-1 through FR-6 plus FR-8 through FR-12. FR-7 requires sign-in.

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
app (egui shell) --> domain (pure) <-- ports <-- infra (adapters)
```

Ports (each a trait in `infra/`, with in-memory fakes in tests):

- `Catalog`: InnerTube search, shelves, detail, library, and radio (FR-1, FR-2, FR-3, FR-6, FR-7).
- `StreamResolver`: maps a video id to a playable audio URL (FR-5 transport input).
- `AudioBackend`: play, pause, seek, volume, and position queries over `rodio`.
- `SessionStore`: session cookies in the OS credential store, never in logs or plain files.
- `LyricsSource`: LRCLIB lookup returning parsed LRC lines (FR-8).
- `CoverCache`: memory LRU plus disk cache keyed by artwork URL hash (FR-9).
- `Presence`: Discord status updates; a no-op implementation when disabled.
- `Updater`: GitHub Releases check returning the latest version and asset URLs (FR-12).

Crate layout stays `src/{main.rs, lib.rs, app.rs, domain/, infra/}`. `main.rs` builds the
`tokio` runtime, instantiates adapters, injects them into `AppState`, and bootstraps `eframe`.
New adapters are added behind existing ports; `domain` never depends on `infra` or `app`.

Locked decision D-1 (streaming): v1 resolves audio through a bundled `yt-dlp` sidecar,
downloaded on first launch into the app data directory and self-updated roughly every 3 days
(same model as YTubic). Rationale: YouTube breaks stream extraction several times a year;
the fix then arrives via `yt-dlp` without an emergency `moggyt` release. The `StreamResolver`
port keeps a pure-Rust decipher implementation replaceable later without touching `domain`.

Runtime caveat (live verification 2026-10-24): upstream `yt-dlp` (2026.10.21) extracts stream URLs successfully but
warns that YouTube extraction without a supported JavaScript runtime is deprecated. Full long-term extraction
robustness may require bundling or documenting a JS runtime dependency (e.g. `deno`) alongside the sidecar.

Locked decision D-2 (auth): anonymous by default; library features unlock by importing
`music.youtube.com` cookies from the user's browser into the OS keyring (Secret Service or
a compatible provider on Linux, Credential Manager on Windows). Streaming stays anonymous and
never sends account cookies. Rationale: there is no webview for an embedded login page, and
cookie import is the proven YTubic flow for unofficial clients.

Locked decision D-3 (lyrics): v1 integrates LRCLIB only; the `LyricsSource` trait is designed
as a chain so Musixmatch and Genius can be appended in v1.1. Rationale: one integration with
no API keys covers synced lyrics for the popular catalog; the other sources add rate limits,
scraping fragility, and mostly unsynchronized text.

Audio stack: `rodio` with `symphonia` decoding (single binary; WASAPI on Windows,
ALSA/PulseAudio on Linux). No GStreamer dependency in v1.

## 7 Data model

Immutable domain types, all serializable: `Track { id, title, artist, album, duration }`,
`Album`, `Artist`, `Playlist`, `Shelf { title, items }`, `PlaybackQueue` (push, advance,
retreat, shuffle, repeat modes), and `SearchQuery` (immutable value with paginated results).
Wall-clock time enters only as injected parameters so tests stay deterministic.

## 8 UI design

Pages: Search, Library, Queue, Now Playing, plus a persistent bottom player bar; a right-side
panel layout follows once the bottom bar is stable. Shelf and track lists are virtualized;
image decoding runs off the UI thread with placeholders. Theming is a single native
`Theme { background, surface, accent, text }` struct (YouTube red accent), not CSS.
The v1.1 floating widget is a second lightweight `eframe` window sharing the playback handle.

## 9 Desktop integration

- Linux: MPRIS via D-Bus, tray icon, single instance via socket lock, secrets via Secret Service.
- Windows: SMTC integration, tray icon with thumbnail controls, single instance via named mutex.
- Shared: media keys, desktop notifications, configurable global shortcuts (off by default
  where the compositor restricts them, with an explanatory note).
  Note: inbound MPRIS/SMTC media-key routing is attach-no-op (outbound metadata only; inbound dispatch deferred).

## 10 Packaging and releases

CI builds on `ubuntu-latest` and `windows-latest` only. Artifacts: `.AppImage`, `.deb`,
`.rpm` for Linux; NSIS `.exe` (x64) for Windows. No signing in v1, so the docs explain the
expected SmartScreen and quarantine prompts. `Cargo.lock` stays committed. V1 auto-update is
check-plus-link to the Releases page; silent self-update is deferred to v2.
Arch Linux is served via `packaging/arch/moggyt-bin` PKGBUILD (`-bin`, repackages release tarball); AUR submission is TODO.

## 11 Security and privacy

Unofficial-client disclaimer (no affiliation with Google or YouTube). Account cookies are used
only for library calls and live only in the OS credential store. Streaming requests are
anonymous. No telemetry in v1. Secret scanning and dependency audit run in CI.

## 12 Testing and quality gates

TDD is mandatory for `domain/` and `app.rs` intent handlers: red test on public behavior,
minimal implementation, refactor on green. All ports get in-memory fakes; InnerTube parsers
get golden JSON fixtures; unit tests never touch the network or real audio. Per-OS manual
checklists cover media keys, tray, installer, and keyring behavior. Local gate before push:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
./scripts/check-repo.sh
```

## 13 Milestones

Each milestone ships as small PRs under 400 lines:

1. Ports plus fakes; harden queue and search domain logic.
2. Read-only InnerTube adapter (search, shelves, detail).
3. `yt-dlp` resolver plus `rodio` playback plus player bar.
4. Cookie-import auth with OS keyring plus library views.
5. LRCLIB lyrics plus cover cache.
6. MPRIS/SMTC plus tray plus single instance (inbound media-key routing deferred; outbound metadata only).
7. Release pipeline (AppImage, deb, rpm, exe) plus update check.
8. Polish: Discord presence, Last.fm, hi-res covers.

## 14 Risks and mitigations

- YouTube changes stream extraction: mitigated by the `yt-dlp` sidecar and the port boundary.
- YouTube changes InnerTube schemas: golden fixtures plus a narrow parser layer per endpoint.
- `egui` limits for rich media lists: virtualized rows and off-thread image decoding.
- Linux keyring fragmentation: fall back to a clear error plus docs when no provider exists.
- Scope creep from the 40-plugin reference: ports anticipate extensions; the SDK stays v2.

## 15 Open questions

- Q-1: exact `yt-dlp` minimum version pinning and rollback policy after a bad upstream release.
- Q-2: cookie export UX (manual paste versus helper extension) to be prototyped in milestone 4.
- Q-3: quality preference mapping (which `yt-dlp` audio format selector backs each setting).
