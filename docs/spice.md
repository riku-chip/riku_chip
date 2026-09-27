# Simulación: formas de onda de ngspice (`.raw`)

El módulo `spice` (`riku/src/modules/spice/`, feature `spice`, activada por defecto) compara los resultados de simulación que escribe ngspice. Sirve para versionar el `.raw` junto al circuito y ver qué cambió en el comportamiento, no solo en el esquemático.

## Qué compara

- **Archivo:** un `.raw` de ngspice (formato de Berkeley SPICE3), binario (`set filetype=binary`) o de texto. Se reconoce por la extensión o por su cabecera.
- **Análisis:** cada `.raw` trae uno o más *plots* (`op`, `tran`, `ac`, `dc`…); se emparejan por nombre (`Transient Analysis`, `AC Analysis`…) y orden. Los análisis complejos (`ac`, `noise`) se comparan en magnitud (dB).
- **Señales:** por cada señal (`v(out)`, `i(v1)`) se reporta si es nueva, si ya no está o cuánto cambió. Dos corridas no usan los mismos pasos de tiempo (ngspice ajusta el paso a la actividad del circuito), así que las curvas se comparan sobre la unión de las dos grillas, interpolando cada una:
  - **Δmáx:** el error máximo |B − A| y en qué punto del eje ocurre.
  - **RMS:** el error cuadrático medio, ponderado por el paso.
  - **% del rango:** Δmáx relativo al rango (máximo − mínimo) de la señal.
- **Cosmético:** un cambio es cosmético si Δmáx no pasa del **0,1 % del rango** de la señal (con un piso absoluto de 1e-12 para señales casi constantes). Así, volver a simular sin cambios reales no ensucia el reporte.
- Si el eje cambió (otra duración de la simulación), se compara el tramo común y se avisa.

## En la CLI

Igual que los demás formatos: `riku diff`, `riku show`, `riku log` y `riku status` ([`cli.md`](cli.md)).

```text
$ riku diff HEAD~1 HEAD rc.raw        # C1 pasó de 100 pF a 120 pF
Archivo : rc.raw
Cambios : 2
Cosméticos: 1

  ~ v(out)
      Δmáx 120.565 mV en 1.097 µs · RMS 73.251 mV · 6.74 % del rango  (Transient Analysis)
  ~ i(v1)
      Δmáx 12.056 µA en 1.097 µs · RMS 7.325 µA · 3.36 % del rango  (Transient Analysis)
```

El cosmético (`v(in)`, la fuente, idéntica) no se lista en el texto. En JSON (`-f json`) cada señal es un cambio con `element: {"type": "signal", "plot", "name"}` y los detalles `max_abs_diff`, `at`, `rms_diff`, `rel_diff` (fracción del rango), `unit` y `x_unit`. Con `--ci`, un cambio de más del 0,1 % cuenta como funcional (código 1).

## En el visor

`riku gui sim.raw` abre la vista de formas de onda; `riku diff A B sim.raw -f visual` la abre comparando dos commits. No pasa por `ViewerBackend` (que dibuja planos): una curva necesita ejes con unidades, escala logarítmica en frecuencia y zoom independiente en X e Y (`egui_plot`).

