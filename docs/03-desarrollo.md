# 03 - Desarrollo y verificacion

## Toolchain

- Rust stable (`rustup default stable`), componentes `rustfmt` y `clippy`.
- `Cargo.lock` se commitea: es la fuente unica de verdad de versiones reproducibles.

## Comandos

```bash
cargo run                                  # ejecuta la app (ventana eframe)
cargo test --all-features                  # suite completa
cargo fmt --all -- --check                 # formato (CI falla si difiere)
cargo clippy --all-targets --all-features -- -D warnings
./scripts/check-repo.sh                    # auditoria de estandares del repo
```

## Flujo de ramas y commits

- Rama por defecto `main`. Todo cambio entra por Pull Request desde `<type>/<descripcion-corta>`
  (`feat/`, `fix/`, `refactor/`, `docs/`, `test/`, `chore/`).
- Commits en ingles, imperativo, formato Conventional Commits: `feat(player): add repeat-one mode`.
- Un cambio logico por commit; PRs < 400 lineas; docs actualizados si cambia comportamiento, flags o config.

## TDD (obligatorio para `domain/`)

1. Prueba roja sobre el contrato publico (comportamiento observable, sin estado privado).
2. Implementacion minima hasta verde.
3. Refactor sobre verde (complejidad y nombres), re-ejecutando la suite.

Los tests usan fakes en memoria (colas y catalogos falsos), nunca la red ni el audio real.
Tiempo y aleatoriedad se inyectan; prohibido `sleep` o flakiness por reloj.

## Pre-commit local (recomendado)

```bash
cp .githooks/pre-commit.example .githooks/pre-commit
chmod +x .githooks/pre-commit
git config core.hooksPath .githooks
```
