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

1. Read [Vision and Scope](01-vision.md) and [Architecture](02-architecture.md) to understand modular boundaries.
2. Read [Software Design Document (SDD)](04-sdd.md) to inspect technical contracts and functional requirements.
3. Follow [Development and Verification](03-development.md) for local verification gates and coding rules.
4. Review [ADR-001](adr/001-update-check-releases.md) for current update check distribution decisions.
