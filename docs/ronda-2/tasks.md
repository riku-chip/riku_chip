# Ronda 2: tareas

Marcar `[x]` al terminar. Requisitos en [`requirements.md`](requirements.md), diseño en [`design.md`](design.md). Compilar y probar en el contenedor (`docs/desarrollo.md`); editar y commitear en `Documents`. Antes: `git pull`, rama `ronda-2` desde `main`, `cargo test --workspace --locked` en verde como base (440 pasan, 5 ignoradas al cerrar la ronda 1).

## T6. `LayoutNets` guarda los nombres SPICE, las compuertas y los grupos (R6) · commit `feat(layout):`

- [x] **T6.1** Pruebas primero en `nets/probe.rs` con una `Netlist` a mano (dos transistores en paralelo, uno suelto, un resistor, una red sin nombre y el sustrato): los mapas de D6 tienen lo esperado. Ver que no compilan o fallan.
- [x] **T6.2** Campos `spice_nets`, `gates`, `group_of`, `bodies` armados en `LayoutNets::new` (D6). `names` y `at` sin cambios.
- [x] **T6.3** ~~Aviso si dos redes comparten nombre SPICE.~~ Cambió: dos redes con la misma etiqueta son una en la netlist, así que `net_named` devuelve las dos (design.md D6).
- [x] **T6.4** Prueba de no desfase con `spice()` (D6, R8.4).

## T6b. El visor extrae como el LVS (R6.4) · commit `fix(layout):`

- [x] **T6b.1** Seguir el camino de lectura del visor hasta `add_electrical` y ver dónde está el `MagInfo` de un `.mag`.
- [x] **T6b.2** Pasarlo a `cell_nets` (o anotar por qué no se puede en ese camino).
- [x] **T6b.3** Con un `.mag` de los ejemplos que tenga transistores: los nombres de `riku lvs` (o de `nets::spice`) están en la sonda. Si no hay `.mag` con par de esquemático, comparar `layout_spice` contra la sonda en una prueba.

## T7. `net_named` y `device_named` (R7) · commit `feat(layout):` (puede ir junto con T6)

- [x] **T7.1** Pruebas: `parse_device` (tabla de D7); `net_named` exacto, sin mayúsculas, ambiguo (`None`), inexistente; `device_named` de un finger devuelve todo el grupo; resistor; índice fuera de rango.
- [x] **T7.2** Implementar `parse_device`, `net_named`, `device_named` (D7).
- [x] **T7.3** `cargo test -p riku-mod-layout` y el workspace con `-D warnings`.

## T7b. El aviso y la documentación (R7.6) · commit `docs(lvs):`

- [x] **T7b.1** Texto nuevo de `lvs_view.layout_unplaced` en `es.yml` y `en.yml` (D7b); `cargo test -p riku i18n`.
- [x] **T7b.2** `docs/lvs.md`: "Lado del layout" con lo hecho; quitar "Lo que falta del layout"; quitar la frase "nada de esto existe todavía como comando" del principio (`riku lvs` existe).

## T8. Probado en el demo `ota` (R8)

- [x] **T8.1** `riku demo ota` en `/tmp`; `riku lvs -f json` en `HEAD` y en `120ee0b` (anotar los nombres del layout: hoy `19`, `20`, `7`, `9`, `0` y la red `Vout`).
- [x] **T8.2** Con `examples` o una prueba `#[ignore]`: para cada nombre del layout, cuántas compuertas devuelve `device_named` y que la suma de sus W sea el W que da Netgen (19 µm para `7` y `9`, 2 µm para `19` y `20`, 19 µm para `0`).
- [x] **T8.3** GUI en el contenedor (XTest, `riku gui .` desde la raíz del demo, esperar ~30 s en debug): abrir la vista de LVS con `ota-5t.sch` o `ota-5t.gds` abierto, elegir cada fila y capturar. En el layout: compuertas resaltadas y encuadradas, resto atenuado; sin el aviso de "no se encontró".
- [x] **T8.4** Lo mismo en `120ee0b` (`git checkout` en la copia del demo): la red `Vout` resaltada entera, con el tramo que toca a `Vp`.
- [x] **T8.5** Guardar las capturas fuera del repo y describir lo visto en el mensaje del commit o en el PR.

## Cierre

- [x] **C1** `cargo fmt --check` y `cargo test --workspace --locked` (`-D warnings`) en verde.
- [x] **C2** `docs/pendientes.md`: si queda algo (p. ej. dispositivos de sub-celdas, o el clic que resalta la pareja aunque coincida), anotarlo.
- [ ] **C3** Merge a `main` cuando lo pidas.

## Cómo terminó (2026-10-03)

- **Probado en el demo `ota`** con `examples/lvs_probe` y en el visor (XTest):
  - `HEAD`: `M1 ↔ 19` y `M2 ↔ 20` marcan 2 compuertas pfet cada uno (W 2 µm); `M3 ↔ 7` y `M4 ↔ 9`, 4 compuertas nfet (W 19 µm); `M8 ↔ 0`, 6 compuertas: los rellenos de L 0,5 µm junto al par (W 19 µm, el que informa Netgen) y los de L 1 µm de la rama de cola, que Netgen juntó en el mismo dispositivo.
  - `120ee0b`: la red sin pareja marca `Vout` entera, con el tramo nuevo que toca el pin `Vp`.
- **Lo que cambió del diseño al probar:** `fingers()` daba 9 grupos y Netgen 8 → `parallel_groups()` sin mirar L. Dos redes con la misma etiqueta → una sola en la netlist.
- **Magic (T6b):** en las 437 celdas `sky130_fd_sc_hd` los nombres de red son iguales con y sin `MagInfo`; en `nor4bb_4` la etiqueta `k` dejaba de ser pin. El visor ahora usa la misma información que `riku lvs`.
- **Pruebas:** 444 pasan (440 + 4 de la sonda); dos mutaciones (sin agrupar, nombres del tooltip) las hacen fallar.
