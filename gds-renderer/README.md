# gds-renderer

Lógica GDS de Riku sobre `gdstk-rs`: escena de dibujo, diff geométrico, paletas por PDK, render SVG y el backend neutro que usa `riku-gui`. `riku` (CLI) y `riku-gui` no dependen de gdstk directamente: todo pasa por este crate.

## API pública

| Tema | Funciones / tipos |
|---|---|
| Diff | `diff_gds`, `diff_gds_with_config` (librería completa: celdas añadidas/removidas + XOR por celda y capa), `diff_cell` (una celda, con los polígonos del XOR), `changed_cells` (qué celdas cambiaron, incluidos cambios heredados de sub-celdas) |
| Tipos del diff | `GdsDiffReport`, `GdsGeomDiff` (áreas µm², bbox, origen, `cosmetic`), `CellDiff`, `LayerPolygons`, `CellChange`, `DiffConfig` (`cosmetic_threshold_um2`) |
| Escena | `scene_from_cell` (labels de la celda), `scene_from_cell_in` (labels de toda la jerarquía), `flatten_labels`, `select_top_cell` |
| Visor | `GdsBackend` (`viewer_core::ViewerBackend`: `load`, `load_entry` por celda, `load_diff`), `list_cells` |
| SVG | `render_scene`, `render_scene_with_highlights`, `render_cell` |

Un lado vacío en el diff (0 bytes: el archivo no existía en ese commit) cuenta como librería vacía.

## Paletas por PDK

`src/palette.rs` tiene tablas de SKY130, GF180MCU e IHP SG13G2 con nombre, color y **rol** de cada capa:

- **Device** — difusión, poly, contactos, metales: relleno visible.
- **Well** — pozos: tinte tenue.
- **Outline** — implantes, marcadores, boundaries, pines, labels: solo contorno (cubren celdas enteras y rellenos taparían todo).

El orden de cada tabla es el de apilado físico. Los colores de GF180 e IHP salen de sus `.lyp` oficiales (`libs.tech/klayout/tech`). Las capas fuera de tabla toman el rol según la convención de datatypes de cada PDK. `detect_pdk` usa la ruta del archivo y, si no alcanza, las capas presentes.

## Estructura

```
src/
├── gds_diff.rs            diff por celda/capa, changed_cells
├── hier_walk.rs           atribución de origen (qué sub-celda aportó un cambio)
├── labels.rs              labels de la jerarquía con transformaciones de instancias
├── palette.rs             paletas PDK, roles, detect_pdk
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
