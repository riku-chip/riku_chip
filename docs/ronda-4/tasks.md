# Ronda 4: tareas

Marcar `[x]` al terminar. Requisitos en [`requirements.md`](requirements.md), diseño en [`design.md`](design.md). Compilar y probar en el contenedor; editar y commitear en `Documents`. Antes: `git pull`, rama `ronda-4` desde `main`, `cargo test --workspace --locked` en verde como base (457 pasan, 5 ignoradas al cerrar la ronda 3).

## T16. Reglas `device … +tipos` (R16) · commit `fix(layout):`

- [x] **T16.1** Línea de base: netlists de `lvs_probe` de las 437 celdas `sky130_fd_sc_hd`, `riku lvs` del demo `ota` y la salida de `tools/verify/devices/` y `tools/verify/nets/`, guardadas fuera del repo.
- [x] **T16.2** Pruebas primero en `devices/rules.rs`: una línea `device msubcircuit Ignore mvnfet … +npn,pnp` seguida de la del `nfet_g5v0d10v5`; sin `npn`/`pnp` cerca se elige la segunda; con ellos, `Ignore`. Ver que falla.
- [x] **T16.3** `ModelRule` con `near`; lectura de `+tipos`; `model_for` y `model` (D16).
- [x] **T16.4** En `extract.rs`, `near_ok` por compuerta y no listar los `Ignore`.
- [x] **T16.5** El inversor de `demo_sky130A`: `riku lvs` coincide; los transistores con su modelo y W/L.
- [x] **T16.6** Repetir T16.1: mismas netlists y resultados.
- [x] **T16.7** Ejemplos `nets` y `lvs_probe` con la carpeta del `.mag` (D16) · commit aparte `fix(examples):`.

## T17. Demo `inversor` (R17) · commit `feat(demo):`

- [x] **T17.1** `tools/demos/inversor.py`: el repo con los archivos y la licencia de D17, y el commit inicial.
- [x] **T17.2** Cada edición de la tabla de D17, con las medidas calculadas desde la grilla; `lvs_probe` confirma los W.
- [x] **T17.3** La comprobación al final del script: `riku log --lvs -f json` contra la tabla esperada, y Magic `extract` + Netgen en cada commit con el mismo veredicto. Si falla, no hay bundle.
- [x] **T17.4** README del repo con los comandos y lo que se ve (salidas reales).
- [x] **T17.5** Bundle en `examples/demos/inversor.bundle`; entrada en `DEMOS`; `demo.about.inversor` en `es.yml`/`en.yml`; la prueba de `demo.rs` comprueba el inversor.
- [x] **T17.6** (visor: layout con capas por nombre; LVS coincide en main; en el commit del abierto, la red out sin pareja marcada en los dos lados; Historial con la rama y el merge, y Tab + Enter abren el diff con el abierto marcado) A mano: `riku demo inversor`, cada comando del README; el visor (Historial y vista de LVS) con capturas.

## T18. Demo `chip` (R18)

- [x] **T18.1** Confirmar la licencia de `sky130_sram_macros` en su fuente; si no sirve, elegir otro macro y anotarlo.
- [x] **T18.2** `tools/demos/chip.py`: el repo de D18 con KLayout; medir su tamaño tras `git gc --aggressive` (meta: < 40 MB).
- [x] **T18.3** (no aplica: el chip va embebido) `Source::Remote`, `RIKU_DEMO_CHIP_URL`, `riku demo` sin nombre no clona `chip`, `--list` con el tamaño, error con la URL · commit `feat(demo):`.
- [x] **T18.4** (no aplica: el chip va embebido) Prueba sin red con un repo local (`file://`).
- [x] **T18.5** Mediciones de D18 con el binario de release; README del repo del chip con los tiempos.
- [x] **T18.6** (no aplica: el chip va embebido) **Pedir permiso** para crear `riku-chip/riku-demo-chip` y subirlo (o que lo haga el usuario); después, `riku demo chip` desde la URL real.

## T19. Documentación (R19) · commit `docs:`

- [x] **T19.1** `README.md` y `docs/cli.md`: los cuatro demos.
- [x] **T19.2** `docs/desarrollo.md`: regenerar demos y los tiempos del chip.
- [x] **T19.3** `docs/pendientes.md`: quitar las filas de demos; anotar lo que quede.

## Cierre

- [x] **C1** `cargo fmt --check`, `cargo test --workspace --locked` (`-D warnings`) y las combinaciones de features en verde.
- [ ] **C2** Merge a `main` cuando lo pidas.

## Cómo terminó (2026-10-04)

- **T16, más de lo previsto.** Para que el inversor coincida hicieron falta dos reglas del `.tech` que Riku no evaluaba: `+npn,pnp` (la parte de un bipolar que se ignora) y los terminales distintos del transistor de drenaje extendido (`g5v0d16v0`). Sin cambios en las celdas estándar de los tres PDK (`tools/verify`, antes y después) ni en el demo `ota`.
- **Dos arreglos que salieron al armar el demo**, comprobados contra Magic: una etiqueta de puerto de Magic es de lo que hay bajo su rectángulo (Riku la anclaba en un punto del borde y la perdía), y un abierto en la celda de arriba que cae sobre una instancia sin cambios ahora se ve en el diff de redes. `riku-mod-layout` 0.4.5.
- **Inversor:** 11 commits (la rama es el nmos más largo: el pmos no se puede ensanchar sin re-rutear). El script comprueba cada commit con `riku log --lvs`, `riku show` y Magic; bundle de 14 KiB.
- **Chip:** el repo pesó 1,4 MB, así que se embebió (decisión del usuario); licencia Apache-2.0 confirmada en `VLSIDA/sky130_sram_macros`. Tiempos en su README y en `desarrollo.md`.
- **Pruebas:** 458 pasan, 0 fallan, 5 ignoradas; `cargo fmt --check` y las combinaciones de features de la CI en verde.
