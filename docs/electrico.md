# De las capas a lo eléctrico

Hoy Riku muestra y compara **capas**: dice cuánta área de `poly` cambió, no que un transistor pasó de W = 0,42 a 0,84 µm. Este documento es el camino para que hable de **dispositivos y redes**, en niveles que se apoyan uno en otro:

| Nivel | Qué | Estado |
|---|---|---|
| 1. Leer el dibujo | Leyenda y resaltar una capa | Hecho ([`gui.md`](gui.md#controles)) |
| 2. Dispositivos | Reconocer transistores (tipo, modelo, W, L), mostrarlos y compararlos | Reconocer y mostrar: hecho (fases 2.1–2.5). Comparar: en curso |
| 3. Conectividad | Redes del layout; abiertos y cortos entre versiones | Idea |
| 4. LVS | Layout contra esquemático, con el resultado en el visor | Idea |
| 5. Chequeos eléctricos | ERC, antena, parásitos, post-layout | Idea |

## Nivel 2: dispositivos

**Objetivo:** en un layout de SKY130, GF180MCU o IHP SG13G2, cada transistor marcado en el visor con su modelo (`sky130_fd_pr__nfet_01v8`), W y L; y en el diff, **"M en (12,3; 4,5): W 0,42 → 0,84 µm"** en vez de "+0,3 µm² en poly".

### De dónde salen las reglas: el `.tech` de Magic del PDK

Cada PDK trae en su `.tech` de Magic (el principal, no el `-GDS`) todo lo necesario, así que no hay tablas escritas a mano por PDK:

- **`cifinput`**: cómo se arma cada tipo de transistor de Magic a partir de capas GDS, con `and`, `and-not`, `or`, `grow`, `shrink` y capas temporales (`templayer`). Por ejemplo, en SKY130:
  ```
  layer nfet DIFF,barediff
   and POLY
   and NSDM
   and-not PSDM
   and-not HVI,hvcheck
   and-not LVTN
   ...
  calma DIFF 65 20
  ```
- **`device`**: el modelo SPICE de cada tipo: `device msubcircuit sky130_fd_pr__nfet_01v8 nfet,scnfet …`.

Medido en los PDK instalados: SKY130 (123 capas `calma`, 2 estilos), GF180MCU (61 `device`, 68 `calma`) e IHP (45 `device`, 131 `calma`). Es la misma fuente que ya usamos para los colores y el lambda de Magic (`pdk_tech.rs`, `magic_layers_generated.rs`), con el mismo esquema: se lee del PDK instalado y, si no hay, de una tabla generada (`tools/palettes/gen_devices.py` → `devices_generated.rs`).

### Algoritmo, por celda

1. **Compuertas:** `POLY ∩ DIFF` (las capas que la regla del tipo cruza con `and`), aplanando la celda. Cada región conexa es una compuerta (un *finger*).
2. **Tipo:** en un punto interior de cada compuerta se evalúa la regla de cada tipo como condición de pertenencia: `and X` → el punto está en X; `and-not X` → no está; `or` → cualquiera de las dos. Gana el primer tipo que cumple, en el orden del `.tech`, igual que en Magic. Los marcadores (implantes, pozos, `LVTN`, `HVI`) cubren la compuerta entera en un diseño que pasa DRC, así que un punto alcanza. `grow`/`shrink` sobre marcadores se ignoran; es una aproximación que se mide en la verificación.
3. **W y L**, como el LVS de KLayout: los bordes de la compuerta que caen sobre el borde del poly son los de fuente y drenaje. `W = (suma de esos bordes) / 2` y `L = área / W`. Para un rectángulo es exacto; para una compuerta doblada, la convención estándar.
4. **Magic (`.mag`):** los transistores ya vienen pintados como capas (`nfet`, `pfet`, `nfetlvt`…). No hace falta el paso 1 ni el 2: cada polígono de una capa de transistor es una compuerta, y el modelo sale de `device`.

Cada finger es un dispositivo. Agrupar fingers que comparten fuente y drenaje (`nf`) pide conectividad y queda para el nivel 3.

### Qué se ve

- **Visor:** una capa sintética **"Transistores"** en la escena: contorno amarillo de cada compuerta y una etiqueta `N · W 0,42 · L 0,15`. Al ser una capa más, reutiliza todo lo que ya existe: la leyenda, resaltarla, ocultarla desde **Capas**, y el tooltip con el modelo completo. Viene **oculta**: se prende desde el panel.
- **Resumen:** `Transistores: 12 (8 N, 4 P)` en el panel **Detalles**.
- **CLI y JSON:** `riku diff` y `show` suman cambios de dispositivo en un layout.
  - Los dispositivos de A y B se emparejan por celda y por posición de la compuerta; una compuerta que se movió menos que su propio L sigue siendo la misma.
  - Se reportan altas, bajas y cambios de modelo, W o L.
  - Van como `Element::Device { cell, at, model }` con `details` de `w`/`l` en µm. Es un tipo nuevo dentro de `riku-diff/v2` (ver Decisiones).
  - En texto: `~ INV:M(1,20; 0,50) sky130_fd_pr__nfet_01v8  W 0,42 → 0,84 µm`.
- **Diff en el visor:** los cambios de dispositivo en la lista **Cambios** (un clic encuadra) y en la capa "Transistores" del diff.

### Dónde va

| Qué | Dónde |
|---|---|
| `AND` de polígonos | `gdstk_rust`: `boolean_owned(a, b, And)`, junto al XOR que ya existe (Clipper) |
| Leer `cifinput` y `device` | `riku-mod-layout/src/devices/rules.rs` (con `pdk_tech.rs`) |
| Tabla sin PDK instalado | `riku-mod-layout/src/devices/devices_generated.rs`, de `tools/palettes/gen_devices.py` |
| Compuertas, tipo, W y L | `riku-mod-layout/src/devices/extract.rs` |
| Comparar dispositivos | `riku-mod-layout/src/devices/diff.rs`, desde `gds_diff.rs` |
| Capa "Transistores" | `riku-mod-layout/src/viewer_core_compat.rs` y `diff_scene.rs` |
| `Element::Device` | `riku-kernel/src/change.rs`; texto en `cli/format/diff_text.rs` |

No cambia ningún contrato de `viewer-core`: la capa sintética es geometría y etiquetas como las demás. El núcleo solo suma un tipo de elemento; no sabe de transistores.

### Rendimiento

Solo se extraen dispositivos de las celdas que el diff ya marcó como cambiadas (las iguales se descartan antes por huella) y de la celda abierta en el visor. Se aplanan solo las capas que usan las reglas (poly, difusión y marcadores). El resultado va a la caché de diffs, cuya versión se sube. Hay que medir con el chip de 42 MB: si una celda tiene demasiadas compuertas, se extrae por pedazos, como el XOR.

### Verificación

`tools/verify/devices/compare_devices.sh`: la extracción de KLayout (sus decks de LVS, que traen los tres PDK) contra la de Riku, celda por celda, en las librerías estándar (SKY130: 437 celdas, GF180MCU: 230, IHP: 78). Se compara la cantidad de dispositivos por modelo, y W y L de cada uno con una tolerancia de una grilla. Las diferencias conocidas (fingers agrupados, `grow`/`shrink` ignorados) se listan.

### Fases

| Fase | Qué | Esf. |
|---|---|---|
| 2.1 | `AND` de polígonos en gdstk_rust | S |
| 2.2 | Reglas desde `cifinput` y `device` para los tres PDK, con la tabla generada | M |
| 2.3 | Extracción (compuertas, tipo, W, L) y Magic directo, con tests por PDK | M |
| 2.4 | Verificación contra KLayout en las librerías estándar | M |
| 2.5 | Visor: capa "Transistores", resumen y tooltip | S |
| 2.6 | Diff: cambios de dispositivo en CLI, JSON y visor | M |

En total, **L** (una a dos semanas). Cada fase se cierra con tests y un commit. La 2.1 a 2.4 no cambian nada visible; la 2.5 ya sirve sola.

### Decisiones abiertas

1. **Un tipo de elemento nuevo en `riku-diff/v2`:** propongo no subir a v3, porque es aditivo: un consumidor que no conoce `device` lo ignora. Pero conviene documentar en `cli.md` que los tipos de `element` pueden crecer.
2. **Fingers:** por ahora cada finger es un dispositivo; agruparlos (`nf`) espera al nivel 3.
3. **Capa "Transistores" oculta por defecto:** para no cambiar lo que se ve hoy. Se puede prender por defecto si resulta útil.

## Nivel 3: conectividad

Unir las capas conductoras por contactos y vías (la sección `connect` del `.tech` de Magic, o `connect` del LVS de KLayout) para armar las **redes** de cada celda, con los nombres de las etiquetas. Entre dos versiones: una red que se partió (**abierto**) o dos que se unieron (**corto**), que un diff de área no ve. Con redes, los dispositivos del nivel 2 saben qué red va a cada terminal y se pueden agrupar sus fingers. Esfuerzo **L**; es lo que más vale después del nivel 2.

## Nivel 4: LVS

Layout contra esquemático en cada commit, con herramientas externas: Magic `extract all` + `ext2spice lvs` y `xschem --netlist`, luego `netgen -batch lvs … <pdk>_setup.tcl out.json` (o el LVS de KLayout). Riku muestra el resultado junto al diff: qué dispositivos o redes no coinciden y **dónde están en el layout**. En CI, bloquear un PASS → FAIL; un FAIL → FAIL es aviso. Detalle del flujo en [`pendientes.md`](pendientes.md#ideas-a-futuro), sección *Verificación en CI*. Esfuerzo **M**.

## Nivel 5: chequeos eléctricos

Con herramientas externas, y mostrando sus resultados y su diferencia entre commits:

- **ERC:** compuertas o pines sin conectar, pozos sin polarizar.
- **Antena:** las reglas de antena del PDK (van en su deck de DRC).
- **Parásitos (PEX):** R y C extraídas (Magic `ext2spice` con `cthresh`/`rthresh`) y cuánto cambian: "la red `out` subió 12 fF".
- **Post-layout:** simular la netlist extraída y comparar las `.meas` con las de antes (*Regresión de `.meas`* en [`pendientes.md`](pendientes.md#ideas-a-futuro)).

Esfuerzo **L**. Los niveles 1 a 3 son de Riku, en Rust y sin herramientas externas; el 4 y el 5 orquestan Netgen, Magic, KLayout y ngspice.
