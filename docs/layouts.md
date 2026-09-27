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

1. **Huella jerárquica** (`prints.rs`, un árbol de Merkle sobre la jerarquía, como los árboles de Git): la de una celda combina el hash de su geometría propia con, por cada instancia, la huella jerárquica de la celda instanciada y su transformación. Dos celdas con la misma huella jerárquica aplanan a lo mismo: se descartan sin aplanar nada (solo se leen los polígonos propios de cada celda).
2. **Instancias gemelas:** en una celda que difiere, cada instancia se empareja con su gemela de la otra versión (misma celda según la huella jerárquica, misma transformación). Las gemelas se cancelan; solo se aplanan las que no tienen gemela y la geometría propia.
3. **Huella por capa:** de lo que queda, un hash de cada polígono en **forma canónica** (vértices cuantizados, sin repetidos, sentido antihorario, empezando por el menor), agrupado por capa. Dos capas con la misma huella son iguales aunque el archivo las escriba distinto (reexportadas por otra herramienta): se saltan sin XOR.
4. **XOR solo de lo que cambió:** en una capa distinta, los hashes que sobran de cada lado son los polígonos propios; Clipper recibe esos y los comunes que los tocan (`xor_split_owned` de gdstk-rs), que salen de las instancias sin gemela y de las gemelas cuyo bbox toca la zona del cambio. El resultado es el mismo que el del XOR completo.
5. **XOR por cuadrantes:** si entre los dos lados hay más de 2 000 polígonos, un quadtree parte la zona hasta ~1 000 polígonos por cuadrante; cada cuadrante hace su XOR en paralelo y recorta el resultado a su rectángulo. Clipper se vuelve casi cuadrático con miles de rectángulos alineados (una capa de relleno de 124 mil tardaba 358 s; por cuadrantes, 0,28 s). Un polígono de diferencia que cruza un borde de cuadrante sale partido: el conteo `+N polys` puede subir; las áreas, los bbox y las instancias no cambian.

Todo se aplana **por pedazos** (lo propio de la celda y cada instancia por separado): cada pedazo se usa y se suelta, así nunca está un chip entero aplanado en memoria. Los pedazos, las capas y las celdas se reparten entre los núcleos (`--jobs N` o `RIKU_JOBS`, por defecto todos; ver [`cli.md`](cli.md)).

Un `user_project_wrapper` de 42 MB (IHP, 6,2 millones de polígonos) pasó de no terminar en 45 minutos a **1,4–2,3 s** (sin cambios reales, con cambios en la top o dentro de una sub-celda) y **4,7–7 s** si se mueve la instancia de un pad (74 mil polígonos corridos: el peor caso de Clipper; KLayout tarda 211 s), siempre con menos de 1 GB y áreas idénticas a KLayout. Diseño y mediciones en [`diseno/fase6.md`](diseno/fase6.md).

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
- `profile_prints layout.gds [celda] [hilos]`: lo que decide la huella por pedazos (fase 6.4): reparto de las referencias de la top, igualdad con la huella entera, tiempo de aplanar/hashear/ordenar, memoria por polígono y escalado por hilos (`SKIP_WHOLE=1` para medir la memoria de los pedazos sola).
- `profile_xor a.gds b.gds <celda instanciada o top> <layer> <datatype>`: el XOR de una capa entre dos versiones, entero y por k×k cuadrantes (tiempos, áreas, duplicados por los bordes); `SKIP_WHOLE=1` cuando el entero tarda minutos.
- `verify_dump`: el volcado para comparar con KLayout.
