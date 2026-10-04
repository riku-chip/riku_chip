# LVS en Riku (diseño)

Propuesta para comparar el layout contra el esquemático (LVS) en cada versión y rastrear las diferencias en los dos dibujos a la vez. Es un diseño para acordar entre las dos mitades del proyecto (esquemáticos y layouts). `riku lvs` (Fase 1 y el historial con `--log`) y la vista de LVS (Fase 3, ver abajo) ya existen; el uso está en [`cli.md`](cli.md#riku-lvs-el-layout-contra-el-esquemático).

## Por qué

Un LVS que solo dice "no coincide" cuesta arreglarlo. Hacen falta dos cosas que hoy no tiene ninguna herramienta abierta junta:

- **Dónde:** hacer clic en una red o un transistor que no calza y verlo resaltado en el esquemático y en el layout a la vez (*cross-probing*).
- **Desde cuándo:** qué commit rompió el LVS, y qué cambió en ese commit en cada lado. Eso es propio de Riku.

## Qué ya hay (probado el 2026-09-29 con `riku demo ota`)

| Pieza | Dónde | Estado |
|---|---|---|
| Netlist SPICE del layout (transistores con W y L, redes con nombre, pines) | `riku-mod-layout`: `nets::cell_nets` + `nets::spice`; ejemplo `examples/nets.rs` | hecha; verificada con Netgen contra las netlists de los PDK y contra Magic (`tools/verify/nets/`) |
| Netlist del esquemático | `xschem-viewer-rust`: `spice::netlist` en modo LVS (como `xschem --netlist` con `lvs_netlist` y `top_subckt`), sin Xschem; ejemplo `examples/spice.rs` | hecha; verificada con Netgen contra la de Xschem en los ejemplos de los tres PDK (`tools/verify/netlist/`) |
| Comparación | `netgen -batch lvs … <pdk>_setup.tcl out -json` | funciona: da `comp.out` y `comp.json` (`badnets`, `badelements`, `properties`, `pins`) |
| Resaltar en el layout | `NetProbe` / `net_at`, clic en un polígono | hecho |
| Resaltar en el esquemático | `component_bbox`, wires con su `lab`, atenuado del diff | hecho |
| Jerarquía | celdas del layout; sub-esquemáticos con doble clic y Volver | hecho |

Con el demo `ota` en `HEAD`, la cadena completa da **"Circuits match uniquely. Property errors were found."**: la conectividad coincide, pero el esquemático tiene M1/M2 con W = 4 µm y M3/M4 con 18 µm, y el layout quedó con 2 µm y 19 µm. Es el caso que Riku debería mostrar solo: el commit `a603147` cambió el esquemático y el layout no lo siguió.

## Fase 1: `riku lvs` en la CLI

```text
riku lvs [REV] [--sch x.sch] [--layout x.gds|x.mag] [--cell C] [-f text|json] [--ci]
```

- **Qué se compara con qué:** por defecto, el esquemático y la celda de layout con el mismo nombre (`ota-5t.sch` ↔ celda `ota-5t` de `ota-5t.gds`/`.mag`). Si no, `.riku.toml`:
  ```toml
  [[lvs]]
  schematic = "xschem/ota-5t.sch"
  layout = "layout/ota-5t.gds"
  cell = "ota-5t"
  ```
- **Netlists:** las dos las escribe Riku, sin herramientas externas: la del esquemático con `spice::netlist` (con los símbolos del PDK y los archivos del proyecto de esa versión, del disco o de un commit), la del layout con `nets::spice`.
- **Comparación:** Netgen con el `setup.tcl` del PDK (`$PDK_ROOT/<pdk>/libs.tech/netgen`). Riku lee el `comp.json`.
- **Salida:** `riku-lvs/v1` en JSON: resultado (`match`, `property_errors`, `mismatch`), pines, y por cada discrepancia qué hay de cada lado (nombre en el esquemático, nombre en el layout, parámetros). Texto legible por defecto. `--ci`: sale con 1 si no coincide.
- **Requisitos:** `netgen` instalado (está en iic-osic-tools); Xschem no hace falta. `riku doctor` dice si falta. Sin él, `riku lvs` lo dice y no hace nada más; el resto de Riku no lo necesita.

### La netlist del esquemático, sin Xschem

`spice::netlist` escribe lo mismo que Xschem en modo LVS. Verificado con `tools/verify/netlist/compare_xschem.sh`: Netgen da el mismo veredicto con la nuestra que con la de Xschem en todos los esquemáticos de ejemplo de GF180 (59) e IHP (59 que Netgen puede comparar) y en 59 de 60 de SKY130. Lo que cubre:

- Conectividad por geometría (extremos, wires que se tocan, pines sobre wires), nombres de las etiquetas, `#net` viejos de Xschem sin unir nets, `.GLOBAL` de `vdd`/`gnd`.
- `lvs_format`/`format` del símbolo o de la instancia; `@name`, `@pinlist`, `@@PIN`, `@symname`, atributos con el `template`; `clave=@x` vacía y `m=1` no se escriben; `tcleval(…)` con las variables del `xschemrc` del PDK (`$::SKYWATER_MODELS`, …), con `PDK_ROOT` y `PDK` de `sak-pdk`.
- Sub-circuitos: `.subckt` con sus pines (y los de `extra` que son nets) y parámetros; `@x` de adentro con el template del símbolo; variantes `schematic=` de una instancia; `device_model`.
- Buses (`A[3:0]`, `a,b,c`) e instancias vector (`x[3:0]`); pines repetidos; símbolos viejos con `G {…}`.
- Bloques de código (`place=header` antes del `.subckt`) y el `S {…}` del esquemático.

No cubre: bloques de código que son programas en Tcl (bucles, `xschem` …); no hay intérprete de Tcl.

## Fase 2: el LVS en el tiempo

- `riku log --lvs` y `riku status --lvs` (hecho en la ronda 3): el resultado por commit respecto de su primer padre, el commit donde pasó de coincidir a no coincidir y qué discrepancias aparecieron, se arreglaron o cambiaron.
- Caché por dependencias (hecho): se reusa un resultado solo si no cambió nada de lo que leyó la corrida ni el entorno (ver Decisiones, 4). Un commit que no tocó nada de eso no recalcula.
- `status --lvs` (hecho): sale con 1 si dejó de coincidir o empeoró; si ya no coincidía y aparecen discrepancias nuevas, sale con 0 y avisa. Falta lo mismo para un PR (base contra head) en la CI.

## Fase 3: el visor de LVS

- **Esquemático y layout lado a lado**, con la lista de discrepancias en el panel de la derecha (redes sin pareja, dispositivos de más o de menos, parámetros distintos).
- **Cross-probing:** un clic en una discrepancia la encuadra y la resalta en los dos lados, con el resto atenuado (lo mismo que el diff). Un clic en una red o un transistor de un lado resalta su pareja en el otro, aunque coincida.
- **Jerarquía:** entrar a un sub-esquemático o a una sub-celda en un lado lleva al par del otro lado, si existe.
- **En un diff:** abrir el LVS de las dos versiones y ver qué discrepancias aparecieron o se arreglaron.

### Qué ya hay (rama `lvs-visor`)

- Botón **LVS** en la barra de arriba, con un esquemático o un layout abierto que tenga par (`lvs::pair_for`: `.riku.toml` o el mismo nombre). Abre `riku/src/gui/lvs_view.rs`: los dos lienzos lado a lado, el veredicto y la lista (parámetros, redes y dispositivos sin pareja). Netgen corre en otro hilo y cada escena carga aparte (`Loader::load_detached`).
- **Lado del esquemático, completo:** un clic en algo de la lista lo resalta (redes: sus wires y pines; dispositivos: el recuadro de la instancia), atenúa el resto y lo encuadra con contexto. Esc lo suelta. Los nombres de Netgen llevan a la geometría con `Report::places` (`spice::Places`), que escribe el mismo netlister que vio Netgen: el mapeo es exacto, también para las nets sin nombre (`net3`).
- **Lado del layout, completo** (ronda 2, [`ronda-2/`](ronda-2/plan.md)): la sonda de la escena (`LayoutNets`) implementa `net_named` y `device_named` con los nombres de la netlist que compara Netgen (`nets::spice`): las redes por `Netlist::net_name` (la etiqueta, `VSUBS` o `n<i>`; dos redes con la misma etiqueta son una) y los dispositivos por su índice en `nl.devices` (`19`, `X19` o `M19`; `R3`/`XR3` para un resistor). Un dispositivo trae las compuertas de **todos los que están en paralelo con él** (`nets::parallel_groups`): Netgen los junta aunque cambie L y nombra al conjunto por el de índice menor. El visor extrae las redes con la misma información de Magic que `riku lvs`. Si algo no se encuentra (celda demasiado grande para calcular redes, layout sin PDK conocido), la lista lo avisa.
- **Verificar a mano:** `cargo run -p riku-mod-layout --example lvs_probe -- layout.gds celda 19 20 Vout` dice cuántas compuertas o pedazos ubica cada nombre de Netgen y el W de cada grupo, para compararlo con `riku lvs -f json`.

## Lo difícil

- **Nombres.** Netgen empareja por conectividad y da los nombres de cada lado (`sky130_fd_pr__pfet_01v8:19 vs. …:M1`). Para resaltar hay que ir de ese nombre a la geometría: en el esquemático, el nombre de la instancia (`M1`) ya lleva a su recuadro; en el layout, `nets::spice` numera los transistores (`X19`) y hace falta guardar la posición de cada uno junto a su número.
- **Fingers y multiplicidad.** El esquemático dice `W=18 nf=4`; el layout tiene cuatro transistores de 4,5 µm. Netgen (con el `setup.tcl` de SKY130) combina en paralelo los del mismo modelo y terminales **aunque cambie L**, y no solo los fingers iguales: en el demo `ota` junta los rellenos de L = 0,5 µm y de L = 1 µm de una rama en un dispositivo (`0`) e informa el W de cada L. Por eso `nets::fingers` (que agrupa también por L) da 9 grupos y Netgen 8; la vista usa `nets::parallel_groups`, que da los mismos 8.
- **Jerarquía distinta en cada lado.** El esquemático va por niveles. El layout se extrae por celdas (ronda 5) y se escribe **plano**, aplanando esa extracción, con los mismos nombres que la sonda del visor. Ya no hay tope de polígonos: la macro de 1 KB entera (127 628 transistores) sale en segundos, pero Netgen tarda unos 20 min en compararla plana. Escribir la SPICE por niveles (un `.subckt` por celda) está en `pendientes.md`.
- **Dispositivos de relleno.** Los *dummies* del layout (transistores con compuerta a una fuente) aparecen como dispositivos de más si el esquemático no los tiene. Netgen tiene reglas para ignorarlos por PDK; hay que ver cuáles aplican.
- **Netgen como dependencia.** Es el estándar de SKY130, GF180 e IHP y ya lo usamos para verificar. Un comparador propio (grafo + emparejamiento por firma) evitaría la dependencia, pero es otro proyecto: primero Netgen.

## Reparto propuesto

| Parte | Quién |
|---|---|
| Netlist del esquemático (`spice::netlist` con el PDK y los archivos de una versión) | esquemáticos (Carlos) |
| Netlist del layout con la posición de cada transistor y red por su nombre SPICE | layouts (Adriel) |
| `riku lvs` (emparejar archivos, correr Netgen, leer `comp.json`, JSON y `--ci`) | cualquiera; toca el núcleo |
| Historial del LVS y caché | núcleo |
| Visor de LVS (dos paneles, lista, cross-probing) | esquemáticos (Carlos), con la API de posiciones del layout |

## Decisiones (ronda 3, 2026-10-04)

1. **Netgen, por ahora.** Es el comparador de SKY130, GF180 e IHP, y sus reglas por PDK (paralelos aunque cambie L, propiedades, *dummies*) ya hicieron falta en la vista de LVS. Un comparador propio (grafo + emparejamiento por firma) es otro proyecto. Se reabre, y se conversa con Carlos, si aparece un caso concreto: Netgen no se puede instalar donde se usa Riku, o hace falta un mensaje que Netgen no da.
2. **Emparejamiento por nombre y en `.riku.toml`, con prioridad:** `--sch/--layout` > `[[lvs]]` en `.riku.toml` > mismo nombre (`.gds` > `.oas` > `.mag`). El caso común no pide configuración; cuando el nombre elige entre varios layouts, Riku lo avisa y sugiere fijarlo.
3. **En `log` y `status`, solo con `--lvs`.** Una corrida nueva son segundos por par y versión; el `log` de siempre no debe esperar a Netgen ni fallar si no está. Con `--lvs`, el código de salida de `status` pasa a ser el del LVS (1 si empeoró). Un `[lvs] in_status = true` en `.riku.toml` queda pendiente.
4. **No se versiona el resultado: se recalcula, con caché.** Es un derivado que depende del PDK y de Netgen; commitearlo dejaría resultados viejos y conflictos en cada merge. La caché (`~/.cache/riku/lvs/v2`) guarda, por resultado, todo lo que leyó la corrida (archivos del proyecto con su id de Git, también los que buscó y no encontró) y una huella del entorno (Riku, netlister, PDK, `setup.tcl`, `xschemrc`, Netgen): se reusa solo si nada de eso cambió, para un commit o para el working tree.
