# Releases and Packaging

This document outlines the release delivery pipeline, distribution packages, update mechanics, and security guarantees
for `moggyt`.

## Build and CI Pipeline

Release automation runs across standard GitHub Actions runners:

- **Linux**: built on `ubuntu-latest` with standard ALSA and Ayatana AppIndicator build dependencies.
- **Windows**: built on `windows-latest` targeting 64-bit Windows (`x86_64-pc-windows-msvc`).
- **Reproducibility**: `Cargo.lock` is tracked in version control and acts as the single source of truth for all crate
  dependencies across development and release pipelines.

## Distribution Artifacts

Release assets by shipment status:

| Artifact | Target OS | Distribution Channel | Status |
| --- | --- | --- | --- |
| `moggyt-*-linux.tar.gz` | Linux (Generic tarball) | GitHub Releases (consumed by Arch PKGBUILD) | Shipped |
| `moggyt-*-windows.zip` | Windows 64-bit | GitHub Releases | Shipped |
| `moggyt_*_amd64.deb` | Linux (Debian, Ubuntu) | GitHub Releases | Shipped |
| `moggyt-*-x86_64.AppImage` | Linux | GitHub Releases | Planned |
| `moggyt-*.x86_64.rpm` | Linux (Fedora, openSUSE) | GitHub Releases | Planned |
| `moggyt-*-setup.exe` | Windows 64-bit | GitHub Releases (NSIS installer) | Planned |

### Arch Linux Packaging

The repository maintains an Arch Linux package specification in `packaging/arch/moggyt-bin/PKGBUILD`. It downloads and
repackages the official pre-compiled Linux release tarball into `/usr/bin/moggyt` and provides a standard desktop
entry. Submission to the Arch User Repository (AUR) is scheduled following public release stabilization.

## Code Signing Policy

In version 1, binaries are not digitally signed with commercial authenticode or enterprise code signing certificates:

- **Windows**: Microsoft Defender SmartScreen may display an unrecognized application warning on first launch. Users
  must select "More info" followed by "Run anyway".
- **Linux**: packages rely on standard distribution packaging mechanisms without supplementary signing layers.

## Update Check Workflow

`moggyt` includes an unauthenticated update checker (FR-12):

1. The client queries the public GitHub Releases endpoint
   (`https://api.github.com/repos/kveld9/moggyt/releases/latest`) on user request.
2. If a newer semantic version is returned, the UI renders an update banner with a direct link to the GitHub download
   page.
3. Users download and install the package manually. To protect user autonomy and system stability, automatic silent
   background installations are intentionally omitted in v1 and deferred to v2.
4. During pre-launch development on private repositories, unauthenticated queries return 404 and the client soft-fails
   safely to `CheckFailed` without disrupting playback (see [ADR-001](adr/001-update-check-releases.md)).
