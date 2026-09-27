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
