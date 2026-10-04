# Ronda 5: tareas

Marcar `[x]` al terminar. Requisitos en [`requirements.md`](requirements.md), diseño en [`design.md`](design.md).

Compilar y probar en el contenedor (`sync-riku.sh`, target `/headless/riku-target/ws`, `RUSTFLAGS="-D warnings"`); editar y commitear en `Documents`. Antes de empezar:

1. `git pull`.
2. La rama `ronda-5` desde `main`.
3. `cargo test --workspace --locked` en verde como base: 458 pasan y 5 ignoradas al cerrar la ronda 4.

Cada etapa deja `main` utilizable: hasta T24, el diff, el visor y el LVS siguen con la extracción plana.

## T20. Línea de base · sin commit

- [ ] **T20.1** **Tiempos y memoria** de R24.3 con el binario de release, en `/tmp`, con `RIKU_PROFILE=1` y `/usr/bin/time -v`. Se guardan fuera del repo (`eda/designs/ronda5-base/`):
  - el demo `chip`: `show` del bitcell, `log -n 10`, `diff v1.0 HEAD`, con la caché vacía y llena;
  - la SRAM de `examples/`: el commit de relleno;
  - el visor: abrir la SRAM.
- [ ] **T20.2** **Netlists planas de hoy**, para comparar en T22 y en T25:
  - con `lvs_probe`: las 437 `sky130_fd_sc_hd`, la SRAM de `examples/GDS/`, los demos `ota`, `sram`, `inversor` y las sub-celdas de la macro del demo `chip`;
  - las salidas de `tools/verify/devices/` y `nets/`;
  - `riku lvs` y `log --lvs -f json` de `ota` e `inversor`.
- [ ] **T20.3** **Cuántos polígonos** aplanados tiene la macro del demo `chip`, y si Magic la extrae entera en el contenedor (tiempo). Si Magic no puede, R24.2 se hace con su celda más grande que sí pueda.

## T21. El resumen de una celda (R20.1, R20.4, R20.5) · commit `feat(layout):`

- [ ] **T21.1** **Pruebas primero** (`nets/hier/`), con `same_netlist` (D20.5), contra la extracción plana. Ver que fallan:
  - dos instancias de un inversor que se tocan por `li`;
  - un padre con metal que une dos hijas;
  - una etiqueta del padre sobre el metal de una hija;
  - un AREF 4×4 con espejos;
  - la SRAM de `examples/GDS/`;
  - el inversor del demo (Magic).
- [ ] **T21.2** **Lo común con la plana.** `build_with` acepta pedazos ajenos con su nodo fijado (D20.1 paso 3). `cell_nets_in` separa "geometría → regiones y transistores" de "armar las redes", para usarlo con la geometría propia. Las pruebas de hoy siguen igual.
- [ ] **T21.3** `CellNets`, `NetRef`, `Inst`, `Transform`; `raw.rs` (la grilla por capa y la búsqueda de un punto bajando por la jerarquía); `query::pieces_in` (D20.2).
- [ ] **T21.4** `cell.rs`, pasos 1–6 de D20.1, por niveles con `rayon`:
  - **transformaciones raras:** magnificación o ángulo no recto → se aplana;
  - **ciclos y celdas sin huella:** se extraen planos.
- [ ] **T21.5** Las vecindades memorizadas (D20.3), con el conteo de calculadas y reusadas en `stats`.
- [ ] **T21.6** `HierNets`, `extract`, `flatten`, `flat_size`, `stats` y el orden determinista (D20.5). Prueba: la misma netlist en paralelo y con `RAYON_NUM_THREADS=1`.
- [ ] **T21.7** El ejemplo `hier_check` (R24.1).

## T22. Contexto y sustrato (R20.2, R20.3) · commit `feat(layout):`

