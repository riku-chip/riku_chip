# Ronda 3: tareas

Marcar `[x]` al terminar. Requisitos en [`requirements.md`](requirements.md), diseño en [`design.md`](design.md). Compilar y probar en el contenedor (`docs/desarrollo.md`); editar y commitear en `Documents`. Antes: `git pull`, rama `ronda-3` desde `main`, `cargo test --workspace --locked` en verde como base (444 pasan, 5 ignoradas al cerrar la ronda 2). Sin tocar `external/xschem-viewer-rust`.

## T9. Discrepancias y delta (R9) · commit `feat(lvs):`

- [x] **T9.1** `core/analysis/lvs_types.rs`: `Verdict`, `Transition` (movidos, reexportados desde `lvs.rs`), `Discrepancy`, `Delta`, `LvsState`, `PairLvs`. `cargo build -p riku --no-default-features` sigue compilando.
- [x] **T9.2** Pruebas primero: `Discrepancy::key()` (parámetro, redes con y sin nombres del esquemático, pin); `delta()` con apareció / se arregló / cambió; mismo resultado con las listas en otro orden; error relativo (`4`/`2` → −50 %, `18`/`19` → +5,6 %, no numérico sin %).
- [x] **T9.3** `Comparison::discrepancies()`, `delta()`, `transition(antes, ahora)` (la regla de `mark_transitions`, de a dos veredictos).

## T10. Caché por dependencias (R10) · commit `feat(lvs):`

- [x] **T10.1** Pruebas primero, sin Netgen: un repo de Git armado en la prueba con un esquemático, un símbolo en otra carpeta y un layout; una entrada guardada con sus deps vale para ese commit, no vale si cambia el símbolo, no vale si aparece un archivo que antes faltaba, no vale con otra huella; lo mismo contra el working tree.
- [x] **T10.2** `RecordingFiles` y el registro del esquemático y el layout en `run`.
- [x] **T10.3** `EnvPrint` (D10), con memo por PDK.
- [x] **T10.4** `Version` (commit y disco), `lookup`, `store` en `lvs/v2/`, tope de 200 entradas por par.
- [x] **T10.5** `result_at`; `history` lo usa; se borran `signature` y la caché por carpetas.
- [x] **T10.6** A mano en el demo: `riku lvs --log` dos veces; la segunda no corre Netgen (medir el tiempo de las dos).

## T11. `riku log --lvs` (R11) · commit `feat(cli):`

- [x] **T11.1** `LogCommit::lvs` (opcional, vacío por defecto) y su inicialización donde se arman los `LogCommit`.
- [x] **T11.2** `lvs::annotate_log` (D11): pares filtrados por archivo/`--paths`, resultado del primer padre, transición, delta, aviso sin Netgen.
- [x] **T11.3** `--lvs` en la CLI (solo con `xschem`+`layout`), `help.log_lvs` en `es.yml`/`en.yml`, completaciones.
- [x] **T11.4** Texto en `log_text` y `log_graph` con los niveles de R11.3; JSON por serde.
- [x] **T11.5** Sin `--lvs`: misma salida que antes (comparar `riku log` y `riku log -f json` del demo antes y después, byte a byte).

## T12. `riku status --lvs` (R12) · commit `feat(cli):`

- [x] **T12.1** Pruebas primero: tabla transición/aparecidas/error → código de salida (D12).
- [x] **T12.2** `StatusReport::lvs`, `lvs::annotate_status`, el texto al final de `status`, `--lvs` en la CLI y el código de salida.
- [x] **T12.3** `docs/cli.md`: `log --lvs`, `status --lvs` con la tabla de códigos y el hook de pre-commit.

## T13. `riku lvs --log` con el delta (R13) · commit `feat(lvs):`

- [x] **T13.1** `Step::delta` (opcional en `riku-lvs-log/v1`) y sus líneas en el texto.

## T14. Decisiones (R14) · commit `docs(lvs):`

- [x] **T14.1** `lvs.md`: "Decisiones" con las cuatro respuestas de `plan.md`.
- [x] **T14.2** Aviso de `lvs::pairs` cuando el nombre elige entre varios layouts; prueba.

## T15. Validar (R15)

- [x] **T15.1** Demo `ota`: `riku log --lvs --graph` y `-f json`; anotar en este archivo la historia que sale (qué apareció en cada commit) y compararla con R15.1.
- [x] **T15.2** `riku status --lvs` en el demo: limpio (0); W cambiado en el esquemático (0 y aviso); el layout de `120ee0b` en el disco (1). Restaurar el demo.
- [x] **T15.3** Caché: un símbolo del proyecto en otra carpeta, modificado → se recalcula; sin cambios, la segunda corrida no ejecuta Netgen (`RIKU_LVS_KEEP` o el contador del aviso).
- [x] **T15.4** `RIKU_NO_CACHE=1 riku log --lvs` da lo mismo que con caché.

## Cierre

- [x] **C1** `cargo fmt --check` y `cargo test --workspace --locked` (`-D warnings`) en verde; `cargo build -p riku --no-default-features` y las combinaciones de features de la CI.
- [x] **C2** `docs/pendientes.md`: lo que quede (insignia en el Historial, Netgen en paralelo, `[lvs] in_status`, escribir solo los archivos leídos en `Tree::commit`).
- [ ] **C3** Merge a `main` cuando lo pidas.

## Cómo terminó (2026-10-04)

- **Pruebas:** 457 pasan (444 + 13: claves, delta, corto sin parámetros, error relativo, código de salida de `status`, caché contra un repo de Git armado en la prueba, aviso de emparejamiento, formato). Mutación: si la caché ignora lo que "faltaba", falla la prueba de un archivo que aparece. Compilan las cuatro combinaciones de features de la CI.
- **`riku log --lvs --graph` en el demo `ota`** (la historia de R15.1, confirmada):
  - `5338fc4` coincide; `a603147` deja de coincidir con `+ M1 w: esquemático 4, layout 2 (−50 %)` y `M2` igual.
  - Rama: `0f05efe` `+ M3`/`M4 w 18 vs 20 (+11 %)`; `02496a4` `~ M3`/`M4 layout 20 → 19 (+5,6 %)` y `+ M8 w 20 vs 19 (−5,0 %)`. El merge `ad104a7` trae `M3`, `M4`, `M8` a `main`.
  - `120ee0b` empeora: `+ redes sin pareja: Vout, Vp (layout: Vout)`, `+ pin Vp solo en el esquemático`. `6184836` los arregla (`−`).
- **Dos errores que encontró el demo y se corrigieron:** con un corto, Netgen no compara parámetros, así que el commit del corto "arreglaba" todos los W y el siguiente los hacía "aparecer" (ahora, si en un lado no coinciden las conexiones, los parámetros no entran en el delta); y `(no matching pin)` de Netgen salía como pin.
- **`riku status --lvs`:** limpio → 0 (`sin cambios`); W de M6 2 → 3 en el esquemático → 0 con aviso y `+ M6 w: esquemático 3, layout 2 (−33 %)`; el layout de `120ee0b` en el disco → 1 (`← empeoró`).
- **Caché:** `riku lvs --log -n 12`, 8 corridas la primera vez y 34 ms sin Netgen la segunda. El commit que solo cambia el testbench reusa el resultado (la caché por carpetas lo recalculaba). Un `sky130_fd_pr/nfet_01v8.sym` en la raíz del proyecto (fuera de `xschem/` y `layout/`) hace correr Netgen otra vez (la caché por carpetas no se enteraba); al borrarlo vuelve a reusar. `RIKU_NO_CACHE=1` da la misma salida (10 corridas, 5,7 s).
