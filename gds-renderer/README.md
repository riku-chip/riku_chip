# gds-renderer

Lógica GDS de Riku sobre `gdstk-rs`: escena de dibujo, diff geométrico, paletas por PDK, render SVG y el backend neutro que usa `riku-gui`. `riku` (CLI) y `riku-gui` no dependen de gdstk directamente: todo pasa por este crate.

## API pública

| Tema | Funciones / tipos |
|---|---|
| Diff | `diff_gds`, `diff_gds_with_config` (librería completa: celdas añadidas/removidas/renombradas + XOR por celda y capa, instancias agrupadas), `diff_gds_cached` (ídem con `DiffCache`), `diff_cell` / `diff_cell_as` (una celda, con los polígonos del XOR y un item por instancia), `changed_cells` (qué celdas cambiaron, incluidos cambios heredados de sub-celdas y renombres), `is_layout` |
| Cache | `DiffCache` (`from_env`, `disabled`, `at`): resultados en `~/.cache/riku/diff` para layouts de más de 1 MiB |
| Tipos del diff | `GdsDiffReport`, `GdsGeomDiff` (áreas µm², bbox, origen, `instance_at_um`, `instances`, `cosmetic`), `CellDiff`, `LayerPolygons`, `CellChange`, `DiffConfig` (`cosmetic_threshold_um2`) |
| Escena | `scene_from_cell` (labels de la celda), `scene_from_cell_in` (labels de toda la jerarquía), `flatten_labels`, `select_top_cell` |
| Visor | `GdsBackend` (`viewer_core::ViewerBackend`: `load`, `load_entry` por celda, `load_diff`), `list_cells` |
| SVG | `render_scene`, `render_scene_with_highlights`, `render_cell` |

Lee GDSII y OASIS (el formato se elige por la firma del archivo). Un lado vacío en el diff (0 bytes: el archivo no existía en ese commit) cuenta como librería vacía.

## Paletas por PDK

`src/palette.rs` tiene tablas de SKY130, GF180MCU e IHP SG13G2 con nombre, color y **rol** de cada capa:

- **Device** — difusión, poly, contactos, metales: relleno visible.
- **Well** — pozos: tinte tenue.
- **Outline** — implantes, marcadores, boundaries, pines, labels: solo contorno (cubren celdas enteras y rellenos taparían todo).

El orden de cada tabla es el de apilado físico. Los colores de GF180 e IHP salen de sus `.lyp` oficiales (`libs.tech/klayout/tech`). El resto de las capas de GF180 e IHP sale de `src/palette_generated.rs`, generado desde los `.lyp` con `tools/palettes/gen_palettes.py`; las tablas curadas mandan. Las capas que no están en ninguna toman el rol según la convención de datatypes de cada PDK. `detect_pdk` usa la ruta del archivo y, si no alcanza, las capas presentes.

## Estructura

```
src/
├── gds_diff.rs            diff por celda/capa, changed_cells, renombres
├── diff_cache.rs          cache en disco de diffs de layouts grandes
├── hier_walk.rs           atribución de origen (qué instancia de sub-celda aportó un cambio)
├── labels.rs              labels de la jerarquía con transformaciones de instancias
├── palette.rs             paletas PDK curadas, roles, detect_pdk
├── palette_generated.rs   capas completas de GF180 e IHP (generado)
├── viewer_core_compat.rs  GdsBackend: escenas, catálogo de celdas, escena de diff
├── compat.rs              escena desde una celda de gdstk
├── scene.rs · style.rs · viewport.rs · renderer.rs · output.rs   render SVG
└── top_cell.rs            elección determinista de top cell
```

## Tests

```bash
cd gds-renderer
cargo test
```

Los fixtures (`tests/fixtures/*.gds`) se generan con los scripts Python de la misma carpeta (gdstk). La geometría, los labels y el XOR se verificaron contra KLayout sobre las librerías de celdas estándar de los tres PDKs: ver `docs/integracion_gds_estado.md`.
