# Fase 8: Magic (`.mag`) — ideas, antes del diseño

Estado (2026-09-27): **ideas acordadas y referencias estudiadas** (Magic y KLayout); falta el diseño detallado (`/sc:design`). Resumen en [`../roadmap.md`](../roadmap.md).

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
- **`mag/` contra `maglef/`:** Magic no los distingue (es una convención de open_pdks para las rutas de búsqueda). Riku busca en `mag/` (vista completa) y deja `maglef/` como opción.
- **Celdas con `GDS_FILE` (9 149 de 9 281 en los PDK):** su máscara real está en el GDS; el `.mag` es una vista. Riku compara el `.mag` tal cual (lo que edita el diseñador); las celdas del PDK son iguales en los dos lados y la huella jerárquica las descarta.
- **Queda para el diseño:** el contrato del kernel (lector de archivos hermanos) sin romper a Carlos.

### Qué hay en los PDK (9 281 `.mag`)

11,6 M `rect`, 153 000 `tri`, 300 000 `use` (siempre `use celda id`, sin directorio), 51 `array`, 344 000 `port`. `magscale`: `1 2` (3 774), `1 10` (1 629), `1 5` (39), `12 1` (98, fuentes de sky130), ninguno (3 741).

### Pruebas de referencia

Los casos de KLayout (`testdata/magic/`), con su resultado esperado: `MAG_TEST`, `PearlRiver` (jerarquía con directorio en `use`), `ringo`, `issue_1925` (sky130 con `port`), `gf180mcu_ocd_sram_test` (`magscale 1 10`, `$VAR` en el directorio). Más los 9 281 `.mag` de los PDK: leer todos sin errores y comparar área por capa contra KLayout (`LoadLayoutOptions.mag_lambda`, `mag_library_paths`, `mag_keep_layer_names = True`).

### Lenguaje del lector: Rust

gdstk (C++) sigue con GDS/OASIS y la geometría (Clipper); no se reescribe. El lector `.mag` se escribe **en Rust** dentro de gdstk-rs: es parsear texto, sin puente al C++, seguro entre hilos (leer los archivos de una jerarquía en paralelo con rayon) y sin sumar otro C++ al build. Ni Magic ni KLayout sirven como librería para reusar su lector.

## Pasos tentativos

| Paso | Qué | Dónde |
|---|---|---|
| 8.1 | Lector `.mag`: `rect`, triángulos, `use` + `transform` + `array`, `rlabel`/`flabel`/`port`, `magscale`, `properties`. Leer los ~9 300 `.mag` sin errores y comparar área por capa contra KLayout | gdstk_rust (`rust/`) |
| 8.2 | `LibraryBuilder` (celdas, polígonos, referencias, nombres de capa) en el shim | gdstk_rust |
| 8.3 | Jerarquía entre archivos: función que resuelve los `use` (commit y PDK); lectura en paralelo de los archivos de una jerarquía | gdstk_rust + riku |
| 8.4 | `.mag` en `riku-mod-layout`; "lector de archivos hermanos" en el contrato del kernel | riku-kernel, riku-mod-layout, riku |
| 8.5 | Visor: paleta por nombre de capa, coherente con la de GDS del mismo PDK | riku-mod-layout |
| 8.6 | Extra semántico: **puertos** (`port 1 nsew signal input`) comparados como en Xschem: "se agregó el puerto `EN`", "`A` pasó de input a inout" | riku-mod-layout |

## Riesgos a verificar en el diseño

Unidades, triángulos y `mag/` contra `maglef/`: resueltos en *Referencias estudiadas*. Queda el cambio en el contrato del kernel sin romper a `xschem-viewer-rust`.

## Otros pendientes anotados en esta sesión

- **TUI** (fase 7.4, `ratatui`): para después.
- **Panel History:** decidir si, con un filtro de archivos, se ocultan los merges que no tocan esos archivos (hoy se muestran, como `riku log`).
- Decisiones del usuario: licencia y primer release (tag); qué hacer con `.agents/` y `skills-lock.json`.
