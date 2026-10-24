# 02 - Arquitectura

Arquitectura hexagonal minima, alineada con los kits `principios-git` (gobierno del repo) y
`principios-codigo` (diseno, TDD, modularidad).

## Direccion de dependencias

```text
UI (app.rs, eframe) --> dominio (domain/) <-- puertos <-- adaptadores (infra/)
```

- `domain/`: tipos inmutables y reglas puras. Prohibido `std::net`, `std::fs`, `std::process`, relojes y aleatoriedad directos.
- `infra/`: implementa los puertos con I/O real (HTTP a YouTube Music, backend de audio).
- `main.rs` (bin) es el unico *composition root*: construye el runtime `tokio`,
  instancia adaptadores e inyecta dependencias en `app.rs`.
- El tiempo y la aleatoriedad se inyectan como parametros o relojes falsos en tests (determinismo, cero `sleep` en pruebas).

## Mapa de modulos

```text
src/
├── main.rs            # Composition root (bin): tracing, tokio runtime, eframe::run_native
├── lib.rs             # Contratos publicos: app + domain + infra
├── app.rs             # Shell de UI: estado de navegacion, delega eventos al dominio
├── domain/
│   ├── mod.rs         # Re-exporta el contrato publico del dominio
│   ├── queue.rs       # Cola de reproduccion pura (agregar, siguiente, anterior, mezclar)
│   └── search.rs      # Modelos de busqueda (query inmutable, resultados paginados)
└── infra/
    ├── mod.rs         # Re-exporta adaptadores
    ├── ytm.rs         # Adaptador del catalogo (stub: retorna datos fijos hasta integrar la API)
    └── audio.rs       # Adaptador de audio (stub: registra intencion sin reproducir aun)
```

## Reglas de evolucion

1. Un concepto, un dueno: cada tipo de dominio vive en un solo modulo (fuente unica de verdad).
2. Grafo aciclico: `domain` nunca depende de `infra` ni de `app`; `infra` nunca depende de `app`.
3. Nuevos adaptadores (API real, `rodio`/`gstreamer`) se agregan tras el puerto existente, nunca tocando el dominio.
4. Complejidad ciclomática CC <= 5 por funcion; guard clauses al inicio; DRY solo desde la tercera repeticion.
