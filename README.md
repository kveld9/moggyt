# moggyt

Native YouTube Music desktop client in Rust and Slint.

Fast, lightweight desktop audio player with no web engine or Electron overhead.

> **Project Context:** `moggyt` is a personal learning project built to satisfy the author's own music playback needs.
> The codebase is largely LLM-generated and assisted as an exploratory project in AI-driven systems programming with
> Rust and Slint. Maintenance is best-effort.

## Highlights

- **Audio Engine**: Dual-sink Rodio playback with gapless crossfade support and automated `yt-dlp` stream resolution.
- **Interface**: Lightweight declarative Slint GUI with native theme support, system tray, and media keys (MPRIS/SMTC).
- **Synced Lyrics & Scrobbling**: Real-time synchronized lyrics via LRCLIB with dynamic offset timing, Last.fm
  scrobbling, and Discord Rich Presence.
- **Library & Search**: Fast catalog search, albums, artists, local playlist storage, listening history, and OS keyring
  credentials.

## Quickstart

### Prerequisites

- Rust 1.85+ (`cargo`, `rustc`, `rustfmt`, `clippy`)
- Git, CMake, and a C toolchain (the bundled Opus decoder compiles from vendored sources)
- Operating system development packages:
  - **Debian / Ubuntu**: `sudo apt install libasound2-dev libayatana-appindicator3-dev libdbus-1-dev pkg-config`
  - **Arch Linux**: `sudo pacman -S --needed alsa-lib libayatana-appindicator dbus pkg-config`
  - **Fedora**: `sudo dnf install alsa-lib-devel libayatana-appindicator-gtk3-devel dbus-devel pkgconf-pkg-config`
- Node.js 20+ (`npx`) for running local verification gates (`./scripts/check-markdown.sh`)
- Recommended runtime helper: `deno` (for `yt-dlp` JavaScript extraction)

### Build and Run

```bash
git clone https://github.com/kveld9/moggyt
cd moggyt
cargo run --release
```

Pre-built binaries (`.deb`, `.AppImage`, tarballs, `.exe`, zip) are available on
[GitHub Releases](https://github.com/kveld9/moggyt/releases).
For detailed setup across all platforms, see the [Installation Guide](docs/06-installation.md).

## Architecture

`moggyt` follows a strict hexagonal architecture separating UI and external services from domain logic:

```text
UI Shell (src/slint_shell/)  ──>  Pure Domain (src/domain/)  <──  Ports  <──  Adapters (src/infra/)
```

`src/main.rs` serves as the composition root. The domain layer contains zero I/O or wall-clock dependencies.
For architectural invariants, see [Architecture](docs/02-architecture.md) and the
[Software Design Document (SDD)](docs/04-sdd.md).

## Documentation

Comprehensive project documentation is organized in the [`docs/`](docs/) directory:

- [Documentation Hub](docs/README.md): Documentation map and recommended reading order.
- [User Guide](docs/05-user-guide.md): Navigation, transport controls, and library sessions.
- [Architecture](docs/02-architecture.md) & [SDD](docs/04-sdd.md): Port boundaries, module invariants, and specifications.
- [Configuration](docs/07-configuration.md): Data paths, cover cache, credentials, and diagnostics.
- [Troubleshooting](docs/08-troubleshooting.md): Operational symptoms, causes, and solutions.
- [Development](docs/03-development.md): Verification gates, TDD guidelines, and branch policies.

## Verification

Run local verification gates before submitting changes:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
./scripts/check-repo.sh
./scripts/check-markdown.sh
./scripts/check-i18n.sh
```

All contributions must follow [Conventional Commits](https://www.conventionalcommits.org/)
and enter through pull requests. See [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md).

## Security & Disclaimer

`moggyt` is an independent project and is not affiliated with Google or YouTube.
Account cookies reside strictly in the operating system credential store and are
never sent with media stream requests. Disclose vulnerabilities privately per
[SECURITY.md](SECURITY.md).

## License

MIT License. See [LICENSE](LICENSE). For bundled third-party components, see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
