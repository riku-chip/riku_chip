# Fase 8: Magic (`.mag`)

Estado (2026-09-27): **hecha** (8.1–8.7). Cómo se usa: [`../layouts.md`](../layouts.md#magic-mag). Resumen en [`../roadmap.md`](../roadmap.md). Lo que sigue es el estudio previo, el plan y lo que se hizo.

## Qué es un `.mag` y en qué se diferencia de un GDS

| | `.mag` (Magic) | `.gds` |
|---|---|---|
| Para qué | Formato de **edición** de Magic | Formato de **intercambio y fabricación** |
| Archivo | Texto; **una celda por archivo** | Binario; todas las celdas en uno |
| Jerarquía | Entre archivos: `use inv_1` apunta a `inv_1.mag` | Dentro del archivo (SREF/AREF) |
| Capas | **Lógicas, con nombre** (`ndiff`, `poly`, `ndiffc`, `metal1`) | **De máscara, numeradas** (`66/20`) |
| Geometría | Rectángulos (y triángulos) alineados a los ejes | Polígonos, paths, texto |

GDS también tiene jerarquía: lo propio de Magic es que está **repartida en archivos**. Al exportar a GDS, el `.tech` del PDK convierte las capas de Magic en capas de máscara (un contacto `ndiffc` pasa a ser cortes `licon` + difusión + metal, se generan implantes, etc.): el mismo diseño no tiene la misma geometría en `.mag` y en `.gds`. Riku compara en **capas de Magic**, lo que el diseñador edita, sin reproducir esa conversión.

Ejemplo real (`sky130_fd_sc_hd__inv_1.mag`):

```text
magic
tech sky130A
magscale 1 2
<< nwell >>
rect -38 261 314 582
<< metal1 >>
rect 0 496 276 592
<< labels >>
rlabel locali s 64 215 130 263 6 A
port 1 nsew signal input
<< end >>
```

## Qué hay para probar (verificado en el contenedor)

- **~9 300 `.mag` reales:** 4 976 en SKY130 y 4 305 en GF180 (IHP no usa Magic). Hay `mag/` (celdas completas) y `maglef/` (abstractas).
- **KLayout 0.30 lee `.mag`** con los nombres de capa de Magic: sirve de referencia, como con GDS (`compare.sh`).
- **Magic está instalado** (`/foss/tools/magic/bin/magic`), por si hace falta comparar contra su exportación.

## Enfoque acordado: el lector en el motor (gdstk-rs), Riku fino

Idea del usuario: poner la abstracción en **gdstk-rs** para tener el motor de layouts centralizado. Queda así:

```
gdstk-rs (el motor, repo gdstk_rust; no sabe nada de Riku)
├── Library::from_bytes        GDS   (gdstk C++)
├── Library::from_oas_bytes    OASIS (gdstk C++)
├── Library::from_mag(...)     Magic (nuevo, Rust)
└── LibraryBuilder             armar celdas, polígonos y referencias desde Rust (nuevo; lo usa from_mag)
                                           │ la misma Library
riku-mod-layout (el módulo de Riku para layouts)
├── diff, huellas, instancias gemelas, cuadrantes, cache   (sin cambios)
├── visor, índice, paletas                                 (+ paleta por nombre de capa)
└── lo propio de Riku para Magic: de dónde salen los `use` (Git o PDK), puertos semánticos
```

- **Un solo motor:** cualquier formato entra como la misma `Library`; el diff, la huella jerárquica, las gemelas, el XOR por cuadrantes, el visor y el panel History funcionan con Magic sin cambios.
- **Riku queda fino:** no hace falta un crate `riku-mod-magic`; `riku-mod-layout` acepta también `.mag` (ya maneja GDS y OASIS).
- **Microkernel:** el motor no sabe de Riku (bytes → `Library`); el módulo adapta.
- **Jerarquía entre archivos:** el motor no sabe de Git. `from_mag(bytes, |celda| -> Option<bytes>)` recibe una función que le da los otros `.mag`; Riku la arma buscando primero en el mismo commit y después en las librerías del PDK (`$PDK_ROOT/.../mag`). Las celdas del PDK son iguales en los dos lados: la huella jerárquica las descarta sin costo. Una celda que no se encuentra: aviso y se compara el resto.
- **Contrato del kernel:** hoy `FormatModule::diff` recibe solo los bytes de antes y después. Magic necesita pedir **otros archivos del mismo commit** → el contrato gana un "lector de archivos hermanos" opcional (con valor por defecto, para no romper el crate de Carlos). Le sirve también a Xschem, que hoy busca los símbolos en el disco por su cuenta.
- **Capas con nombre:** las capas de Magic se numeran en la `Library` y se guarda el nombre (gdstk ya soporta nombres de capa por OASIS); los reportes y el visor muestran `metal1`, no un número.

## Referencias estudiadas (2026-09-27)

Código leído (solo lectura, fuera del repo, en `Documents/referencias/`): **Magic 8.3.684** (`database/DBio.c`, el dueño del formato) y **KLayout** (`src/plugins/streamers/magic/db_plugin/dbMAGReader.cc`, un lector independiente que usamos de oráculo). Regla: **donde difieren, seguimos a Magic**; en las pruebas contra KLayout, las diferencias se anotan.

### Gramática (Magic, `DBio.c`)

| Línea | Significado |
|---|---|
| `magic` | primera línea, obligatoria |
| `tech <nombre>` | opcional; `magscale` y `timestamp` solo se leen si está |
| `magscale n d` | **1 unidad del archivo = n/d lambda**; sin la línea, 1/1. Por archivo |
| `<< capa >>` | abre una sección; reservadas: `labels`, `elements`, `properties`, `end` |
| `rect x1 y1 x2 y2` | rectángulo; los de área cero se descartan |
| `tri x1 y1 x2 y2 nw\|sw\|se\|ne` | triángulo rectángulo; el sufijo es la esquina del ángulo recto (tabla abajo) |
| `use celda [id] [dir]` | instancia; `dir` es un **directorio** (el archivo es `dir/celda.mag`), puede empezar con `$PDKPATH` etc. y se omite en usos posteriores de la misma celda |
| `array xlo xhi xsep ylo yhi ysep` | opcional, antes de `transform`; pasos en coordenadas de la hija, `xlo > xhi` invierte el paso |
| `transform a b c d e f` | `x' = a·x + b·y + c`, `y' = d·x + e·y + f`; solo las 8 orientaciones Manhattan |
| `box ...` | caja de la hija; no es geometría |
| `rlabel`/`flabel capa [s] x1 y1 x2 y2 pos ... texto` + `port idx nsew [uso clase [forma]]` | etiquetas y puertos (para 8.6) |
| `<< properties >>` / `string CLAVE valor` | metadatos: `GDS_FILE`/`GDS_START`/`GDS_END`, `FIXED_BBOX`, `LEF*`, `MASKHINTS_*` |
| `<< end >>` | fin; lo que sigue se ignora |

Triángulos (rectángulo `xlo ylo xhi yhi`); Magic y KLayout dan los mismos vértices:

| Sufijo | Vértices |
|---|---|
| `nw` | (xlo,ylo) (xlo,yhi) (xhi,yhi) |
| `sw` | (xlo,ylo) (xlo,yhi) (xhi,ylo) |
| `se` | (xlo,ylo) (xhi,ylo) (xhi,yhi) |
| `ne` | (xlo,yhi) (xhi,yhi) (xhi,ylo) |

Instancia en arreglo: elemento `(i, j)` = `transform ∘ trasladar(i·xsep, j·ysep)`.

### En qué difieren Magic y KLayout, y qué hace Riku

| Tema | Magic | KLayout | Riku |
|---|---|---|---|
| `array` con `xlo > xhi` | paso invertido | 0 instancias | Magic |
| `<< end >>` | termina | sigue leyendo | Magic |
| Línea en blanco entre secciones | error | la acepta | tolerante (aviso) |
| `checkpaint`, `error_*`, `space`, `magnet`, `fence`, `rotate` | capas de DRC/router, no de máscara | `checkpaint` se ignora; `error_*` pasa a capa | se excluyen del diff |
| Nombres de celda con `.` | se respetan | corta en el primer `.` | se respetan |
| `rect` superpuestos | aplica las reglas de pintado del `.tech` | los une por capa | se unen por capa (sin `.tech`) |
| Celda que no se encuentra | error de lectura | celda vacía + aviso | celda vacía + aviso |
| Escala | lambda→µm sale del `.tech` (`scalefactor`) | `lambda` como opción (por defecto 1 µm) | lambda como parámetro; por PDK: sky130 0,01 µm, gf180 0,05 µm |

### Qué cierra de los riesgos

- **Unidades:** coordenada en lambda = valor · n/d, exacta (racional). Los `.mag` del PDK mezclan escalas dentro de una misma librería (SRAM de gf180: 590 con `magscale 1 10`, 1 775 sin la línea): el lector lleva todo a una grilla común (MCM de los `d`) y convierte a µm con el lambda del PDK.
- **Triángulos:** tabla de arriba; 153 000 `tri` en los PDK, las 4 orientaciones.
- **Diff por regiones, no por listas de `rect`:** Magic reescribe la misma geometría como tiras horizontales distintas tras una edición. Comparar la lista de líneas daría cambios falsos; el diff geométrico por capa (el de GDS) es lo correcto.
- **`mag/` contra `maglef/`:** Magic no los distingue (es una convención de open_pdks para las rutas de búsqueda). Riku busca en `mag/` (vista completa); `maglef/` queda como mejora posible.
- **Celdas con `GDS_FILE` (9 149 de 9 281 en los PDK):** su máscara real está en el GDS; el `.mag` es una vista. Riku compara el `.mag` tal cual (lo que edita el diseñador); las celdas del PDK son iguales en los dos lados y la huella jerárquica las descarta.
- **Contrato del kernel:** resuelto en 8.3 con métodos nuevos con valor por defecto (`diff_with`, `load_with`, `load_diff_with`); el crate de Carlos no cambia.

### Qué hay en los PDK (9 281 `.mag`)

11,6 M `rect`, 153 000 `tri`, 300 000 `use` (siempre `use celda id`, sin directorio), 51 `array`, 344 000 `port`. `magscale`: `1 2` (3 774), `1 10` (1 629), `1 5` (39), `12 1` (98, fuentes de sky130), ninguno (3 741).

### Pruebas de referencia

Los casos de KLayout (`testdata/magic/`), con su resultado esperado: `MAG_TEST`, `PearlRiver` (jerarquía con directorio en `use`), `ringo`, `issue_1925` (sky130 con `port`), `gf180mcu_ocd_sram_test` (`magscale 1 10`, `$VAR` en el directorio). Más los 9 281 `.mag` de los PDK: leer todos sin errores y comparar área por capa contra KLayout (`LoadLayoutOptions.mag_lambda`, `mag_library_paths`, `mag_keep_layer_names = True`).

### Lenguaje del lector: Rust

gdstk (C++) sigue con GDS/OASIS y la geometría (Clipper); no se reescribe. El lector `.mag` se escribe **en Rust** dentro de gdstk-rs: es parsear texto, sin puente al C++, seguro entre hilos (leer los archivos de una jerarquía en paralelo con rayon) y sin sumar otro C++ al build. Ni Magic ni KLayout sirven como librería para reusar su lector.

## Pasos y avance

| Paso | Estado | Qué se hizo |
|---|---|---|
| 8.1 | Hecho (2026-09-27) | `gdstk_rust` `a6a87a8`, `rust/src/magic/parse.rs`: la gramática de Magic 8.3 (tabla de arriba), tolerante con lo inofensivo (avisos con número de línea), `<< end >>` termina, `rect` sin área se descarta, solo los 8 transforms Manhattan. Tests unitarios, más dos ignorados: los 9 281 `.mag` de los PDK (se leen todos: 57 591 celdas en sus jerarquías, 18 s) y los casos de `testdata/magic` de KLayout |
| 8.2 | Hecho (2026-09-27) | Mismo commit. `LibraryBuilder` en Rust + un solo shim C++ (`library_from_parts`: celdas, polígonos en un arreglo plano, referencias con repetición `Regular`, etiquetas, nombres de capa; termina en `finish_load`, así la `Library` sigue siendo `Send + Sync`). `magic::collect` (jerarquía nivel por nivel, cada nivel en paralelo) y `Library::from_mag` (grilla común, orientación = `atan2(d, a)` + espejo por el determinante, arreglos como `L·(xsep, 0)` / `L·(0, ysep)`, ciclos descartados, celdas faltantes vacías). `Library::layer_names()` (también LAYERNAME de OASIS). `sniff_format` reconoce Magic. Ejemplo `mag_area` y `tools/verify/mag/compare_mag.sh`: **8 jerarquías de SKY130 y GF180 idénticas a KLayout 0.30.12** en cantidad de polígonos y área por capa (hasta 701 celdas, `magscale` mezclados, triángulos), y los casos de KLayout también |
| 8.3 | Hecho (2026-09-27) | `a03e387`. `viewer-core`: `FileSource`, `DiskFiles`, `DiffFiles`, `join_relative`; `ViewerBackend::load_with` / `load_diff_with`. `riku-kernel`: `FormatModule::diff_with`, `Element::Geometry.layer_name`, `Element::Port`. Todo con valor por defecto: el crate de Carlos compila sin cambios; el JSON de GDS y el v1 no cambian |
| 8.4 | Hecho (2026-09-27) | `6fb87a6`. `riku-mod-layout/mag.rs` (dónde se busca cada `use`: su directorio, junto al archivo en la misma versión, `$RIKU_MAG_PATH` y el PDK; lambda por tecnología), `diff_layout_sides` (clave de cache con todos los archivos de la jerarquía), `diff_libraries`. Núcleo: `GitFiles` (conexión propia y perezosa) en `diff`, `show`, `log` y `status` (HEAD + disco). Visor con los archivos de cada commit. `.mag` en extensiones, ayuda y `riku doctor`. `riku/tests/mag_e2e.rs` |
| 8.5 | Hecho (2026-09-27) | Mismo commit. `tools/palettes/gen_magic_layers.py` → `magic_layers_generated.rs` (plano de cada capa y, en los contactos, el plano que conectan; 326/291/280 nombres en SKY130/GF180/IHP). `magic_layer_spec`: color y apilado de la capa GDS equivalente del PDK; `magic_pdk`: el PDK que conoce más nombres. El visor lista `locali`, `viali`, `metal1`… |
| 8.6 | Hecho (2026-09-27) | `fba8100`. `mag::port_changes`: por celda y nombre, agregados, quitados y cambios de clase, uso, índice, lados o capa; solo movido = cosmético. `Element::Port` en el JSON y el texto (`inv:port:A` · `class: input → inout`) y en la lista de cambios del visor |
| 8.7 | Hecho (2026-09-27) | Documentación (`layouts.md`, `cli.md`, `gui.md`, `arquitectura.md`, READMEs, `desarrollo.md`) y medición (abajo) |

### Resultados

Jerarquía real: la librería `sky130_fd_io` completa (2 545 `.mag`) en un repo; `sky130_fd_io__top_gpio_ovtv2` como archivo comparado; release, sin cache, en el contenedor (`tools/verify/mag/mag_bench.sh`):

| Caso | Tiempo | Memoria | Resultado |
|---|---|---|---|
| Diff del top sin cambios | 1,21 s | 574 MB | 0 cambios |
| Diff del top, rect nuevo solo en una sub-celda | 1,24 s | 585 MB | el cambio en la sub-celda y en el top, vía la instancia, con posición |
| Diff del top, la misma geometría en otras tiras | 1,32 s | 588 MB | 0 cambios |
| Diff de la sub-celda sola | 0,10 s | 23 MB | 1 cambio |
| `riku log` | 0,14 s | 36 MB | |

Además: regresión de la fase 1 (8 salidas idénticas), `compare.sh` de GDS contra KLayout (tres PDKs idénticos), la suite con `-D warnings`, las cuatro combinaciones de features y el crate de Carlos con `viewer-core-compat`.

### Diferencias con el plan

- **Numeración de capas:** un hash estable del nombre (bit 30 en adelante), no una tabla con números chicos: los dos lados de un diff coinciden sin compartir nada, y como en todos lados se muestra el nombre, el número es interno.
- **`LibraryBuilder`** junta todo en Rust y cruza al C++ en una sola llamada, en vez de un handle mutable con un método por figura.
- **Color de las capas:** por plano y tipo (del `.tech`), no por la conversión completa de `cifoutput` (demasiadas capas temporales y operaciones para reproducirla).
- **KLayout como oráculo:** hace falta 0.30.12; la 0.30.2 y la 0.30.4 del contenedor ignoran `magscale`. Se compara sin unir polígonos (unir una jerarquía de 700 celdas tardaba minutos); es más estricto: misma cantidad y misma suma.
- **Diferencias con KLayout que se mantienen** (se sigue a Magic): `array` invertido camina hacia atrás; `<< end >>` termina; las capas de DRC y del router no son geometría; los nombres con `.` no se cortan.

## Pendiente

- **TUI** (fase 7.4, `ratatui`): para después.
- **Panel History:** decidir si, con un filtro de archivos, se ocultan los merges que no tocan esos archivos (hoy se muestran, como `riku log`).
- Decisiones del usuario: licencia y primer release (tag); qué hacer con `.agents/` y `skills-lock.json`.
- Magic, posibles mejoras: `.mag.gz`; `MASKHINTS_*` como geometría; opción para leer las vistas `maglef/`.
