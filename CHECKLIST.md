# Checklist de moggyt (derivada de principios-git + principios-codigo)

Auditoria rapida del repo. Marcar solo lo verificado con evidencia (comando verde, diff revisado).

## Nivel 1: Base legal y estructural

- [x] **Licencia (`LICENSE`)**: MIT, owner y year sustituidos.
- [x] **Portal (`README.md`)**: problema, quickstart < 5 min y layout documentados.
- [x] **`.gitignore`**: cubre `target/`, `.env`, secretos, SO e IDEs.
- [x] **`.gitattributes`**: `* text=auto eol=lf`, `Cargo.lock` como generado.
- [x] **`.editorconfig`**: incluye seccion `[*.rs]` (4 espacios).
- [x] **Rama `main`** como rama por defecto.

## Nivel 2: Colaboracion y calidad

- [x] **`CONTRIBUTING.md`**: ramas, Conventional Commits, comandos cargo.
- [x] **`SECURITY.md`**: canal privado, sin issues publicos de dia cero.
- [x] **`CODE_OF_CONDUCT.md`**: contacto definido.
- [x] **Templates**: bug report, feature request y PR template presentes.
- [x] **Sin emojis en ejecucion**: CI lo verifica en `src/`, `scripts/`, configs y workflows.
- [x] **CI basica**: fmt, clippy `-D warnings`, tests y markdownlint en cada push/PR.
- [x] **Dependabot**: grupos para `github-actions` y `cargo`.
- [x] **Gitleaks**: escaneo de secretos con historial completo.
- [x] **Hooks locales**: `.githooks/pre-commit.example` + `core.hooksPath` documentado.

## Codigo (principios-codigo, fases)

- [x] **Diseno**: dominio puro sin I/O, puertos en `infra/`, composition root en `main.rs`, grafo aciclico.
- [x] **Implementacion**: CC <= 5 objetivo, guard clauses, nombres en ingles, cero secretos en codigo, `Cargo.lock` commiteado.
- [x] **Tests**: unit tests de `domain` (cola, busqueda) y `app` (fakes en memoria, deterministas, sin `sleep`).
- [ ] **Refactor y revision**: PRs < 400 lineas, auto-revision con diff propio, severidad explicita en comentarios.

## Pendiente (fases siguientes, no bloquear el scaffold)

- [ ] Adaptador real de YouTube Music tras el puerto `Catalog` (PR dedicada).
- [ ] Backend de audio real tras el puerto `AudioBackend` (PR dedicada).
- [ ] Activar rulesets en GitHub segun `.github/settings.example.yml` (1 approval, checks estrictos, tags `v*` inmutables).
- [ ] Pipeline de release con checksums + atestacion Sigstore (ver `release.yml.example`).
