# Layouts GDSII y OASIS

Un esquemático se compara semánticamente (componentes, nets). Un layout es geometría: el diff útil es **qué área cambió, en qué capa y en qué celda**. Todo vive en el crate `riku-mod-layout`, sobre [`gdstk-rs`](https://github.com/Adriel2503/gdstk_rust) (binding de gdstk, submódulo `external/gdstk`). El resto de Riku no usa gdstk directamente.

## El diff

- **XOR por celda y capa** (layer/datatype), aplanando la jerarquía: un cambio dentro de una sub-celda aparece en cada celda que la instancia, con su origen (`TOP:L1/0:INV`) y el bbox en coordenadas de la celda comparada. Áreas en µm².
- **Cosmético:** un cambio con área total bajo `--cosmetic-threshold-um2` (0,01 µm² por defecto).
- **Instancias:** cada instancia (y cada repetición de un AREF) tiene su item y su recuadro en el visor (`met1 · en INV @ (10.00, 10.00)`); la CLI los agrupa (`en N instancias`) y el JSON trae `instances`.
- **Renombres:** una celda que desaparece y otra con la misma geometría que aparece son un renombre (`r cell:INV → INV_X1`), no baja + alta. Solo renombres puros; si hay varias candidatas no se adivina.
- **Librerías:** `changed_cells` dice qué celdas cambiaron, incluidos cambios heredados de sub-celdas. El visor las marca (`+` `−` `~`) y puede filtrar "solo con cambios".
- **OASIS:** el lector se elige por la firma del archivo, así que A y B pueden ser de formatos distintos.
- **Archivo nuevo o borrado:** un lado vacío es una librería vacía.
- **Cache:** en layouts de más de 1 MiB el resultado (reporte, celdas cambiadas, XOR de una celda) se guarda en `~/.cache/riku/diff` (tope 512 MiB). La clave incluye los bytes, los parámetros y la versión de `riku-mod-layout`. `--no-cache` o `RIKU_NO_CACHE=1` la desactivan.

### Cómo evita el trabajo inútil

1. **Huella por capa:** al aplanar cada celda se calcula un hash de cada polígono en **forma canónica** (vértices cuantizados, sin repetidos, sentido antihorario, empezando por el menor). Dos capas con la misma huella son iguales aunque el archivo las escriba distinto (reexportadas por otra herramienta): se saltan sin XOR.
2. **XOR solo de lo que cambió:** en una capa distinta, los polígonos idénticos de A y B se emparejan; Clipper recibe solo los propios de cada lado y los comunes que los tocan (`xor_split_owned` de gdstk-rs). El resultado es el mismo que el del XOR completo.

Un `user_project_wrapper` de 42 MB (IHP, 6,2 millones de polígonos) pasó de no terminar en 45 minutos a **6,4 s** (sin cambios reales) o **16 s** (con un cambio en capas de millones de polígonos), con áreas idénticas a KLayout. Diseño y mediciones en [`diseno/fase6.md`](diseno/fase6.md).

## El visor

Ver [`gui.md`](gui.md). Lo propio de layouts:

- **Paletas por PDK** (SKY130, GF180MCU, IHP SG13G2, de sus `.lyp` oficiales) con **rol de capa**: dispositivo (relleno), pozo (tinte tenue), implantes/marcadores/boundary/pines (solo contorno), en orden de apilado físico. El PDK se detecta por la ruta del archivo o por las capas presentes. Las tablas completas de GF180 e IHP se generan con `tools/palettes/gen_palettes.py`.
- **Labels** de toda la jerarquía, con su anchor.
- **Selector de celdas** con buscador y filtros; `riku gui archivo.gds --cell NOMBRE` abre una celda concreta.
- **Diff visual:** la vista **Diff** muestra la versión "después" atenuada con lo añadido en verde y lo eliminado en rojo; **Before** y **After** muestran cada versión. El panel **Cambios** lista por capa y origen; un clic encuadra el cambio.
- **Layouts grandes:** índice espacial y nivel de detalle; el de 42 MB abre con 2,5 GB y dibuja el chip completo en ~2 ms por cuadro.

## Verificación contra KLayout

Scripts en [`tools/verify/`](../tools/verify/README.md), con el módulo Python de KLayout 0.30: `compare.sh` vuelca cada librería con Riku (`examples/verify_dump.rs`) y con KLayout y compara los volcados.

| Qué | Dónde | Resultado |
|---|---|---|
| Geometría por celda: bbox, polígonos y área por capa, labels con posición | 437 celdas de `sky130_fd_sc_hd`, 230 de `gf180mcu_fd_sc_mcu7t5v0`, 78 de `sg13g2_stdcell` | idéntico |
| Ídem leyendo OASIS | `hier_inv_b.oas` | idéntico |
| XOR por capa | `inv_1` con met1 añadido, mcon borrado, poly movido 0,05 µm, licon movido 5 nm | idéntico |
| XOR jerárquico | met1 dentro de `inv_2` → `macro_sparecell`; AREF 3×2 | idéntico |
| Layout de 42 MB | reexportado (sin cambios) y con cambios en las capas 6/0 y 19/0 | idéntico |

Visualmente (`tools/verify/klayout_snapshot.py`) la geometría coincide; cambia el estilo: KLayout usa tramados y Riku rellenos translúcidos según el rol de la capa.

## Medir

`riku-mod-layout/examples/`:

- `profile_diff a.gds b.gds [s]`: tiempo y memoria de cada etapa del diff (`SKIP_FP=1`, `PRINTS=1`, `CANON=1` para diagnósticos).
- `profile_view layout.gds`: cuánto tarda el visor en armar la escena y el índice, y qué se dibujaría con el chip completo.
- `verify_dump`: el volcado para comparar con KLayout.
