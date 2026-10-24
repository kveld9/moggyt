# 02 - Architecture

Minimal hexagonal architecture, aligned with the `principios-git` (repo governance) and
`principios-codigo` (design, TDD, modularity) kits.

## Dependency direction

```text
UI (app.rs, eframe) --> domain (domain/) <-- ports <-- adapters (infra/)
```

- `domain/`: immutable types and pure rules. Direct `std::net`, `std::fs`, `std::process`, clocks, and randomness are forbidden.
- `infra/`: implements the ports with real I/O (HTTP to YouTube Music, audio backend).
- `main.rs` (bin) is the only *composition root*: builds the `tokio` runtime,
  instantiates adapters, and injects dependencies into `app.rs`.
- Time and randomness are injected as parameters or fake clocks in tests (determinism, zero `sleep` in tests).

## Module map

```text
src/
├── main.rs            # Composition root (bin): tracing, tokio runtime, eframe::run_native
├── lib.rs             # Public contracts: app + domain + infra
├── app.rs             # UI shell: navigation state, delegates events to the domain
├── domain/
│   ├── mod.rs         # Re-exports the public domain contract
│   ├── queue.rs       # Pure playback queue (add, next, previous, shuffle)
│   └── search.rs      # Search models (immutable query, paginated results)
└── infra/
    ├── mod.rs         # Re-exports adapters
    ├── ytm.rs         # Catalog adapter (stub: returns fixed data until the API lands)
    └── audio.rs       # Audio adapter (stub: records intent without playing yet)
```

## Evolution rules

1. One concept, one owner: each domain type lives in a single module (single source of truth).
2. Acyclic graph: `domain` never depends on `infra` or `app`; `infra` never depends on `app`.
3. New adapters (real API, `rodio`/`gstreamer`) are added behind the existing port, never touching the domain.
4. Cyclomatic complexity CC <= 5 per function; guard clauses first; DRY only from the third repetition.
