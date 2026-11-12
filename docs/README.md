# Documentation Index

This directory contains technical and user documentation for `moggyt`, a native YouTube Music desktop client written
in Rust (Slint).

## Document Directory

| Document | Purpose |
| --- | --- |
| [Vision and Scope](01-vision.md) | High-level goals, guiding principles, and scope boundaries |
| [Architecture](02-architecture.md) | Hexagonal architecture, module map, port boundaries, and dependency rules |
| [Development and Verification](03-development.md) | Toolchain setup, verification gates, TDD flow, and branch conventions |
| [Software Design Document (SDD)](04-sdd.md) | Comprehensive v1 technical design, reference analysis, and requirements |
| [User Guide](05-user-guide.md) | Interface navigation, transport controls, queue operations, and library sign-in |
| [Installation](06-installation.md) | Building from source, release artifacts, package managers, and runtime setup |
| [Configuration](07-configuration.md) | Platform data paths, cover cache, credential storage, and logging filters |
| [Troubleshooting](08-troubleshooting.md) | Diagnostic table mapping common symptoms to underlying causes and solutions |
| [Releases and Packaging](09-releases.md) | CI build pipelines, distribution artifacts, signing status, and update checks |
| [Architecture Decision Records (ADRs)](adr/README.md) | Record of architectural and technical trade-off decisions |

## Recommended Reading Order

### For New Users

1. Read [README.md](../README.md) for quickstart and feature overview.
2. Read [Installation](06-installation.md) for build options and platform prerequisites.
3. Read [User Guide](05-user-guide.md) for application features and playback controls.
4. Consult [Troubleshooting](08-troubleshooting.md) if audio, cover art, or stream resolution encounters issues.

### For Contributors

#### Onboarding Learning Tracks

- **Track 1: Day 1 Developer Setup and First Build**
  1. Install system dev headers and toolchain per [CONTRIBUTING.md](../CONTRIBUTING.md).
  2. Follow [Development and Verification](03-development.md) to run the 6 local CI verification gates.
  3. Install the recommended pre-commit hook (`cp .githooks/pre-commit.example .githooks/pre-commit`).
  4. Launch the application window via `cargo run`.

- **Track 2: Hexagonal Architecture and Data Flow**
  1. Read [Vision and Scope](01-vision.md) to understand project principles and non-goals.
  2. Study [Architecture](02-architecture.md) for the pure domain skeleton, module map, and port contracts.
  3. Review the "Anatomy of a feature" walkthrough in [Development and Verification](03-development.md) to
     follow how Slint UI callbacks traverse async channels to pure domain logic and back.
  4. Review [Software Design Document (SDD)](04-sdd.md) for detailed contracts across core subsystems.

- **Track 3: UI Development and Slint Guidelines**
  1. Review the UI verification and Slint lessons in [Development and Verification](03-development.md).
  2. Review Slint components in `ui/shell.slint`, `ui/views/`, and `ui/components/`.
  3. Learn layout rules (avoiding collapsing `Flickable`, centering children, and thread safety).
  4. Understand UI chrome translation invariants (`src/i18n.rs`).

- **Architecture Decisions**
  - Consult [ADR-001](adr/001-update-check-releases.md) for update check and release distribution rationale.
