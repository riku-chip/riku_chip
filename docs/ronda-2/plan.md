# Ronda 2: el LVS del lado del layout

Que la vista de LVS resalte en el layout lo que Netgen dice que no coincide, igual que ya lo hace en el esquemático (*cross-probing* en los dos lados). Es la parte que [`lvs.md`](../lvs.md) le asigna al lado de layouts. Documentos: [`requirements.md`](requirements.md) (qué), [`design.md`](design.md) (cómo), [`tasks.md`](tasks.md) (pasos).

## Qué hay hoy (revisado el 2026-10-03)

- La vista de LVS (`riku/src/gui/lvs_view.rs`) ya le pide al `NetProbe` de la escena del layout `net_named(nombre)` y `device_named(nombre)`, y pinta lo que vuelve. Si vuelve `None`, la lista avisa que el layout no sabe ubicarlo.
- `viewer-core` define esos dos métodos con `None` por defecto. `LayoutNets` (`riku-mod-layout/src/nets/probe.rs`) solo implementa `at` (la red bajo el cursor), y nombra las redes con `net_label`, que no es el nombre de la netlist que ve Netgen.

## Lo que se midió en el demo `ota` antes de diseñar

`riku lvs -f json` en `HEAD`:

| Del lado del layout, Netgen dice | Del esquemático | Parámetro |
|---|---|---|
| `19`, `20` (pfet) | `M1`, `M2` | W 2 ≠ 4 |
| `7`, `9` (nfet) | `M3`, `M4` | W 19 ≠ 18 |
| `0` (nfet) | `M8` | W 19 ≠ 20 |

- **Los dispositivos llegan como un índice pelado** (`19`), sin `X` ni `M`: `parse_netgen` ya le saca el modelo (`sky130_fd_pr__pfet_01v8:19` → `19`).
- **Netgen junta los fingers en paralelo.** El layout tiene 24 transistores y Netgen cuenta 8 (5 N, 3 P). El `W=19` de `9` es la suma de un grupo. Resaltar solo el transistor 9 mostraría un finger de cuatro.
- **Las redes llegan con el nombre de la netlist.** En `120ee0b` (Vout toca Vp) Netgen da `Vout` del lado del layout contra `Vout, Vp` del esquemático. `HEAD` no tiene redes sin pareja, así que `net_named` se prueba en ese commit.
- Hay una tercera discrepancia que la consigna no nombraba: `M8` (20 contra 19).

## Orden

| # | Tarea | Toca | Esf. |
|---|---|---|---|
| 6 | `LayoutNets` guarda el nombre SPICE de cada red, la compuerta de cada transistor (y el cuerpo de cada resistor) y su grupo de fingers | `riku-mod-layout/src/nets/probe.rs` | S |
| 6b | El visor extrae las redes igual que el LVS (la información de Magic) | `riku-mod-layout/src/viewer_core_compat.rs` | S |
| 7 | `net_named` y `device_named` | `nets/probe.rs` | S |
| 7b | El aviso de la lista ya no dice "todavía": dice que no se encontró | `riku/locales/*.yml` | S |
| 8 | Probarlo en el demo `ota`: `HEAD` (parámetros) y `120ee0b` (redes) | — | M |

Sin cambios en `viewer-core` (el contrato ya está) ni en la GUI (`lvs_view.rs` ya llama a los dos métodos): la ronda es de `riku-mod-layout`, como dice el reparto de `lvs.md`.

## Fuera de esta ronda

- Un clic en una red o un transistor de un lado que resalte su pareja en el otro aunque coincida (Fase 3 de `lvs.md`): pide la tabla de equivalencias de Netgen, que hoy no se lee.
- Ubicar dispositivos de sub-celdas: la extracción es por celda aplanada, como el LVS.
- Leer del `comp.json` los dispositivos que Netgen juntó: se reconstruye el grupo con `fingers()` (ver diseño).
