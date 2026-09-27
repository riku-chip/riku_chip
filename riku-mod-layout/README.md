# riku-mod-layout

Módulo de layouts de Riku (GDSII, OASIS y Magic) sobre `gdstk-rs`: diff geométrico con cache, paletas por PDK y el backend del visor. El resto de `riku` no usa gdstk directamente: todo pasa por este crate, que `riku/src/modules/layout.rs` registra como módulo de formato.

## API pública

Solo lo que usa `riku`; el resto es interno al crate.

| Tema | Funciones / tipos |
|---|---|
| Diff | `diff_layout_sides` (cualquier formato, con los archivos de cada versión: celdas añadidas/removidas/renombradas, XOR por celda y capa con instancias agrupadas, puertos de Magic), `diff_cell` (una celda, con los polígonos del XOR), `is_layout` |
| Tipos del diff | `GdsDiffReport`, `GdsGeomDiff` (áreas µm², bbox, origen, `layer_name`, `instance_at_um`, `instances`, `cosmetic`), `CellDiff`, `LayerPolygons`, `CellChange`, `LayerKey`, `BBoxUm`, `DiffConfig`, `GdsError`, `LayoutSide` |
| Cache | `DiffCache` (`from_env`, `disabled`, `at`): resultados en `~/.cache/riku/diff` para layouts de más de 1 MiB |
| Magic | `mag::collect`, `mag::build` (lambda de la tecnología), `mag::port_changes`, `mag::pdk_libraries` |
| PDK | `pdk_tech` (lo leído de los PDK instalados: `.lyp` y `.tech` de Magic) |
| Visor | `GdsBackend` (`viewer_core::ViewerBackend`), `flatten_labels` |

Lee GDSII, OASIS y Magic (el formato se elige por la firma del archivo; Magic, con sus sub-celdas: ver [`docs/layouts.md`](../docs/layouts.md#magic-mag)). Un lado vacío en el diff (0 bytes: el archivo no existía en ese commit) cuenta como librería vacía.

## Estilo de las capas y PDK

Cada capa tiene nombre, color y **rol**:

- **Device** — difusión, poly, contactos, metales: relleno visible.
- **Well** — pozos: tinte tenue.
- **Outline** — implantes, marcadores, boundaries, pines, labels: solo contorno (cubren celdas enteras y rellenos taparían todo).

Todo sale de un solo modelo, `Process` (`src/process.rs`), que la escena y el diff consultan. Junta, de más a menos prioridad:

1. las **tablas curadas** de SKY130, GF180MCU e IHP SG13G2 (`src/palette.rs`), en orden de apilado físico;
2. el **`.lyp` del PDK instalado** (`src/pdk_tech.rs`, leído de `$PDK_ROOT`/`$PDKPATH` al ejecutar); sin PDK instalado, la tabla generada de su `.lyp` (`src/palette_generated.rs`, con `tools/palettes/gen_palettes.py`);
3. para lo que nadie conoce, un color de la paleta genérica y el rol por la convención de datatypes del PDK.

Las capas de Magic (`metal1`, `viali`) van a su capa GDS y toman su estilo: la equivalente curada (por plano y, en los contactos, el plano que conectan; `src/magic_layers_generated.rs`, con `tools/palettes/gen_magic_layers.py`) o, para cualquier otro PDK, la que escribe el `cifoutput` de su `.tech`. El lambda de un `.mag` sale del `scalefactor` de ese `.tech`.

El PDK de un layout: el compilado que reconocen sus capas o su ruta (`detect_pdk`, `magic_pdk`) y el instalado que más capas suyas conoce, de la misma familia. Un PDK nuevo se dibuja sin recompilar.

## Estructura

```
src/
├── source.rs              lectura de un lado (GDSII/OASIS o jerarquía Magic) y clave de cache; la usan CLI y visor
├── gds_diff.rs            diff por celda/capa, changed_cells, renombres
├── prints.rs              huellas por capa y XOR guiado por ellas, en paralelo
├── hier_walk.rs           atribución de origen (qué instancia de sub-celda aportó un cambio)
├── box_grid.rs            grilla de bboxes (consultas rápidas de solapamiento)
├── diff_cache.rs          cache en disco de diffs de layouts grandes
├── labels.rs              labels de la jerarquía con transformaciones de instancias
├── mag.rs                 Magic: sub-celdas (commit, disco, PDK), lambda, puertos
├── process.rs             Process: estilo de cada capa (compilado + PDK instalado)
├── palette.rs             tablas compiladas: capas curadas, roles, detect_pdk, capas de Magic
├── pdk_tech.rs            PDK instalados: .lyp y .tech de Magic
├── palette_generated.rs   capas completas de GF180 e IHP (generado)
├── magic_layers_generated.rs  plano de cada capa de Magic por PDK (generado)
├── layer_style.rs         clave y pintura de cada capa de una escena
├── viewer_core_compat.rs  GdsBackend: carga, escena de una celda, catálogo de celdas
├── diff_scene.rs          escena de diff: overlay del XOR y lista de cambios
├── style.rs               Color y Pdk
└── top_cell.rs            elección determinista de top cell
```

## Tests

```bash
cargo test -p riku-mod-layout
```

Los fixtures (`tests/fixtures/*.gds`) se generan con los scripts Python de la misma carpeta (gdstk). La geometría, los labels y el XOR se verificaron contra KLayout sobre las librerías de celdas estándar de los tres PDKs: ver [`docs/layouts.md`](../docs/layouts.md).
