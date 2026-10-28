# moggyt

Native YouTube Music desktop client written in Rust, featuring a lightweight Slint user interface.

`moggyt` is an unofficial desktop client with no affiliation with Google or YouTube. It provides a fast, standalone
desktop listening experience without bundling a web browser or Electron runtime. The project is currently at the v0.1
stage, evolving toward the v1 Software Design Document: core hexagonal architecture, UI navigation, playback via
`rodio` with a `yt-dlp` stream resolution sidecar, LRCLIB synced lyrics, disk cover art caching, and OS keyring
session storage are functional.

> **Personal project:** `moggyt` is a personal project built to cover the author's own needs.
> Contributions are welcome, but maintenance is best-effort: expect limited review capacity
> rather than commercial-grade polish or support.

## Features

- **Playback & Transport**: persistent player bar with play, pause, next, previous, seek (-10s / +10s), volume
  adjustment, and elapsed position reporting.
- **Queue Management**: ordered in-memory playback queue with cursor navigation, shuffle, and repeat modes (off, all,
  one).
- **Search & Catalog**: catalog search powered by catalog API queries with client-side query sanitization.
- **Library & Authentication**: anonymous browsing by default; optional session cookie import into the OS credential
  store (Secret Service on Linux, Credential Manager on Windows) for library collections. Authenticated library
  endpoints are interim anonymous empty readers.
- **Synced Lyrics & Artwork**: synchronized timestamped lyrics from LRCLIB with dynamic user timing offset controls;
  cover art rendering with memory LRU and disk caching.
- **Desktop Integration**: system tray icon with playback controls and show/quit actions; outbound desktop media
  presence via MPRIS (Linux) and SMTC (Windows); dormant Discord Rich Presence (activates once a valid application ID is
  configured); dormant Last.fm scrobbler port.
- **Update Checks**: manual release checking against GitHub Releases with direct download links.
- **Planned Features**: hi-res cover upgrades and compact floating player (v1.1); equalizer, crossfade, and quality
  selector (v2).

## Quickstart (< 5 minutes)

Prerequisites: stable Rust toolchain (version 1.85 or newer, including `rustfmt` and `clippy`) and Git.

```bash
git clone https://github.com/kveld9/moggyt
cd moggyt
cargo run
```

## Installation

`moggyt` can be compiled from source or installed using pre-built binary packages from GitHub Releases:

- **From Source**: build release binaries with `cargo build --release`.
- **Release Packages**: standalone `.AppImage` and `.deb` packages, plus portable tarballs for Linux x86_64;
  NSIS installer (`.exe`) and portable zip for 64-bit Windows.
- **Arch Linux**: packaging definition available under `packaging/arch/moggyt-bin/PKGBUILD` which repackages the
  official Linux release tarball via `makepkg -si` (AUR publishing pending).
- **Stream Resolver Sidecar**: on first launch, `moggyt` automatically downloads a standalone `yt-dlp` binary into the
  app data directory with a ~3-day update check. Upstream `yt-dlp` (2026.10.21+) deprecates YouTube extraction without
  a JavaScript runtime; installing `deno` is recommended.

For step-by-step instructions across all platforms, see [Installation](docs/06-installation.md).

## Usage

Launch `moggyt` to display the main interface. Navigation is organized into four primary pages:

- **Search**: query tracks, albums, and artists.
- **Queue**: inspect, advance, retreat, and manage queued tracks.
- **Now Playing**: view active track artwork, playback progress, and timestamped synced lyrics with offset controls.
- **Library**: authenticate via cookie import, inspect saved playlists, liked tracks, albums, and check for updates.

The persistent bottom player bar remains active across all views for uninterrupted transport control. For complete
usage details, see the [User Guide](docs/05-user-guide.md).

## Configuration

`moggyt` adheres to standard platform conventions for storage and configuration:

- **Linux / Unix**: `$XDG_DATA_HOME/moggyt` (falling back to `~/.local/share/moggyt`).
- **Windows**: `%APPDATA%\moggyt`.
- **Covers Cache**: `<data_dir>/covers` (with automated temporary directory fallback).
- **Credentials**: imported cookies live exclusively in the operating system credential store (never in plain files or
  logs). Streaming requests remain strictly anonymous.
- **Diagnostics**: control log verbosity via the `RUST_LOG` environment variable (e.g. `RUST_LOG=debug cargo run`).

For in-depth configuration and directory layouts, see [Configuration](docs/07-configuration.md).

## Architecture

The codebase enforces a strict hexagonal architecture:

```text
UI Shell (src/slint_shell/, Slint) --> Pure Domain (src/domain/) <-- Ports <-- Adapters (src/infra/)
```

`src/main.rs` is the sole composition root, assembling dependencies and bootstrapping the Slint window. Pure
business logic in `domain/` contains zero I/O or wall-clock dependencies.

For architectural invariants and design documents, see [Architecture](docs/02-architecture.md) and the
[Software Design Document (SDD)](docs/04-sdd.md).

## Documentation

Comprehensive project documentation is maintained in the [`docs/`](docs/) directory:

| Document | Description |
| --- | --- |
| [Documentation Index](docs/README.md) | Overview and recommended reading paths for users and contributors |
| [Vision and Scope](docs/01-vision.md) | High-level goals, guiding principles, and scope boundaries |
| [Architecture](docs/02-architecture.md) | Hexagonal architecture, module map, port boundaries, and dependency rules |
| [Development](docs/03-development.md) | Toolchain setup, verification gates, TDD flow, and branch conventions |
| [Software Design Document (SDD)](docs/04-sdd.md) | Comprehensive v1 technical design, reference analysis, and requirements |
| [User Guide](docs/05-user-guide.md) | Interface navigation, transport controls, queue operations, and library sign-in |
| [Installation](docs/06-installation.md) | Building from source, release artifacts, package managers, and runtime setup |
| [Configuration](docs/07-configuration.md) | Platform data paths, cover cache, credential storage, and logging filters |
| [Troubleshooting](docs/08-troubleshooting.md) | Diagnostic matrix mapping operational symptoms to causes and solutions |
| [Releases and Packaging](docs/09-releases.md) | CI build pipelines, distribution artifacts, signing status, and update checks |
| [ADR Index](docs/adr/README.md) | Architecture Decision Records, including [ADR-001](docs/adr/001-update-check-releases.md) |

## Development

The project enforces test-driven development (TDD) for domain logic and intent handlers using in-memory fakes. All
contributions must enter via Pull Requests adhering to [Conventional Commits](https://www.conventionalcommits.org/)
in English (see [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md)). Direct commits to `main` are
prohibited.

Local verification gates matching continuous integration:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
./scripts/check-repo.sh
./scripts/check-markdown.sh
```

## Security

`moggyt` is an unofficial, independent client with no affiliation with Google or YouTube. Account cookies are stored
only in the OS credential store and are never sent with audio stream requests; the client includes no telemetry.
Report vulnerabilities confidentially per [SECURITY.md](SECURITY.md); do not open public issues.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).
