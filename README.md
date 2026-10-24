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
