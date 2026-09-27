# Fase 8: Magic (`.mag`) — ideas, antes del diseño

Estado (2026-09-27): **ideas acordadas, falta el diseño detallado** (`/sc:design`). Resumen en [`../roadmap.md`](../roadmap.md).

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

- Unidades: `magscale` y la escala del `.tech`.
- Cómo escribe Magic los triángulos (tiles no Manhattan).
- `maglef/` (abstractas) contra `mag/` (completas): cuál usar para las celdas del PDK.
- El cambio en el contrato del kernel sin romper a `xschem-viewer-rust`.

## Otros pendientes anotados en esta sesión

- **TUI** (fase 7.4, `ratatui`): para después.
- **Panel History:** decidir si, con un filtro de archivos, se ocultan los merges que no tocan esos archivos (hoy se muestran, como `riku log`).
- Decisiones del usuario: licencia y primer release (tag); qué hacer con `.agents/` y `skills-lock.json`.
