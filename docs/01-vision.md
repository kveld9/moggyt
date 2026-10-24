# 01 - Vision y alcance

## Problema

Los clientes web de YouTube Music consumen mucha memoria y dependen del navegador.
`moggyt` es un cliente de escritorio nativo, liviano y rapido, escrito en Rust.

## Alcance v0.1 (scaffold actual)

- Ventana nativa `eframe`/`egui` con navegacion minima (busqueda, cola, ahora suena).
- Nucleo de dominio puro: cola de reproduccion y modelos de busqueda, sin I/O.
- Puertos (`trait`) para el catalogo de YouTube Music y el backend de audio; adaptadores reales en fases posteriores.

## Fuera de alcance (por ahora)

- Descargas offline y gestion de biblioteca local.
- Casting a dispositivos remotos.
- Empaquetado para tiendas (Flatpak, MSI, dmg); solo binarios `cargo build --release` y releases de GitHub.

## Principios rectores

1. Lo nativo primero: arranque rapido, binario unico, sin runtime web.
2. Dominio puro y comprobable: la logica de cola y busqueda se prueba sin red ni audio.
3. Cambio minimo por PR: cada integracion (API, audio, UI) entra en pasos atomicos y revisables (< 400 lineas).