- **Diff / Before / After:** como en los demás formatos. **Diff** muestra B continua sobre A punteada y, debajo, el error B − A con el eje X enlazado. Las tres vistas encuadran igual y conservan el zoom: alternar **Before**/**After** sobre un tramo muestra cómo cambia la curva.
- **Detalles:** análisis a mostrar, señales (al abrir, las tres primeras; **Solo las que cambiaron**, **Ocultar nodos internos** de dispositivos y subcircuitos), **Mostrar error (B − A)** y una tabla con el valor de cada señal en A y B y su Δ.
- **Comparar con:** con un `.raw` abierto suelto, elegir otro `.raw` del proyecto para compararlos sin pasar por Git (por ejemplo, dos esquinas de simulación).
- Cada curva se reduce a 4 000 puntos para dibujarla (se conservan el mínimo y el máximo de cada tramo); la comparación usa todos los puntos.

## Expresiones

Además de las señales del archivo se pueden comparar **señales calculadas**, con la sintaxis de ngspice. Cada expresión se evalúa en A y en B, en cada análisis que tenga sus señales, y se compara igual que una señal más (misma tolerancia). Si da un número en vez de una curva (`max(v(out))`, `v(out)[0]`), se compara ese número.

```text
$ riku diff HEAD~1 HEAD rc.raw --expr "gain = v(out)/v(in)" --expr "tran: vpk = max(v(out))" --expr "tran: slew = max(deriv(v(out)))"
  ~ gain
      = v(out)/v(in)
      Δmáx 1.584 dB en 100.000 MHz · RMS 1.581 dB · 2.75 % del rango  (AC Analysis)
  ~ vpk
      = max(v(out))
      1.788 V → 1.773 V · Δ 15.428 mV (0.86 %)  (Transient Analysis)
  ~ slew
      = max(deriv(v(out)))
      1.799 M → 1.499 M · Δ 299685 (16.66 %)  (Transient Analysis)
```

| Qué | Sintaxis |
|---|---|
| Señales | `v(out)`, `v(a,b)` (= `v(a) − v(b)`), `i(v1)`, `@m1[id]`, `time`, `frequency`; un nodo a secas (`out`) es `v(out)` |
| Números | `1.5`, `1e-9`, sufijos SPICE `f p n u m k meg g t` (`10u`, `2meg`, `100nF`), `pi`, `e` |
| Operadores | `+ − * / ^` y paréntesis |
| Por punto | `abs` (`mag`), `real`, `imag`, `ph` (grados), `db`, `sqrt`, `exp`, `ln`, `log` (= `log10`), `sin`, `cos`, `tan`, `atan`, `max(a, b)`, `min(a, b)` |
| Cálculo | `deriv(v)`: derivada respecto del eje (tiempo o frecuencia); `integ(v)`: integral acumulada por trapecios |
| Escalares | `max(v)`, `min(v)`, `pp(v)` (pico a pico), `mean(v)`, `rms(v)` (en el eje), `integral(v)` (total), `length(v)`, `at(v, x)` (valor interpolado en `x`) |
| Índices | `v[0]`, `v[-1]` (el último), `v[10:20]` (tramo por índice; el resto no se compara), `window(v, x0, x1)` (tramo por valor del eje) |
| Nombre | `gain = v(out)/v(in)`: el reporte y el visor usan `gain` |
| Análisis | `tran: …`, `ac: …`, `op: …`, `dc: …`, `noise: …`: evaluar solo en ese análisis (sin prefijo, en todos donde existan las señales) |

- **Complejos:** en `ac` se opera con los valores complejos (`v(out)/v(in)` divide complejos) y el resultado se muestra en dB; `ph()` da la fase en grados y `mag()` la magnitud lineal.
- **Unidades:** se conservan cuando se pueden deducir (`2*v(out)` en V, `max(i(v1))` en A, `db()` en dB, `ph()` en °); el resto sale sin unidad.
- **Donde no aplica:** una expresión se omite en un análisis que no tiene sus señales o donde no da ningún valor válido (`v(out)/v(in)` en un punto de operación con `v(in) = 0`). Solo se avisa si no se pudo aplicar en ninguno. Los puntos sin valor (división por cero, fuera de un tramo) no se comparan; el JSON lo informa en `skipped_points`.
- **JSON:** una señal calculada lleva además `expression` (la fórmula); un escalar lleva `value` con antes y después.

En el visor, el panel **Detalles** tiene un campo **Expresiones**: las que dan una curva aparecen en la lista de señales con `ƒ` y se grafican como las demás (con Diff / Before / After y el error B − A); las que dan un número aparecen en **Mediciones** con A, B y Δ. Las expresiones se recuerdan entre sesiones, y `riku diff … -f visual --expr …` las abre ya cargadas.