- [ ] **T22.1** **Pruebas primero**, cada una igual a la plana:
  - un implante del padre sobre la difusión de una hija;
  - un poly del padre sobre la difusión de una hija;
  - un pozo P de la hija bajo un `dnwell` del padre;
  - un cuerpo sin pozo.
- [ ] **T22.2** `DeviceRules::context_pairs` y la huella de contexto de cada celda; la prueba gruesa y la fina (D20.4).
- [ ] **T22.3** Sustrato: `sub_open`, `sub_pin`, la exclusión en el padre y cerrar la raíz (D20.4, D20.5).
- [ ] **T22.4** **`tools/verify/nets/hier.sh`:**
  - `hier_check` sobre lo de T20.2 (tres PDK);
  - Netgen comparando la SPICE plana de hoy contra la de `flatten`.

  Todo igual. Si algo difiere: el arreglo, una prueba que lo reproduce y, si es una regla no local, el par en `context_pairs`.
- [ ] **T22.5** **Medir la extracción jerárquica** de la macro del demo `chip`: tiempo, memoria, instancias aplanadas por contexto, vecindades. Si se aplana demasiado, refinar (Riesgos).
- [ ] **T22.6** **`tools/verify/nets/chip_vs_magic.sh`** (R24.2). La SPICE jerárquica se escribe recién en T25.2: acá se compara `spice(flatten())` si la macro cabe en memoria, o se deja el script listo para T25.

## T23. Memoria por huella (R21) · commit `feat(layout):`

- [ ] **T23.1** **Pruebas primero:**
  - la misma `NetKey` en dos librerías con la misma celda;
  - otra etiqueta, otro puerto de Magic, otra unidad u otras reglas dan otra clave;
  - el mismo resultado saliendo de memoria, del disco o recalculado;
  - una entrada corrupta se recalcula.
- [ ] **T23.2** `NetKey` en la pasada de `tree_prints` (D21); `DeviceRules::fingerprint`; `riku-mod-layout` a 0.5.0.
- [ ] **T23.3** `Memo` en memoria, con su tope (`RIKU_NETS_MEM_MB`) y sacando lo menos usado; las vecindades aparte.
- [ ] **T23.4** **Corte temprano** (D20.6, R21.5):
  - pruebas primero con la historia sintética (contacto interior, en el borde, corto interior entre puertos);
  - `query.rs` registra cada consulta en `Deps`;
  - `iface`, la numeración estable de las redes y repetir las consultas;
  - el alias en `Memo`, `RIKU_NO_CUTOFF=1` y el conteo en `stats`;
  - la prueba de oro con y sin corte.
- [ ] **T23.5** El disco, en `riku/nets/`:
  - coordenadas enteras;
  - se guarda lo que tarda ≥ 20 ms;
  - se lee recién cuando hace falta;
  - tope de 512 MB y `prune`;
  - `RIKU_NO_CACHE` y `--no-cache`.

## T24. El diff por celda (R22) · commit `feat(diff):`

- [ ] **T24.1** **Pruebas primero**, junto a las de hoy de `gds_diff.rs` y `nets/diff.rs`:
  - un cambio en la celda de arriba de un layout que no cabe aplanado (hoy no se compara): el abierto se ve;
  - una red sin etiqueta se nombra por la hija (R22.5).
- [ ] **T24.2** `classify` separado de las anclas; `net_changes` (plana) lo usa. Las pruebas de hoy siguen igual.
- [ ] **T24.3** `reconcile` (D22):
  - gemelas por clave, el resto por nombre y transformación;
  - las anclas por gemelas y por los puertos de una hija que cambió;
  - la memoria de comparaciones `(a, b)` en `Memo` y en disco;
  - `cell_net_changes_hier`.

  Prueba: el mismo resultado con y sin la memoria de comparaciones.
