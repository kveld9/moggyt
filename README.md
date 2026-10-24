# moggyt

Native YouTube Music client written in Rust, with an `eframe`/`egui` graphical interface.

> Unofficial client with no affiliation with Google or YouTube.
>
> Early scaffold (v0.1.0): the repository standards, CI, and hexagonal module skeleton are in place.
> Playback and the YouTube Music API integration land incrementally behind small, atomic PRs.

## Quickstart (< 5 minutes)

Prerequisites: Rust stable toolchain (rustfmt + clippy), Git.

```bash
git clone <repo-url> moggyt
cd moggyt
cargo run
```

Verify everything locally (same gates as CI):

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
./scripts/check-repo.sh
./scripts/check-markdown.sh
```

## Install

Pre-built binaries and packages are published on the GitHub [Releases](https://github.com/kveld9/moggyt/releases) page.

### Linux (portable tarball)

1. Download `moggyt-<version>-linux.tar.gz` and `SHA256SUMS.txt` from [Releases](https://github.com/kveld9/moggyt/releases).
1. Verify archive integrity:

```bash
sha256sum -c SHA256SUMS.txt
```

1. Extract the standalone `moggyt` binary and place it in your `PATH`:

```bash
tar -xzf moggyt-*-linux.tar.gz
chmod +x moggyt
mv moggyt ~/.local/bin/
```

### Arch Linux (PKGBUILD)

Arch Linux packaging is available via `packaging/arch/moggyt-bin`. Note that `moggyt-bin` is not yet
published on the AUR; installing via `yay -S moggyt-bin` or `paru -S moggyt-bin` is not supported
until AUR submission is completed.

1. Install build tools:

```bash
sudo pacman -S --needed base-devel git
```

1. Clone the repository and navigate to the package directory:

```bash
git clone https://github.com/kveld9/moggyt.git
cd moggyt/packaging/arch/moggyt-bin
```

1. Build and install the package:

```bash
makepkg -si
```

`makepkg -si` downloads the official release tarball, verifies SHA-256 sums, builds the native `.pkg.tar.zst`
package locally, auto-resolves runtime dependencies (`alsa-lib`, `libayatana-appindicator`), and installs
`/usr/bin/moggyt` along with the desktop launcher entry (`/usr/share/applications/moggyt.desktop`).

To update to future releases, pull the latest changes and rebuild:

```bash
git pull
makepkg -si
```

### Windows (portable zip)

Download `moggyt-<version>-windows.zip` from [Releases](https://github.com/kveld9/moggyt/releases),
extract the archive, and run `moggyt.exe`.

## Repository layout

```text
.
├── src/                    # Rust sources: lib (domain/ports/UI) + bin (composition root)
│   ├── main.rs             # Composition root (bin): logging, tokio runtime, eframe bootstrap
│   ├── lib.rs              # Public contracts: app + domain + infra
│   ├── app.rs              # Top-level eframe App (UI shell, navigation state)
│   ├── domain/             # Pure domain: playback queue, search models (no I/O)
│   └── infra/              # Adapters: YouTube Music API, audio backend (ports)
├── docs/                   # Product docs: vision, architecture, development
├── scripts/                # Deterministic maintenance scripts (executable, set -euo pipefail)
├── .github/workflows/      # CI (fmt/clippy/test), secret scanning, CodeQL example
├── Cargo.toml / Cargo.lock # Single source of truth for dependencies (committed)
├── rustfmt.toml            # Formatting policy (max_width 100, edition 2024)
└── clippy.toml             # Lint policy (complexity threshold 5)
```

See [`docs/02-architecture.md`](docs/02-architecture.md) for the module boundaries and dependency direction.

## Standards

- Commits follow [Conventional Commits](https://www.conventionalcommits.org/) in English, one logical change each.
- Emojis are forbidden in code, scripts, configs, CI, and commits; allowed only in `README.md`, `docs/`, and release notes.
- Domain code stays free of I/O; external crates hide behind ports in `src/infra/`.
- Target cyclomatic complexity CC <= 5 per function (hard limit 10).

Details: [`CONTRIBUTING.md`](CONTRIBUTING.md), [`CHECKLIST.md`](CHECKLIST.md), [`docs/`](docs/).

## Security

Do NOT report vulnerabilities via public issues. See [`SECURITY.md`](SECURITY.md).

## License

MIT. See [`LICENSE`](LICENSE).
