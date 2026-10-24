# Installation

This document describes how to install and run `moggyt` from pre-built release packages or from source.

## Prerequisites

Building `moggyt` from source requires:

- Stable Rust toolchain 1.85 or newer (edition 2024 support).
- Cargo and standard components (`rustfmt`, `clippy`).
- Operating system development headers:
  - **Linux**: ALSA development headers (`libasound2-dev` on Debian/Ubuntu, `alsa-lib` on Arch Linux) for audio
    output via `rodio`, and Ayatana AppIndicator headers (`libayatana-appindicator3-dev` on Debian/Ubuntu,
    `libayatana-appindicator` on Arch Linux) for system tray support.
  - **Windows**: standard MSVC or GNU toolchain with Windows SDK.

Refer to your distribution package manager or project CI workflows (`.github/workflows/ci.yml`) for package naming.

## Release Artifacts

Official builds are distributed as GitHub Releases for Linux and Windows x86_64:

| Platform | Format | Description |
| --- | --- | --- |
| Linux | `.AppImage` | Standalone portable executable bundle |
| Linux | `.deb` | Debian and Ubuntu binary package |
| Linux | `.rpm` | Fedora, RHEL, and openSUSE binary package |
| Linux | `moggyt-*-linux.tar.gz` | Standalone portable tarball |
| Linux (Arch) | `moggyt-bin` | Arch Linux PKGBUILD repackaging release tarball (AUR pending) |
| Windows | `.exe` (NSIS) | Windows 64-bit installer |
| Windows | `moggyt-*-windows.zip` | Portable zip archive |

Release binaries are not digitally signed in v1. Operating systems may present security prompts on initial launch:

- On Windows, Microsoft Defender SmartScreen may display an unrecognized app warning. Select "More info" and "Run
  anyway" to proceed.
- On Linux desktop environments, verify executable permissions on portable AppImage files (`chmod +x moggyt.AppImage`).

## Installing on Arch Linux

Arch Linux packaging is available via `packaging/arch/moggyt-bin`. Note that `moggyt-bin` is not yet published on the
AUR; installing via AUR helpers (`yay -S moggyt-bin` or `paru -S moggyt-bin`) is not supported until AUR submission is
completed.

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

`makepkg -si` downloads the official release tarball, verifies SHA-256 sums, builds the native `.pkg.tar.zst` package
locally, auto-resolves runtime dependencies (`alsa-lib`, `libayatana-appindicator`), and installs `/usr/bin/moggyt`
along with the desktop launcher entry (`/usr/share/applications/moggyt.desktop`).

To update to future releases, pull the latest changes and rebuild:

```bash
git pull
makepkg -si
```

## Installing on Linux from Portable Tarball

1. Download `moggyt-<version>-linux.tar.gz` and `SHA256SUMS.txt` from GitHub Releases.
1. Verify archive integrity:

```bash
sha256sum -c SHA256SUMS.txt
```

1. Extract the standalone binary and place it in your `PATH`:

```bash
tar -xzf moggyt-*-linux.tar.gz
chmod +x moggyt
mv moggyt ~/.local/bin/
```

## Installing on Windows from Portable Zip

Download `moggyt-<version>-windows.zip` from GitHub Releases, extract the archive, and run `moggyt.exe`.

## Building from Source

To compile and launch the application directly using Cargo:

```bash
git clone https://github.com/kveld9/moggyt
cd moggyt
cargo run --release
```

## First Launch and Stream Resolution Sidecar

On first startup, `moggyt` prepares its runtime environment:

1. Resolves and creates the platform application data directory.
2. Checks for a local `yt-dlp` executable sidecar in the data directory.
3. If absent or outdated (~3-day self-update cadence), downloads the appropriate standalone release binary from GitHub.
4. Initializes audio output using `rodio`. If audio device initialization fails, the client falls back to a muted stub
   to ensure interface accessibility without crashing.

### JavaScript Runtime Notice

Upstream `yt-dlp` (version 2026.10.21 and later) warns that YouTube stream extraction without an installed JavaScript
runtime is deprecated. For optimal long-term stream resolution stability, ensure a supported JavaScript runtime (`deno`)
is installed on the host system.

## Verification Gates

Contributors and package maintainers can verify repository compliance using the project verification suite:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
./scripts/check-repo.sh
./scripts/check-markdown.sh
```