- [ ] **T24.4** `gds_diff::net_changes` y `device_changes` con la extracción jerárquica: la celda cambiada y sus ancestros; sin ventanas ni confirmación; la regla de la celda más baja.
- [ ] **T24.5** **`RIKU_FLAT_NETS=1`** vuelve al camino de hoy (NF5). Con las dos maneras, comparar `riku show` de cada commit de los demos `inversor` y `sram`: iguales, salvo los avisos que desaparecen.
- [ ] **T24.6** El visor de un diff (la celda abierta) con `cell_net_changes_hier`.
- [ ] **T24.7** `python3 tools/demos/inversor.py` en el contenedor: `check_show` y `check_lvs` en verde, y el bundle sale idéntico.

## T25. Visor y LVS (R23) · commit `feat(layout):` y `feat(lvs):`

- [ ] **T25.1** **Visor:** `add_electrical` con `hier::extract` y `flatten` hasta el tope; por encima, el resumen con `stats` y el aviso nuevo.
  - **Prueba:** abrir la misma celda dos veces extrae una sola vez (un contador en `stats` o en `Memo`).
- [ ] **T25.2** **`hier::spice`** (D23):
  - los `.subckt` por celda con transistores;
  - las celdas que solo llevan cables, metidas en su padre;
  - los pines juntando todos los usos.

  **`layout_spice`** elige plana o jerárquica por `flat_size`.
- [ ] **T25.3** **`parse_netgen`** lee todas las entradas del `comp.json`: el veredicto de la última y las discrepancias de todas con su camino.
  - **Pruebas:** un `comp.json` de dos niveles; las de hoy siguen igual.
- [ ] **T25.4** **R23.4:**
  - `riku lvs` y `log --lvs -f json` de `ota` e `inversor`, iguales a T20.2;
  - `chip_vs_magic.sh` con la SPICE jerárquica: "coinciden".
- [ ] **T25.5** **A mano en el visor** (Xvfb y capturas):
  - la SRAM de `examples/`: la capa Transistores, el tooltip de una red y el clic que la resalta;
  - el demo `inversor`: la vista de LVS con el abierto marcado;
  - la macro del demo `chip`: el resumen con transistores y redes.

## T26. Medidas y documentación (R24.3) · commit `docs:`

- [ ] **T26.1** Repetir T20.1 con el binario nuevo, en los casos de R24.3: antes, después y la meta. Si una meta no se alcanzó, anotar cuánto se llegó y por qué.
- [ ] **T26.2** **`docs/formatos.md`**, «Transistores y redes»:
  - cómo se extrae por celda;
  - el contexto y el sustrato;
  - lo que ya no tiene tope;
  - los avisos que se fueron.
- [ ] **T26.3** **`docs/desarrollo.md`:**
  - Rendimiento, con la memoria por huella y los números nuevos;
  - Verificación, con `hier.sh` y `chip_vs_magic.sh`;
  - `RIKU_NETS_MEM_MB` y `RIKU_FLAT_NETS`.
- [ ] **T26.4** `docs/lvs.md`: el LVS jerárquico por encima del tope y cómo se muestran las discrepancias de una sub-celda.
- [ ] **T26.5** El README del demo `chip`: los tiempos nuevos. Regenerar el bundle solo si cambia el README.
- [ ] **T26.6** **`docs/pendientes.md`:**
  - **se quitan:** la extracción jerárquica, el costo del `log` con cambios en la celda de arriba y el primer `show` del bitcell;
  - **se agregan:** la sonda jerárquica del visor, ubicar las discrepancias de un LVS jerárquico y borrar el camino plano (NF5).

## Cierre

- [ ] **C1** `cargo fmt --check`, `cargo test --workspace --locked` (`-D warnings`) y las combinaciones de features de la CI en verde.
- [ ] **C2** `tools/verify/nets/hier.sh` y `chip_vs_magic.sh` en verde en el contenedor.
- [ ] **C3** Decidir con el usuario si el camino plano (`RIKU_FLAT_NETS`, las ventanas de `nets::context`) se borra ya o en la ronda 6.
- [ ] **C4** Merge a `main` cuando lo pidas.
