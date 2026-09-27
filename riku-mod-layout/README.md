# riku-mod-layout

Módulo de layouts de Riku (GDSII, OASIS y Magic) sobre `gdstk-rs`: diff geométrico con cache, paletas por PDK y el backend del visor. El resto de `riku` no usa gdstk directamente: todo pasa por este crate, que `riku/src/modules/layout.rs` registra como módulo de formato.

## API pública

| Tema | Funciones / tipos |
|---|---|
| Diff | `diff_gds`, `diff_gds_with_config` (librería completa: celdas añadidas/removidas/renombradas + XOR por celda y capa, instancias agrupadas), `diff_gds_cached` (ídem con `DiffCache`), `diff_layout_sides` (cualquier formato, con los archivos de cada versión: lo que usa Riku), `diff_libraries` (dos `Library` ya leídas), `diff_cell` / `diff_cell_as` (una celda, con los polígonos del XOR y un item por instancia), `changed_cells` (qué celdas cambiaron, incluidos cambios heredados de sub-celdas y renombres), `is_layout` |
| Cache | `DiffCache` (`from_env`, `disabled`, `at`): resultados en `~/.cache/riku/diff` para layouts de más de 1 MiB |
| Magic | `mag::collect` (la jerarquía de un `.mag`: sub-celdas del mismo commit, del disco y del PDK), `mag::build` (lambda por tecnología), `mag::port_changes`, `mag::pdk_libraries` |
| Tipos del diff | `GdsDiffReport` (con `ports` en Magic), `GdsGeomDiff` (áreas µm², bbox, origen, `layer_name`, `instance_at_um`, `instances`, `cosmetic`), `CellDiff`, `LayerPolygons`, `CellChange`, `DiffConfig` (`cosmetic_threshold_um2`) |
| Escena | `draw_commands` (polígonos y labels de toda la jerarquía de una celda), `flatten_labels`, `select_top_cell` |
| Visor | `GdsBackend` (`viewer_core::ViewerBackend`: `load`, `load_entry` por celda, `load_diff`, y `load_with` / `load_diff_with` con los archivos de cada versión), `list_cells` |

Lee GDSII, OASIS y Magic (el formato se elige por la firma del archivo; Magic, con sus sub-celdas: ver [`docs/layouts.md`](../docs/layouts.md#magic-mag)). Un lado vacío en el diff (0 bytes: el archivo no existía en ese commit) cuenta como librería vacía.

## Paletas por PDK

`src/palette.rs` tiene tablas de SKY130, GF180MCU e IHP SG13G2 con nombre, color y **rol** de cada capa:

- **Device** — difusión, poly, contactos, metales: relleno visible.
- **Well** — pozos: tinte tenue.
- **Outline** — implantes, marcadores, boundaries, pines, labels: solo contorno (cubren celdas enteras y rellenos taparían todo).

El orden de cada tabla es el de apilado físico. Los colores de GF180 e IHP salen de sus `.lyp` oficiales (`libs.tech/klayout/tech`). El resto de las capas de GF180 e IHP sale de `src/palette_generated.rs`, generado desde los `.lyp` con `tools/palettes/gen_palettes.py`; las tablas curadas mandan. Las capas que no están en ninguna toman el rol según la convención de datatypes de cada PDK. `detect_pdk` usa la ruta del archivo y, si no alcanza, las capas presentes.

Las capas de Magic (`metal1`, `viali`) toman el color y el apilado de su capa GDS equivalente del PDK (`magic_layer_spec`), según su plano y, en los contactos, el plano que conectan; la tabla sale de los `.tech` con `tools/palettes/gen_magic_layers.py` (`src/magic_layers_generated.rs`). El PDK de un `.mag` es el que conoce más nombres de sus capas (`magic_pdk`).

## Estructura

```
src/
├── gds_diff.rs            diff por celda/capa, changed_cells, renombres
├── diff_cache.rs          cache en disco de diffs de layouts grandes
├── hier_walk.rs           atribución de origen (qué instancia de sub-celda aportó un cambio)
├── labels.rs              labels de la jerarquía con transformaciones de instancias
├── mag.rs                 Magic: sub-celdas (commit, disco, PDK), lambda, puertos
├── magic_layers_generated.rs  plano de cada capa de Magic por PDK (generado)
├── palette.rs             paletas PDK curadas, roles, detect_pdk
├── palette_generated.rs   capas completas de GF180 e IHP (generado)
├── viewer_core_compat.rs  GdsBackend: escenas, catálogo de celdas, escena de diff
├── scene.rs               comandos de dibujo de una celda (polígonos + labels)
├── style.rs               Color y Pdk
└── top_cell.rs            elección determinista de top cell
```

## Tests

```bash
cargo test -p riku-mod-layout
```

Los fixtures (`tests/fixtures/*.gds`) se generan con los scripts Python de la misma carpeta (gdstk). La geometría, los labels y el XOR se verificaron contra KLayout sobre las librerías de celdas estándar de los tres PDKs: ver [`docs/layouts.md`](../docs/layouts.md).
