# riku-mod-layout

Módulo de layouts de Riku (GDSII, OASIS y Magic) sobre `gdstk_rust`: diff geométrico con cache, estilo de capas por PDK y el backend del visor. El resto de `riku` no usa gdstk directamente: todo pasa por este crate, que `riku/src/modules/layout.rs` registra como módulo de formato. Qué compara: [`docs/formatos.md`](../docs/formatos.md#layouts-gdsii-oasis-y-magic).

## API pública

Solo lo que usa `riku`; el resto es interno al crate.

| Tema | Funciones / tipos |
|---|---|
| Diff | `diff_layout_sides` (cualquier formato, con los archivos de cada versión), `diff_cell` (una celda, con los polígonos del XOR), `is_layout` |
| Tipos del diff | `GdsDiffReport`, `GdsGeomDiff`, `CellDiff`, `LayerPolygons`, `CellChange`, `LayerKey`, `BBoxUm`, `DiffConfig`, `GdsError`, `LayoutSide` |
| Cache | `DiffCache` (`from_env`, `disabled`, `at`): `~/.cache/riku/diff`, layouts de más de 1 MiB |
| Magic | `mag::collect`, `mag::build`, `mag::port_changes`, `mag::pdk_libraries` |
| PDK y visor | `pdk_tech`, `GdsBackend` (`viewer_core::ViewerBackend`), `flatten_labels` |

## Estilo de las capas

Cada capa tiene nombre, color y rol (**Device**: relleno; **Well**: tinte tenue; **Outline**: implantes, marcadores, pines, solo contorno). Todo sale de `Process` (`src/process.rs`), que junta, de más a menos prioridad:

1. las **tablas curadas** de SKY130, GF180MCU e IHP SG13G2 (`src/palette.rs`), en orden de apilado;
2. el **`.lyp` del PDK instalado** (`src/pdk_tech.rs`, de `$PDK_ROOT`/`$PDKPATH`); sin PDK, la tabla generada (`src/palette_generated.rs`, con `tools/palettes/gen_palettes.py`);
3. para el resto, la paleta genérica y el rol por la convención de datatypes.

Las capas de Magic van a su capa GDS equivalente (`src/magic_layers_generated.rs`, con `tools/palettes/gen_magic_layers.py`, o el `cifoutput` del `.tech` del PDK), y el lambda de un `.mag` sale del `scalefactor` de ese `.tech`. Un PDK nuevo se dibuja sin recompilar.

## Tests

`cargo test -p riku-mod-layout`. Los fixtures (`tests/fixtures/`) se generan con los scripts Python de esa carpeta; la verificación contra KLayout está en [`tools/verify/`](../tools/verify/README.md).
