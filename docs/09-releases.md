# Releases and Packaging

This document outlines the release delivery pipeline, distribution packages, update mechanics, and security guarantees
for `moggyt`.

## Build and CI Pipeline

Release automation runs across standard GitHub Actions runners:

- **Linux**: built on `ubuntu-latest` with standard ALSA, Ayatana AppIndicator, and CMake build dependencies.
- **Windows**: built on `windows-latest` targeting 64-bit Windows (`x86_64-pc-windows-msvc`).
- **Reproducibility**: `Cargo.lock` is tracked in version control and acts as the single source of truth for all crate
  dependencies across development and release pipelines.

## Release Procedure

Releases are triggered exclusively by pushing an explicit Semantic Versioning Git tag. Merging pull requests into
`main` never publishes a release.

To cut a new release:

```bash
git tag vX.Y.Z && git push origin vX.Y.Z
```

The release pipeline validates the tag format (`^v[0-9]+\.[0-9]+\.[0-9]+$`), derives the package version, builds
distribution artifacts across platforms, generates SHA-256 checksums, and publishes the release to GitHub Releases.

### Version Reported by the Binary

`Cargo.toml` keeps a static placeholder version. The version the running binary reports, in the About panel, the
update check, and outbound user agents, comes from the pushed tag instead: `build.rs` resolves `MOGGYT_VERSION`
and injects it at compile time.

Resolution order, first match wins:

1. `MOGGYT_VERSION`, set by the release workflow from the tag and by the Arch source PKGBUILD from `pkgver`.
1. The newest reachable `v*` tag, for local development builds.
1. `CARGO_PKG_VERSION`, for source archives that carry no git history.

The `v` prefix is stripped, so a `v1.4.0` tag reports `1.4.0` and the update check never offers the running
release as an update. Release builds must set `MOGGYT_VERSION`, because CI checkouts are shallow and carry no
tags.

### Arch Linux Packaging Bump

After the release workflow finishes publishing artifacts, update the Arch Linux package definitions:

1. Set `pkgver` to the new version in `packaging/arch/moggyt-bin/PKGBUILD` and `packaging/arch/moggyt/PKGBUILD`.
1. Reset `pkgrel=1` in both files.
1. Run `updpkgsums` in each directory so the checksum hashes match the published artifacts:

```bash
(cd packaging/arch/moggyt-bin && updpkgsums)
(cd packaging/arch/moggyt && updpkgsums)
```

1. Commit the updated PKGBUILDs and open a PR.

When this step is skipped, `makepkg -si` silently installs the previous release because `pkgver` remains pinned to
the older release.

## Distribution Artifacts

Release assets by shipment status:

| Artifact | Target OS | Distribution Channel | Status |
| --- | --- | --- | --- |
| `moggyt-*-linux.tar.gz` | Linux (Generic tarball) | GitHub Releases (consumed by Arch PKGBUILD) | Shipped |
| `moggyt-*-windows.zip` | Windows 64-bit | GitHub Releases | Shipped |
| `moggyt_*_amd64.deb` | Linux (Debian, Ubuntu) | GitHub Releases | Shipped |
| `moggyt-*-x86_64.AppImage` | Linux | GitHub Releases | Shipped |
| `moggyt-*-setup.exe` | Windows 64-bit | GitHub Releases (NSIS installer) | Planned |

The Linux release tarball now also ships `LICENSE`, the desktop entry, the hicolor launcher icons, and the Matugen
theme template alongside `THIRD_PARTY_NOTICES.md`. This payload applies to tarballs built after v1.4.0 (the v1.4.0
tarball predates it and carries the binary plus third-party notices only).

### Arch Linux Packaging

The repository maintains two Arch Linux package specifications:

- `packaging/arch/moggyt-bin/PKGBUILD` downloads and repackages the official pre-compiled Linux release tarball.
- `packaging/arch/moggyt/PKGBUILD` builds the tagged source archive with `cargo build --release --frozen`.

`moggyt` installs them from the tag archive (single source); `moggyt-bin` takes the binary and third-party notices from
the release tarball and the remaining assets from the matching source archive, so no commit pin is involved. Both
install `/usr/bin/moggyt`, the desktop entry, the hicolor launcher icons (16 to 256 px), the Matugen theme template at
`/usr/share/moggyt/matugen/moggyt-theme.json`, and the license files under `/usr/share/licenses/`. Both are built and
installed locally from a clone; the project does not publish to any package archive.

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
