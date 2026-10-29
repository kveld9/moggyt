# 02 - Architecture

Minimal hexagonal architecture, aligned with repository governance and
modular design principles.

## Dependency direction

```text
UI (slint_shell/, Slint) --> domain (domain/) <-- ports <-- adapters (infra/)
```

- `domain/`: immutable types and pure rules. Direct `std::net`, `std::fs`, `std::process`, clocks, and randomness are forbidden.
- `infra/`: implements the ports with real I/O (HTTP to YouTube Music, audio backend).
- `main.rs` (bin) is the only *composition root*: builds the `tokio` runtime,
  instantiates adapters, and injects dependencies into `slint_shell`.
- Time and randomness are injected as parameters or fake clocks in tests (determinism, zero `sleep` in tests).

## Module map

```text
src/
├── main.rs            # Composition root (bin): tracing, tokio runtime, Slint bootstrap
├── lib.rs             # Public contracts: slint_shell + domain + i18n + infra
├── i18n.rs            # Interface localization skeleton and translation lookup
├── slint_shell/       # Slint UI shell: window controller, bridges, and app runner
│   ├── app.rs         # Shell composition root: SlintAppState, event handlers, and window wiring
│   └── bridges/       # Platform and domain integration bridges
├── domain/
│   ├── mod.rs         # Re-exports the public domain contract
│   ├── library.rs     # Library and catalog models (Album, Artist, Playlist, Shelf)
│   ├── queue.rs       # Pure playback queue (add, next, previous, shuffle, repeat)
│   ├── search.rs      # Search models (immutable query, track model)
│   └── theme.rs       # Color palette data and ThemeId domain models
└── infra/
    ├── mod.rs         # Re-exports adapters and port traits
    ├── audio.rs       # AudioBackend trait definition
    ├── audio_rodio.rs # rodio playback adapter with LiveAudio fallback
    ├── cover*.rs      # CoverCache port and memory/disk caching adapters
    ├── catalog_client.rs # Catalog API search request builder and parser
    ├── instance.rs    # SingleInstanceGuard lock
    ├── lyrics*.rs     # LyricsSource port and LRCLIB synced lyrics client
    ├── presence*.rs   # DesktopPresence (MPRIS/SMTC) and Discord Rich Presence
    ├── scrobble.rs    # Scrobbler port and Last.fm client adapter
    ├── session*.rs    # SessionStore port and Keyring cookie persistence
    ├── settings.rs    # SettingsStore port and atomic JSON persistence
    ├── stream.rs      # StreamResolver trait definition
    ├── tray.rs        # System tray handle and action event loop
    ├── updater*.rs    # Updater port and GitHub Releases release checker
    ├── ytdlp.rs       # yt-dlp sidecar download and stream resolution adapter
    └── ytm.rs         # Catalog adapter implementing search and library views
```

## Evolution rules

1. One concept, one owner: each domain type lives in a single module (single source of truth).
2. Acyclic graph: `domain` never depends on `infra` or `slint_shell`; `infra` never depends on `slint_shell`.
3. New adapters (real API, `rodio`/`gstreamer`) are added behind the existing port, never touching the domain.
4. Cyclomatic complexity CC <= 5 per function; guard clauses first; DRY only from the third repetition.
5. File-size modularity caps: Rust (`.rs`) files max 900 lines (adjusted from 800 to fit clean baseline
   `src/infra/ytm.rs` at 891 lines); Slint (`.slint`) modules max 400 lines per module. `ui/shell.slint` is the single
   designated thin entry, exempt only while it stays an import-only entry under ~200 lines with no component
   definitions. Existing monoliths (`src/slint_shell/app.rs`, `src/infra/catalog_client.rs`, and `ui/shell.slint`)
   are temporarily grandfathered in `scripts/check-repo.sh` pending modularization. Remediation path: split oversized
   files by concept (one view or endpoint-domain per module), landing each extracted unit via one small PR per module
   (< 400 lines diff).
