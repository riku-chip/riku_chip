# De las capas a lo eléctrico

Hoy Riku muestra y compara **capas**: dice cuánta área de `poly` cambió, no que un transistor pasó de W = 0,42 a 0,84 µm. Este documento es el camino para que hable de **dispositivos y redes**, en niveles que se apoyan uno en otro:

| Nivel | Qué | Estado |
|---|---|---|
| 1. Leer el dibujo | Leyenda y resaltar una capa | Hecho ([`gui.md`](gui.md#controles)) |
| 2. Dispositivos | Reconocer transistores (tipo, modelo, W, L), mostrarlos y compararlos | Hecho (fases 2.1–2.6); ver abajo |
| 3. Conectividad | Redes del layout; abiertos y cortos entre versiones | En curso: 3.1–3.3 hechas (ver abajo) |
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

### Resultado

- **Verificación** (`tools/verify/devices/`): SKY130 437 de 437 celdas estándar iguales a la netlist de referencia del PDK, en GDS y en `.mag`; GF180MCU 228 de 229 y IHP SG13G2 73 de 74 (las que difieren son de la netlist del PDK: Riku da lo mismo que el extractor de KLayout). La SRAM de `examples/GDS/`: 2271 transistores con W y L iguales a KLayout, en 0,24 s.
- **Diferencias con el diseño:** una compuerta tiene que tocar exactamente dos regiones de fuente/drenaje, como en KLayout (descarta los roces del poly en una esquina de la difusión); el modelo puede depender de W y L (las condiciones de `device`, como `w<0.42` en SKY130); en Magic, los alias de `types` (`scnmos` = `scnfet`).
- **Límite:** una celda de más de 2 millones de polígonos aplanados no se reconoce (un chip entero): se avisa y se comparan sus sub-celdas.

### Decisiones abiertas

1. **Un tipo de elemento nuevo en `riku-diff/v2`:** propongo no subir a v3, porque es aditivo: un consumidor que no conoce `device` lo ignora. Pero conviene documentar en `cli.md` que los tipos de `element` pueden crecer.
2. **Fingers:** por ahora cada finger es un dispositivo; agruparlos (`nf`) espera al nivel 3.
3. **Capa "Transistores" oculta por defecto:** para no cambiar lo que se ve hoy. Se puede prender por defecto si resulta útil.

## Nivel 3: conectividad

**Objetivo:** las **redes** de cada celda (qué metal, poly y difusión están unidos por contactos y vías), con el nombre de sus etiquetas, y entre dos versiones lo que un diff de área no ve:

```
! nand2_1: corto entre A y Y          (en (1,20; 0,85): metal1 +0,02 µm²)
! nand2_1: abierto en VPWR, se partió en 2 (una parte con pfet@(0,49; 2,10))
```

Con redes, además, cada transistor sabe qué red va a su compuerta, fuente y drenaje, y los fingers se agrupan en un solo dispositivo (`nf`).

### De dónde salen las reglas: otra vez el `.tech` de Magic

Las tres secciones que hacen falta ya están en el `.tech` que leemos para los transistores; no hay tablas por PDK:

- **`contact`**: cada tipo de contacto y las dos capas que une. En SKY130, `mcon locali metal1` y `ndc ndiff locali`; en GF180MCU e IHP, `ndc ndiff metal1` (no tienen `li`).
- **`connect`**: qué tipos son el mismo conductor. Por ejemplo, `*poly,xpc,allfets,polyfill` (la compuerta es poly, así que la red de la compuerta sale sola) o `allnactivenonfet` (la difusión **sin** los transistores: el canal separa fuente de drenaje).
- **`aliases`**: expande nombres como `allfets` o `allnactivenonfet`. `*li` quiere decir `locali` y todos los contactos que la tienen como una de sus dos capas.
- **`cifinput`**: de qué capas GDS sale cada tipo, con las mismas reglas del nivel 2. Por ejemplo, `ndiff` = `DIFF` y `NSDM`, sin `POLY` ni `NWELL`. Sus líneas `labels LI`, `labels LIPIN port` y `labels LITXT text` dicen en qué capas GDS van las etiquetas de cada conductor.

`gen_devices.py` suma esas secciones a la tabla generada para cuando no está el PDK.

### Algoritmo, por celda aplanada

1. **Regiones:** cada tipo conductor y cada tipo de contacto se evalúa como región con su regla de `cifinput` (`or`, `and` y `and-not` con `boolean_owned`; `grow` y `shrink` se ignoran, como en el nivel 2). Los tipos de un mismo grupo de `connect` se unen.
2. **Pedazos:** cada polígono de la unión de un grupo es un pedazo conductor. Clipper ya junta los que se tocan o se superponen.
3. **Contactos:** cada polígono de contacto une los pedazos de sus dos capas que contiene en un punto interior. Se buscan con la grilla del nivel 2, y los pedazos se unen con *union-find*.
4. **Nombres:** una etiqueta GDS (o de Magic) sobre una capa de `labels` nombra la red del pedazo en el que cae. Si una red tiene dos nombres, se toma el primero en orden alfabético y queda un aviso (en LVS eso sería un error). Las redes sin nombre se llaman por su transistor más cercano: `net@nfet(0,49; 0,66)`.
5. **Terminales:** la compuerta es el pedazo de poly que contiene el transistor. La fuente y el drenaje son los pedazos de difusión que toca, los mismos dos que ya encuentra el nivel 2.
6. **Fingers:** los transistores con el mismo modelo, L y red de compuerta, y el mismo par {fuente, drenaje}, se agrupan en uno (`W` es la suma y `nf` la cantidad).

**Pozos y sustrato** van aparte. `nwell` es un conductor más: el grupo `*nwell,*nsd` une el pozo con sus tomas. El sustrato `pwell` no se dibuja en muchos PDK, así que el cuerpo de un nfet va a una red implícita `sub` unida a las tomas `psd` que no están dentro de un `nwell`. Eso alcanza para comparar contra las netlists de las celdas estándar (`VPB`/`VNB`).

**Salida:** una netlist SPICE por celda (`.subckt` con los pines como puertos: las etiquetas `port` en Magic y las de las capas `PIN` en GDS). Sirve para verificar ahora y es la base del nivel 4.

### Comparar redes entre versiones

Las redes de A y de B no tienen identidad propia, así que se comparan por **anclas** que sí la tienen:

- las etiquetas (nombre y capa);
- los terminales de los transistores emparejados por posición en el nivel 2 (compuerta, fuente y drenaje).

Cada red es un conjunto de anclas. Si el ancla `x` está en la red `a` de A y en la red `b` de B, el par `(a, b)` es una arista, y la estructura que queda dice qué pasó:

- **Abierto:** una red de A cuyas anclas caen en dos o más redes de B.
- **Corto:** dos o más redes de A cuyas anclas caen en una sola red de B.
- **Renombrado:** la misma red, con otro nombre.

Las anclas que existen solo de un lado (un transistor agregado, por ejemplo) no cuentan: agregar un transistor no es un corto.

**Dónde:** la ubicación de un abierto o un corto es la de los cambios de geometría del diff que tocan esas redes. Casi siempre es uno solo, y un clic lo encuadra.

**En el reporte:**
- `Element::Net { cell, name }` con `details` `kind: open|short|renamed` y las redes o anclas del otro lado. Es otro tipo aditivo en `riku-diff/v2`, como `device`.
- Los transistores del nivel 2 suman `nf` y los nombres de sus redes (`g`, `s`, `d`). Un transistor que cambió de red es un cambio más: `~ nfet@(…): d: Y → net@…`.

### Qué se ve

- **CLI y JSON:** abiertos y cortos arriba de todo en `riku diff` y `show`, porque son lo más grave del reporte. El tipo de cambio es `!` en texto y `"severity": "error"` en JSON.
- **Diff en el visor:** abiertos y cortos al principio de la lista **Cambios**, en rojo. Un clic encuadra el lugar.
- **Visor:** el tooltip del polígono bajo el cursor suma su red (`metal1 · red A`), y el resumen dice `Redes: 9 (5 con nombre)`. Resaltar una red entera con un clic necesita preguntarle al backend qué red hay en un punto (ver Decisiones).
- **Ejemplo `nets`:** `riku-mod-layout/examples/nets.rs <gds|mag> [celda]` imprime la netlist extraída.

### Dónde va

| Qué | Dónde |
|---|---|
| `contact`, `connect`, `aliases` y `labels` | `riku-mod-layout/src/devices/rules.rs` (y `gen_devices.py`) |
| Regiones, pedazos, contactos, nombres y terminales | `riku-mod-layout/src/nets/extract.rs`, junto a `devices/` |
| Fingers y netlist SPICE | `riku-mod-layout/src/nets/netlist.rs` |
| Abiertos, cortos y renombres | `riku-mod-layout/src/nets/diff.rs`, desde `gds_diff.rs` |
| `Element::Net` | `riku-kernel/src/change.rs`; texto en `cli/format/diff_text.rs` |
| Lista Cambios y tooltip | `diff_scene.rs` y `viewer_core_compat.rs` |

No hace falta nada nuevo en gdstk_rust: con `boolean_owned` alcanza.

### Rendimiento

Igual que en el nivel 2: solo se extraen las celdas que cambiaron en una capa de las reglas, y la celda abierta en el visor. Se aplanan solo las capas conductoras. Hay un tope de polígonos (`MAX_POLYGONS`), y las celdas más grandes se avisan y se comparan en sus sub-celdas. Lo que cuesta es la unión de cada grupo de conductores: para la SRAM del repo espero menos de un segundo, y se mide antes de la fase 3.4. La extracción jerárquica (cada celda una vez, con sus pines hacia arriba, como KLayout) es lo que haría falta para un chip entero, y queda para después.

### Verificación: Netgen contra las netlists del PDK

Es la misma prueba que un LVS real. Por cada celda estándar, la netlist que extrae Riku se compara con la de referencia del PDK usando **Netgen** (`netgen -batch lvs`, con el `setup.tcl` de cada PDK; está en el contenedor). Netgen compara la topología completa: qué transistor va a qué red.

- `tools/verify/nets/compare_netgen.sh` se corre sobre SKY130 (437 celdas, en GDS y `.mag`), GF180MCU e IHP.
- Las dos celdas conocidas del nivel 2 siguen marcadas como conocidas.
- Espero que falle alguna celda por pozos o sustrato, y esas diferencias se documentan.
- Segundo oráculo para la SRAM: `LayoutToNetlist` de KLayout con las mismas capas. Se compara la cantidad de redes y el grado de cada una.

### Fases

| Fase | Qué | Esf. |
|---|---|---|
| 3.1 | Reglas: `contact`, `connect`, `aliases` y `labels` para los tres PDK, con la tabla generada | S |
| 3.2 | Extracción: regiones, pedazos, contactos, nombres y terminales, con tests por PDK | M |
| 3.3 | Netlist SPICE, fingers (`nf`), ejemplo `nets` y verificación con Netgen en las celdas estándar | M |
| 3.4 | Diff: abiertos, cortos y renombres por anclas, y transistores que cambiaron de red, en CLI y JSON (con un fixture de SKY130 con un corto y otro con un abierto) | M |
| 3.5 | Visor: lista Cambios, tooltip con la red y resumen | S |

En total, **L** (una a dos semanas). La 3.1 y la 3.2 no cambian nada visible; la 3.3 ya da algo útil (la netlist); la 3.4 es lo que se ve en el diff.

### Avance

- **3.1–3.3 hechas** (`riku-mod-layout/src/nets/`, `devices/regions.rs`): redes, terminales, fingers, resistores y la netlist SPICE (`examples/nets.rs`). Verificación en [`tools/verify/README.md`](../tools/verify/README.md#redes-toolsverifynets): con Netgen, GF180MCU 219 de 219 celdas, SKY130 422 de 427, IHP 68 de 73; las diferencias, confirmadas con Magic, son de las netlists del PDK. SRAM del repo: 2,1 s.
- **Diferencias con el diseño:** `grow` y `shrink` se evalúan (con `offset_owned`, nuevo en gdstk_rust): el pozo P de SKY130 se arma agrandando la difusión, y `npd` de la SRAM es `npass` sin las compuertas angostas. Magic pinta los tipos en orden y en un mismo plano el posterior tapa al anterior; sin eso IHP daba cortos falsos. Las regiones se unen agrandadas medio nanómetro (Clipper puede dejar separados polígonos que comparten un borde). Una etiqueta es pin si cae sobre un polígono de pin (`labels LIPIN port`), o en Magic si tiene `port`. También se reconocen resistores (`device resistor|rsubcircuit`); los de modelo `None` (metal en IHP) son cortos.
- **Pendiente:** la SRAM contra Magic difiere en los pull-ups de la celda de memoria (Magic cuenta 720 `special_pfet_latch`, Riku y KLayout 360).

### Decisiones abiertas

1. **Resaltar una red con un clic:** hoy la escena es solo geometría. Para saber la red de un punto hace falta un método nuevo en `ViewerBackend` (`net_at(point) -> Option<polígonos>`), con un valor por defecto que no rompe el crate de Carlos, pero es un cambio en el contrato compartido y conviene avisarle. Propongo dejarlo para después de la 3.5: el tooltip y la lista Cambios ya cubren lo principal.
2. **Severidad:** propongo sumar `severity` (`error` para abiertos y cortos) al `Change` de v2, como campo opcional. La alternativa es no tocar el schema y ordenar por tipo.
3. **Dos nombres en una red:** propongo un aviso y no un error: es un problema del diseño que ya reportaría el LVS del nivel 4.
4. **Sustrato implícito:** una sola red `sub` para todo lo que no está en un `nwell`, suficiente para las celdas estándar. Los pozos aislados (`dnwell`) se tratan como redes normales.

## Nivel 4: LVS

Layout contra esquemático en cada commit, con herramientas externas: Magic `extract all` + `ext2spice lvs` y `xschem --netlist`, luego `netgen -batch lvs … <pdk>_setup.tcl out.json` (o el LVS de KLayout). Riku muestra el resultado junto al diff: qué dispositivos o redes no coinciden y **dónde están en el layout**. En CI, bloquear un PASS → FAIL; un FAIL → FAIL es aviso. Detalle del flujo en [`pendientes.md`](pendientes.md#ideas-a-futuro), sección *Verificación en CI*. Esfuerzo **M**.

## Nivel 5: chequeos eléctricos

Con herramientas externas, y mostrando sus resultados y su diferencia entre commits:

- **ERC:** compuertas o pines sin conectar, pozos sin polarizar.
- **Antena:** las reglas de antena del PDK (van en su deck de DRC).
- **Parásitos (PEX):** R y C extraídas (Magic `ext2spice` con `cthresh`/`rthresh`) y cuánto cambian: "la red `out` subió 12 fF".
- **Post-layout:** simular la netlist extraída y comparar las `.meas` con las de antes (*Regresión de `.meas`* en [`pendientes.md`](pendientes.md#ideas-a-futuro)).

Esfuerzo **L**. Los niveles 1 a 3 son de Riku, en Rust y sin herramientas externas; el 4 y el 5 orquestan Netgen, Magic, KLayout y ngspice.
