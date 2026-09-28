# Formatos

Qué compara Riku en cada tipo de archivo, qué cuenta como cosmético y qué opciones tiene. Cómo se usa: [`cli.md`](cli.md) y [`gui.md`](gui.md).

## Esquemáticos Xschem (`.sch`)

Se leen con [`xschem-viewer-rust`](https://github.com/carloscl03/xschem-viewer-rust) (submódulo); no hace falta tener `xschem` instalado. Los `.sym` se abren en el visor, pero no se comparan.

- **Componentes** por nombre (`R1`, `M5`): añadidos, eliminados, renombrados y modificados, con cada parámetro antes y después.
- **Nets** añadidas y eliminadas.
- **Cosmético:** si todo se movió igual (Move All) o un componente solo cambió de lugar. La posición, el giro y el espejo no cuentan como parámetros.
- **Archivo nuevo o borrado:** todo aparece añadido o eliminado.

**Símbolos y PDK.** Para dibujar y conectar pines hacen falta los `.sym`. Riku los busca en este orden (solo rutas que existen):

1. **`.xschemrc`** del directorio actual o de `~`: `set PDK_ROOT` + `set PDK`, `set XSCHEM_SHAREDIR`, `append XSCHEM_LIBRARY_PATH`.
2. **Variables:** `$PDK_ROOT`/`$PDK` → `$PDK_ROOT/$PDK/libs.tech/xschem`; `$TOOLS` → `$TOOLS/xschem/share/xschem/xschem_library/devices`. En iic-osic-tools, `sak-pdk sky130A` las define.
3. **Detección:** sin `$PDK`, Riku elige entre los PDK instalados (`$PDK_ROOT` o `/foss/pdks`) el que tiene los símbolos que usa el esquemático. Si mezcla PDKs los carga todos; en un empate prefiere `sky130A`, `gf180mcuD` e `ihp-sg13g2`.

El visor muestra en **Detalles** de dónde salió el PDK y avisa si faltan símbolos; `riku doctor` lo resume.

## Layouts GDSII, OASIS y Magic

Todo vive en `riku-mod-layout`, sobre [`gdstk_rust`](https://github.com/Adriel2503/gdstk_rust) (submódulo `external/gdstk`). El formato se elige por la firma del archivo, así que A y B pueden ser de formatos distintos.

- **XOR por celda y capa**, aplanando la jerarquía: un cambio dentro de una sub-celda aparece en cada celda que la instancia, con su origen (`TOP:L1/0:INV`), el bbox en coordenadas de esa celda y el área añadida/eliminada en µm².
- **Instancias:** cada instancia (y cada repetición de un AREF) tiene su cambio y su recuadro en el visor; la CLI las agrupa (`en N instancias`) y el JSON trae `instances`.
- **Renombres:** una celda que desaparece y otra con la misma geometría que aparece son un renombre (`INV → INV_X1`). Solo renombres puros; si hay varias candidatas, no se adivina.
- **Librerías:** se marca qué celdas cambiaron, incluidos cambios heredados de sub-celdas; el visor puede filtrar "solo con cambios".
- **Capas con nombre:** si el OASIS nombra sus capas (LAYERNAME), o en Magic, los cambios llevan el nombre (`layer_name`).
- **Transistores:** en las celdas que cambiaron, los transistores que se agregaron, se quitaron o cambiaron de modelo, W o L: `~ nand2_1:sky130_fd_pr__nfet_01v8 @ (0.490, 0.657)` · `w_um: 0.650 → 0.460`. Ver [Transistores y redes](#transistores-y-redes).
- **Abiertos y cortos:** en las celdas con cambios en una capa que conduce, las redes que se partieron o se unieron, con dónde: `! nand2_1:net:B = Y` · `corto (redes unidas): B, Y → B = Y`. Una red sin etiqueta se nombra por un transistor que toca (`s de nfet_01v8 en (0.49, 0.56)`). Van primero y en JSON con `"severity": "error"`.
- **Cosmético:** un cambio con área total bajo `--cosmetic-threshold-um2` (0,01 µm² por defecto, debajo del piso DRC de SKY130/GF180).
- **Errores:** un layout roto, truncado o con un ciclo de celdas es un error del archivo, no "sin cambios". Las referencias a celdas que no están se avisan y el resto se compara.
- **Cache:** en layouts de más de 1 MiB el resultado se guarda en `~/.cache/riku/diff` (tope 512 MiB). `--no-cache` o `RIKU_NO_CACHE=1` la desactivan.
- **Rendimiento:** un chip de 42 MB (6,2 millones de polígonos) se compara en 1,4–2,3 s con menos de 1 GB. Cómo: [`desarrollo.md`](desarrollo.md#rendimiento).

### Magic (`.mag`)

Magic guarda **una celda por archivo** y sus capas son **lógicas y con nombre** (`ndiff`, `poly`, `metal1`), no las de máscara. Riku compara en esas capas, las que edita el diseñador, sin reproducir la conversión a GDS del `.tech`. Donde Magic y KLayout difieren, se sigue a Magic.

- **Sub-celdas de la misma versión:** cada `use` se busca en el directorio que indique (relativo, o con `$PDK_ROOT`, `$PDKPATH`, `~`), junto al archivo que la usa **en el mismo commit** (o en el disco para `status`), en las librerías del PDK (`$PDK_ROOT/<tech>/libs.ref/*/mag`) y en `$RIKU_MAG_PATH`. Así un cambio solo en `inv.mag` se ve en `top.mag` como cambio vía `inv`.
- **Celda que falta:** queda vacía, con un aviso; el resto se compara.
- **Unidades:** lambda sale del `.tech` del PDK instalado; sin él, 0,01 µm en SKY130 e IHP y 0,05 µm en GF180 (`RIKU_MAG_LAMBDA` la fuerza). `magscale` distintos en una jerarquía van a una grilla común, sin redondeos.
- **La misma geometría partida en otras tiras** (Magic la reescribe al editar) no es un cambio: el diff es por área.
- **Capas:** `metal1`, `viali`, `locali`… con el color y el apilado de la capa GDS equivalente del PDK. Las marcas de DRC y pistas del router (`checkpaint`, `error_*`, `magnet`) quedan fuera.
- **Puertos:** agregados, quitados y cambios de clase, uso, índice, lados o capa (`inv:port:A` · `class: input → inout`); un puerto que solo se movió es cosmético.

Verificado contra KLayout 0.30.12 (las versiones anteriores ignoran `magscale`): mismos polígonos y área por capa en 8 jerarquías de SKY130 y GF180; los 9 281 `.mag` de los PDK se leen sin errores.

### Transistores y redes

En un layout de **SKY130, GF180MCU o IHP SG13G2** (GDS, OASIS o Magic), Riku reconoce los transistores y los resistores, y arma las redes: qué metal, poly y difusión están unidos por contactos y vías. No hay tablas por PDK escritas a mano: todo sale del `.tech` de Magic del PDK instalado (`$PDK_ROOT`), o de una copia compilada si no está (`tools/palettes/gen_devices.py` → `devices_generated.rs`).

| Del `.tech` | Para qué |
|---|---|
| `cifinput` (primer estilo) | de qué capas GDS sale cada tipo de Magic (`nfet` = `DIFF` y `POLY` y `NSDM`, sin `PSDM`…), con `grow`/`shrink`; y en qué capas van las etiquetas y los pines (`labels LIPIN port`) |
| `extract`: `device` | el modelo SPICE de cada transistor o resistor, según W y L (`w<0.42` → `special_nfet_01v8`), y los tipos de fuente, drenaje y cuerpo; `substrate`, qué es el sustrato |
| `contact`, `connect`, `aliases` | qué capas une cada contacto y qué tipos conducen juntos cuando se tocan |
| `types` | los nombres de cada tipo (un `.mag` puede usar cualquiera) |

- **Transistores:** cada compuerta (difusión ∩ poly) es un finger; su tipo es la regla que cumple en un punto interior. W y L, como KLayout: fuente y drenaje son las dos regiones de difusión que toca la compuerta, `W` = el borde con ellas / 2 y `L` = área / W; una compuerta que no separa exactamente dos regiones no es un transistor. En Magic, los transistores ya vienen pintados como capas.
- **Redes:** cada tipo se evalúa como región con su regla (con `grow`/`shrink`, y en el orden de pintado de Magic: en un plano, un tipo posterior tapa al anterior). Los pedazos de tipos que `connect` une y que se tocan son la misma red; el sustrato es una red aunque no esté dibujado. Una etiqueta nombra la red en la que cae; es pin si cae sobre un polígono de pin (o, en Magic, si tiene `port`).
- **Resistores:** el cuerpo entre sus dos terminales (`device resistor`); los de modelo `None` (metal en IHP) son cortos. Diodos y capacitores todavía no.
- **Diff:** los transistores de A y B se emparejan por posición (uno contiene el punto del otro) y se comparan modelo, W y L. Las redes se comparan por lo que existe en los dos lados (las etiquetas y los terminales de los transistores emparejados): una red de A repartida en varias de B es un **abierto**; varias de A en una de B, un **corto**; la misma con otra etiqueta, un renombre. Solo en las celdas con cambios en una capa de las reglas.
- **Límites:** una celda de más de 2 millones de polígonos aplanados (un chip entero) no se analiza; se avisa y se comparan sus sub-celdas. Tiempos: una celda estándar, milisegundos; la SRAM de `examples/GDS/` (2 271 transistores, 976 redes), 2 s.
- **Verificado** ([`desarrollo.md`](desarrollo.md#verificación)): los transistores, iguales a la netlist del PDK en todas las celdas estándar de SKY130 (437) y en todas menos una de GF180MCU e IHP; las redes, con Netgen como un LVS: GF180MCU 219 de 219, SKY130 422 de 427, IHP 68 de 73. En todas las diferencias, Magic extrae lo mismo que Riku: es la netlist del PDK la que no coincide con su layout.
- **Magic cuenta más transistores en la SRAM** (720 `special_pfet_latch` contra 360): los de más tienen L = 0,025 µm y fuente y drenaje en la misma red, la línea de palabra pisando un borde de difusión. Riku y KLayout no los cuentan.

### En el visor

- **Estilo por PDK**, agnóstico: color, nombre y orden de apilado salen del `.lyp` del PDK instalado (`$PDK_ROOT`, `/foss/pdks`, `$PDKPATH`); SKY130, GF180MCU e IHP SG13G2 tienen además tablas curadas, que se usan también sin el PDK instalado. Rol de cada capa: dispositivo (relleno), pozo (tinte tenue), implantes, marcadores y pines (solo contorno).
- **Selector de celdas** con buscador y filtros; `riku gui archivo.gds --cell NOMBRE` abre una celda.
- **Diff:** **Diff** muestra la versión "después" atenuada con lo añadido en verde y lo eliminado en rojo; **Before** y **After**, cada versión. En **Detalles → Cambios**, un clic encuadra el cambio; los abiertos y cortos van primero, en rojo.
- **Transistores y redes:** la capa **Transistores** (oculta al abrir), la red en el tooltip, y un clic en un polígono resalta su red entera (ver [`gui.md`](gui.md#controles)).
- **Cómo se elige el estilo:** de más a menos prioridad, las tablas curadas de SKY130, GF180MCU e IHP (`palette.rs`, en orden de apilado); el `.lyp` del PDK instalado (sin PDK, `palette_generated.rs` de `tools/palettes/gen_palettes.py`); y la paleta genérica con el rol por la convención de datatypes. Las capas de Magic toman el estilo de su capa GDS equivalente (`magic_layers_generated.rs`, o el `cifoutput` del `.tech`). Un PDK nuevo se dibuja sin recompilar.

## Simulaciones de ngspice (`.raw`)

Resultados de ngspice (Berkeley SPICE3, binario o texto), para versionarlos junto al circuito y ver qué cambió en el comportamiento.

- **Análisis:** cada `.raw` trae uno o más (`op`, `tran`, `ac`, `dc`…), emparejados por nombre y orden. Los complejos (`ac`, `noise`) se comparan en magnitud (dB).
- **Señales:** nuevas, que ya no están o cuánto cambiaron. Como dos corridas no usan los mismos pasos de tiempo, las curvas se comparan sobre la unión de las dos grillas, interpolando: **Δmáx** (y dónde), **RMS** y **% del rango**.
- **Cosmético:** Δmáx hasta el **0,1 % del rango** de la señal (piso absoluto 1e-12). Se cambia con `--tolerance 0.5%` o en `.riku.toml`.
- Si cambió la duración, se compara el tramo común y se avisa.

```text
$ riku diff HEAD~1 HEAD rc.raw        # C1 pasó de 100 pF a 120 pF
  ~ v(out)
      Δmáx 120.565 mV en 1.097 µs · RMS 73.251 mV · 6.74 % del rango  (Transient Analysis)
```

En JSON, cada señal es un cambio `{"type": "signal", "plot", "name"}` con `max_abs_diff`, `at`, `rms_diff`, `rel_diff`, `unit` y `x_unit`.

**En el visor**, una vista propia con ejes y unidades: **Diff** (B continua sobre A punteada y, debajo, el error B − A con el eje X enlazado), **Before** y **After** con el mismo zoom; en **Detalles**, qué análisis y señales mostrar, una tabla con el valor en A, en B y el Δ, y **Comparar con** otro `.raw` del proyecto sin pasar por Git. Al acercarse, las curvas se redibujan con todo el detalle del tramo.

### Expresiones

Además de las señales del archivo se comparan **señales calculadas**, con la sintaxis de ngspice (`--expr`, repetible, o `expressions` en `.riku.toml`). Si una expresión da un número (`max(v(out))`), se compara ese número.

```bash
riku diff HEAD~1 HEAD rc.raw --expr "gain = v(out)/v(in)" --expr "tran: vpk = max(v(out))"
```

| Qué | Sintaxis |
|---|---|
| Señales | `v(out)`, `v(a,b)`, `i(v1)`, `@m1[id]`, `time`, `frequency`; `out` a secas es `v(out)` |
| Números | `1.5`, `1e-9`, sufijos SPICE `f p n u m k meg g t`, `pi`, `e` |
| Operadores | `+ − * / ^` y paréntesis |
| Por punto | `abs` (`mag`), `real`, `imag`, `ph` (grados), `db`, `sqrt`, `exp`, `ln`, `log`, `sin`, `cos`, `tan`, `atan`, `max(a, b)`, `min(a, b)` |
| Cálculo | `deriv(v)`, `integ(v)` (integral acumulada) |
| Escalares | `max(v)`, `min(v)`, `pp(v)`, `mean(v)`, `rms(v)`, `integral(v)`, `length(v)`, `at(v, x)` |
| Tramos | `v[0]`, `v[-1]`, `v[10:20]`, `window(v, x0, x1)` |
| Nombre y análisis | `gain = …` nombra el resultado; `tran: …`, `ac: …`, `op: …` lo limitan a ese análisis |

En `ac` se opera con complejos y el resultado se muestra en dB. Las unidades se conservan cuando se pueden deducir. Una expresión se omite en los análisis donde no aplica (solo se avisa si no aplica en ninguno), y los puntos sin valor, como una división por cero, no se comparan (`skipped_points` en el JSON). En el visor, el campo **Expresiones** de **Detalles** las agrega como curvas (con `ƒ`) o como **Mediciones**, y se recuerdan entre sesiones.
