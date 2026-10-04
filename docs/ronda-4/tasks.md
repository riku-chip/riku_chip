# Ronda 4: tareas

Marcar `[x]` al terminar. Requisitos en [`requirements.md`](requirements.md), diseño en [`design.md`](design.md). Compilar y probar en el contenedor; editar y commitear en `Documents`. Antes: `git pull`, rama `ronda-4` desde `main`, `cargo test --workspace --locked` en verde como base (457 pasan, 5 ignoradas al cerrar la ronda 3).

## T16. Reglas `device … +tipos` (R16) · commit `fix(layout):`

- [ ] **T16.1** Línea de base: netlists de `lvs_probe` de las 437 celdas `sky130_fd_sc_hd`, `riku lvs` del demo `ota` y la salida de `tools/verify/devices/` y `tools/verify/nets/`, guardadas fuera del repo.
- [ ] **T16.2** Pruebas primero en `devices/rules.rs`: una línea `device msubcircuit Ignore mvnfet … +npn,pnp` seguida de la del `nfet_g5v0d10v5`; sin `npn`/`pnp` cerca se elige la segunda; con ellos, `Ignore`. Ver que falla.
- [ ] **T16.3** `ModelRule` con `near`; lectura de `+tipos`; `model_for` y `model` (D16).
- [ ] **T16.4** En `extract.rs`, `near_ok` por compuerta y no listar los `Ignore`.
- [ ] **T16.5** El inversor de `demo_sky130A`: `riku lvs` coincide; los transistores con su modelo y W/L.
- [ ] **T16.6** Repetir T16.1: mismas netlists y resultados.
- [ ] **T16.7** Ejemplos `nets` y `lvs_probe` con la carpeta del `.mag` (D16) · commit aparte `fix(examples):`.

## T17. Demo `inversor` (R17) · commit `feat(demo):`

- [ ] **T17.1** `tools/demos/inversor.py`: el repo con los archivos y la licencia de D17, y el commit inicial.
- [ ] **T17.2** Cada edición de la tabla de D17, con las medidas calculadas desde la grilla; `lvs_probe` confirma los W.
- [ ] **T17.3** La comprobación al final del script: `riku log --lvs -f json` contra la tabla esperada, y Magic `extract` + Netgen en cada commit con el mismo veredicto. Si falla, no hay bundle.
- [ ] **T17.4** README del repo con los comandos y lo que se ve (salidas reales).
- [ ] **T17.5** Bundle en `examples/demos/inversor.bundle`; entrada en `DEMOS`; `demo.about.inversor` en `es.yml`/`en.yml`; la prueba de `demo.rs` comprueba el inversor.
- [ ] **T17.6** A mano: `riku demo inversor`, cada comando del README; el visor (Historial y vista de LVS) con capturas.

## T18. Demo `chip` (R18)

- [ ] **T18.1** Confirmar la licencia de `sky130_sram_macros` en su fuente; si no sirve, elegir otro macro y anotarlo.
- [ ] **T18.2** `tools/demos/chip.py`: el repo de D18 con KLayout; medir su tamaño tras `git gc --aggressive` (meta: < 40 MB).
- [ ] **T18.3** `Source::Remote`, `RIKU_DEMO_CHIP_URL`, `riku demo` sin nombre no clona `chip`, `--list` con el tamaño, error con la URL · commit `feat(demo):`.
- [ ] **T18.4** Prueba sin red con un repo local (`file://`).
- [ ] **T18.5** Mediciones de D18 con el binario de release; README del repo del chip con los tiempos.
- [ ] **T18.6** **Pedir permiso** para crear `riku-chip/riku-demo-chip` y subirlo (o que lo haga el usuario); después, `riku demo chip` desde la URL real.

## T19. Documentación (R19) · commit `docs:`

- [ ] **T19.1** `README.md` y `docs/cli.md`: los cuatro demos.
- [ ] **T19.2** `docs/desarrollo.md`: regenerar demos y los tiempos del chip.
- [ ] **T19.3** `docs/pendientes.md`: quitar las filas de demos; anotar lo que quede.

## Cierre

- [ ] **C1** `cargo fmt --check`, `cargo test --workspace --locked` (`-D warnings`) y las combinaciones de features en verde.
- [ ] **C2** Merge a `main` cuando lo pidas.
