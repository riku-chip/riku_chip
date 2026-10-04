# Ronda 2: requisitos

Numerados como las tareas del [`plan.md`](plan.md) (6, 7, 8). Cada uno con un criterio comprobable.

## R6. `LayoutNets` conoce los nombres de la netlist del LVS

**Historia.** Como quien revisa un LVS, quiero que el layout sepa qué red y qué transistor son los que nombra Netgen, para verlos en el dibujo.

- **R6.1** Para cada red de la celda, `LayoutNets` sabe su nombre en la netlist SPICE que compara Netgen: `Netlist::net_name(i)` (la etiqueta con espacios cambiados por `_`, `VSUBS` o `n<i>`).
- **R6.2** Para cada transistor (índice en `Netlist::devices`, el mismo número que escribe `nets::spice`), sabe el contorno de su compuerta (`Device::gate`). Para cada resistor (índice en `Netlist::resistors`), el de su cuerpo.
- **R6.3** Para cada transistor, sabe con qué otros forma un grupo de fingers en paralelo (`nets::fingers`), porque Netgen los junta y nombra al grupo por uno solo.
- **R6.4** El visor extrae las redes igual que `riku lvs`: misma función (`cell_nets`), misma celda y, en un layout de Magic, la misma información del lector (`MagInfo`, que dice qué etiquetas son pines y cambia qué nombre lleva una red). Un nombre que da Netgen existe en la sonda.
- **R6.5** Sin extraer dos veces ni guardar geometría de más: los mapas se arman con la `Netlist` que el visor ya calcula. Las compuertas se copian (son pocos polígonos por transistor).
- **R6.6** `at` (la red bajo el cursor) y su nombre en el tooltip no cambian.

## R7. `net_named` y `device_named`

**Historia.** Como quien elige una discrepancia en la lista de LVS, quiero verla resaltada y encuadrada en el layout, como en el esquemático.

- **R7.1** `net_named(n)` devuelve la red cuyo nombre SPICE es `n`, con todos sus pedazos (lo mismo que dibuja `at`).
- **R7.2** Si no hay coincidencia exacta, prueba sin distinguir mayúsculas, y solo si eso da una única red (Netgen no cambia las mayúsculas en el demo, pero un PDK puede tener nombres que solo difieren en eso).
- **R7.3** `device_named(n)` acepta `19`, `X19` y `M19` (transistor 19) y `R3`/`XR3` (resistor 3). Devuelve el contorno de la compuerta del transistor **y de los demás fingers de su grupo**; de un resistor, su cuerpo.
- **R7.4** Un nombre que no existe, un número fuera de rango o un texto que no se entiende devuelve `None`, sin pánico.
- **R7.5** Con esto, elegir en la lista una diferencia de parámetros, una red o un dispositivo sin pareja resalta el lado del layout, lo atenúa y lo encuadra con contexto, sin cambiar la GUI.
- **R7.6** El aviso de la lista (`lvs_view.layout_unplaced`) deja de decir que el layout "todavía" no sabe: dice que no se encontró en el layout (una celda demasiado grande para calcular sus redes, un layout sin PDK conocido o un nombre que no está).

## R8. Probado en el demo `ota`

- **R8.1** En `HEAD`: elegir `M1 ↔ 19` resalta las compuertas del grupo pfet de M1 en el layout (no un finger suelto); lo mismo `M2 ↔ 20`, `M3 ↔ 7`, `M4 ↔ 9` y `M8 ↔ 0`. Los grupos de M1 y M2 son distintos.
- **R8.2** En `120ee0b`: elegir la red sin pareja resalta en el layout la red `Vout` entera, que incluye el tramo que toca a `Vp`.
- **R8.3** La cantidad de compuertas resaltadas por grupo coincide con la cantidad de fingers que suman el W que da Netgen (p. ej. el de `9` suma 19 µm).
- **R8.4** Hay pruebas automáticas en `riku-mod-layout` de R6–R7 con una `Netlist` armada a mano, y una que compara los nombres de la sonda con los que escribe `spice()` para la misma netlist (que no se desfasen).

## No funcionales

- **NF1** Sin cambios en `viewer-core` ni en el crate de Carlos.
- **NF2** Sin cambios en la salida de `riku lvs`, `diff` o `log`.
- **NF3** Sin dependencias nuevas.
