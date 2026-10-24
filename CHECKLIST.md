# moggyt checklist (derived from principios-git + principios-codigo)

Fast repo audit. Check only what is verified with evidence (green command, reviewed diff).

## Level 1: Legal and structural base

- [x] **License (`LICENSE`)**: MIT, owner and year replaced.
- [x] **Portal (`README.md`)**: problem, quickstart under 5 minutes, and layout documented.
- [x] **`.gitignore`**: covers `target/`, `.env`, secrets, OS and IDE files.
- [x] **`.gitattributes`**: `* text=auto eol=lf`, `Cargo.lock` as generated.
- [x] **`.editorconfig`**: includes `[*.rs]` section (4 spaces).
- [x] **`main` branch** as the default branch.

## Level 2: Collaboration and quality

- [x] **`CONTRIBUTING.md`**: branches, Conventional Commits, cargo commands.
- [x] **`SECURITY.md`**: private channel, no public zero-day issues.
- [x] **Templates**: bug report, feature request, and PR template present.
- [x] **`CODE_OF_CONDUCT.md`**: contact defined.
- [x] **No emojis in runtime**: CI verifies this in `src/`, `scripts/`, configs, and workflows.
- [x] **Basic CI**: fmt, clippy `-D warnings`, tests, and markdownlint on every push/PR.
- [x] **Dependabot**: groups for `github-actions` and `cargo`.
- [x] **Gitleaks**: secret scanning with full history.
- [x] **Local hooks**: `.githooks/pre-commit.example` + `core.hooksPath` documented.

## Code (principios-codigo, phases)

- [x] **Design**: pure domain without I/O, ports in `infra/`, composition root in `main.rs`, acyclic graph.
- [x] **Implementation**: CC <= 5 target, guard clauses, English names, zero secrets in code, committed `Cargo.lock`.
- [x] **Tests**: `domain` unit tests (queue, search) and `app` tests (deterministic in-memory fakes, no `sleep`).
- [ ] **Refactor and review**: PRs under 400 lines, self-review with the own diff, explicit severity in comments.

## Pending (later phases, not blocking the scaffold)

- [ ] Real YouTube Music adapter behind the `Catalog` port (dedicated PR).
- [ ] Real audio backend behind the `AudioBackend` port (dedicated PR).
- [ ] Enable rulesets on GitHub per `.github/settings.example.yml` (1 approval, strict checks, immutable `v*` tags).
- [ ] Release pipeline with checksums + Sigstore attestation (see `release.yml.example`).
