# Roadmap

Qué está hecho, qué sigue y qué queda abierto. Esfuerzo: **S** = horas, **M** = 1–2 días, **L** = varios días.

**Última revisión:** 2026-09-27.

## Fases

| Fase | Qué | Estado |
|---|---|---|
| 0 | Ejecutable único `riku` (CLI + shell + visor), workspace, `.tar.gz`/`.deb` | hecha |
| 1 | `riku-kernel` con tipos de cambio propios; JSON `riku-diff/v2` (la forma v1 se quitó en la fase 9: todo es v2) | hecha |
| 2 | El núcleo deja de conocer formatos: cada módulo detecta y compara | hecha |
| 3 | Registro único de módulos (`FormatModule`, `modules/mod.rs`), features por módulo | hecha |
| 4 | Una sola ruta del visor para `.sch` y `.gds` (fantasmas y anotaciones en la escena) | hecha |
| 5 | `gds-renderer` → `riku-mod-layout` sin el render SVG; `riku show`; `--ci` | hecha |
| 6 | Rendimiento con layouts grandes y multinúcleo — [`diseno/fase6.md`](diseno/fase6.md) | hecha |
| 7 | Grafo del historial: `riku log --graph` (Unicode, colores) y panel **Historial** en el visor — [`diseno/fase7.md`](diseno/fase7.md) | hecha: motor, `log --graph` y panel **History** del visor |
| 8 | Módulo Magic (`.mag`): lector en gdstk-rs, jerarquía entre archivos del mismo commit, capas con nombre, puertos — [`diseno/fase8.md`](diseno/fase8.md) | hecha: igual a KLayout 0.30.12 en 8 jerarquías de los PDK; los 9 281 `.mag` de los PDK se leen |
| 9 | Revisión del proyecto: 10 bugs, rendimiento (CPU/RAM, índices y grafo de celdas), estructura (SOLID sin sobreingeniería, microkernel) y librerías — [`diseno/fase9.md`](diseno/fase9.md) | plan |
| — | Módulo `spice`: diff de formas de onda de ngspice (`.raw`) en la CLI y vista de curvas en el visor ([`spice.md`](spice.md)) | hecho |

Fases 0–5: plan en [`archivo/plan_migracion_microkernel.md`](archivo/plan_migracion_microkernel.md).

### Fase 6, por pasos

| Paso | Qué | Estado |
|---|---|---|
| 6.1 + 6.5.a | Huella por capa en forma canónica y XOR solo de lo que cambió | hecho: diff de 42 MB de ~22 min a 6,4 s, igual a KLayout |
| 6.2 | Visor: índice espacial, nivel de detalle, relleno en cache | hecho: 11 GB → 2,5 GB, ~600 ms → ~2 ms por cuadro |
| 6.3 | gdstk-rs seguro entre hilos (normalizar paths al cargar, `Send`/`Sync`, test concurrente) | hecho: además, las capas dibujadas solo con paths entran al diff |
| 6.4 | Huella jerárquica, instancias gemelas, aplanado por pedazos y `rayon` en el diff (`--jobs`/`RIKU_JOBS`) | hecho: 42 MB sin cambios 5,5 s → 1,4 s; con cambios 14,6 s → 1,8–2,3 s; siempre < 1 GB |
| 6.5.b | XOR por cuadrantes (quadtree) para capas enormes que cambiaron enteras | hecho: 19/0 regenerada, Clipper de 358 s a 0,28 s; instancia movida 7,2 → 5,6 s (3,1 s con 12 hilos) |
| 6.6 | `log`, `show` y `status` en paralelo (una conexión a Git por hilo, tandas planificadas por memoria) | hecho: historial de 12 commits de layouts 1,25 s → 0,31 s; 4 diffs del chip de 42 MB 18,7 s → 8,4 s |

## Pendientes

### Alta

- ~~Publicar la primera versión~~: hecho, [`v0.1.0`](https://github.com/riku-chip/riku_chip/releases/tag/v0.1.0) (`.tar.gz`, `.deb`, `SHA256SUMS`; licencia Apache-2.0). Las siguientes: subir la versión en `riku/Cargo.toml` y crear el tag `vX.Y.Z`. [`v0.2.0`](https://github.com/riku-chip/riku_chip/releases/tag/v0.2.0): la fase 9 (bugs, rendimiento, pantalla de inicio y diff de todo el repo en el visor, todo el JSON en v2, librerías al día).

### Media

| Tema | Por qué | Dónde | Esfuerzo |
|---|---|---|---|
| Pestaña **Before** de una celda renombrada | Busca el nombre nuevo en la versión A, donde no existe (la vista **Diff** sí compara bien) | `gui/app.rs` (`select_diff_tab`), `GdsBackend::load_entry` | S |
| Renombres con cambios | Solo se detectan renombres puros; una celda renombrada que además cambió sale como baja + alta | `gds_diff.rs` (`detect_renames`): emparejar por bbox y huellas en común | M |
| Zoom cercano en zonas muy densas | Con píxeles más chicos que la celda más fina de la pirámide y cientos de miles de elementos a la vista, se dibuja todo uno a uno (~45 ms por cuadro) | `viewer-core/src/index.rs`: pirámide más fina con bitsets dispersos | M |

### Baja

| Tema | Por qué | Esfuerzo |
|---|---|---|
| Exportar SVG/PNG desde el visor | Un botón "Exportar…" en la GUI; la CLI ya lo hace (`riku render`, `-f png\|svg`) y el botón puede reusar `riku/src/export` | S |
| `cargo fmt` en la CI | El formato no es uniforme; activarlo es un commit grande solo de formato | S |
| Clippy en la CI | Primero como job no bloqueante | S |

### Limitaciones conocidas

- **Espaciado entre letras por tamaño:** egui 0.34 no permite ajustarlo.

### Decisiones pendientes (no son código)

- **`.agents/` y `skills-lock.json`** en la raíz: los creó el instalador de una skill del agente; falta decidir si se versionan o se ignoran.
- **Pruebas con usuarios reales:** la GUI se probó con clics y teclado simulados en una sola resolución; falta ver a diseñadores usándola en su flujo.
