# LVS en Riku (diseño)

Propuesta para comparar el layout contra el esquemático (LVS) en cada versión y rastrear las diferencias en los dos dibujos a la vez. Es un diseño para acordar entre las dos mitades del proyecto (esquemáticos y layouts); nada de esto existe todavía como comando.

## Por qué

Un LVS que solo dice "no coincide" cuesta arreglarlo. Hacen falta dos cosas que hoy no tiene ninguna herramienta abierta junta:

- **Dónde:** hacer clic en una red o un transistor que no calza y verlo resaltado en el esquemático y en el layout a la vez (*cross-probing*).
- **Desde cuándo:** qué commit rompió el LVS, y qué cambió en ese commit en cada lado. Eso es propio de Riku.

## Qué ya hay (probado el 2026-09-29 con `riku demo ota`)

| Pieza | Dónde | Estado |
|---|---|---|
| Netlist SPICE del layout (transistores con W y L, redes con nombre, pines) | `riku-mod-layout`: `nets::cell_nets` + `nets::spice`; ejemplo `examples/nets.rs` | hecha; verificada con Netgen contra las netlists de los PDK y contra Magic (`tools/verify/nets/`) |
| Netlist del esquemático | `xschem --netlist` con el `xschemrc` del PDK del diseño y `set lvs_netlist 1; set top_subckt 1` | funciona sin pantalla en el contenedor; el PDK se elige como ya lo hace el visor (`xschem_pdk.rs`) |
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
- **Netlists:** del esquemático con `xschem --netlist` (del disco o de un commit, en una carpeta temporal con los archivos de esa versión); del layout con `nets::spice`, sin herramientas externas.
- **Comparación:** Netgen con el `setup.tcl` del PDK (`$PDK_ROOT/<pdk>/libs.tech/netgen`). Riku lee el `comp.json`.
- **Salida:** `riku-lvs/v1` en JSON: resultado (`match`, `property_errors`, `mismatch`), pines, y por cada discrepancia qué hay de cada lado (nombre en el esquemático, nombre en el layout, parámetros). Texto legible por defecto. `--ci`: sale con 1 si no coincide.
- **Requisitos:** `xschem` y `netgen` instalados (están en iic-osic-tools). `riku doctor` dice si faltan. Sin ellos, `riku lvs` lo dice y no hace nada más; el resto de Riku no los necesita.

## Fase 2: el LVS en el tiempo

- `riku log --lvs` y `riku status --lvs`: el resultado por commit, y **el commit donde pasó de coincidir a no coincidir**, con los cambios de ese commit en cada archivo.
- Caché por contenido: la clave es el hash de las dos netlists y del `setup.tcl`. Un commit que no tocó ni el esquemático ni el layout no recalcula nada.
- `--ci` en un PR: bloquear un "coincide → no coincide"; un "no coincide → no coincide" es aviso.

## Fase 3: el visor de LVS

- **Esquemático y layout lado a lado**, con la lista de discrepancias en el panel de la derecha (redes sin pareja, dispositivos de más o de menos, parámetros distintos).
- **Cross-probing:** un clic en una discrepancia la encuadra y la resalta en los dos lados, con el resto atenuado (lo mismo que el diff). Un clic en una red o un transistor de un lado resalta su pareja en el otro, aunque coincida.
- **Jerarquía:** entrar a un sub-esquemático o a una sub-celda en un lado lleva al par del otro lado, si existe.
- **En un diff:** abrir el LVS de las dos versiones y ver qué discrepancias aparecieron o se arreglaron.

## Lo difícil

- **Nombres.** Netgen empareja por conectividad y da los nombres de cada lado (`sky130_fd_pr__pfet_01v8:19 vs. …:M1`). Para resaltar hay que ir de ese nombre a la geometría: en el esquemático, el nombre de la instancia (`M1`) ya lleva a su recuadro; en el layout, `nets::spice` numera los transistores (`X19`) y hace falta guardar la posición de cada uno junto a su número.
- **Fingers y multiplicidad.** El esquemático dice `W=18 nf=4`; el layout tiene cuatro transistores de 4,5 µm. `nets::netlist::fingers` ya los agrupa; hay que confirmar que Netgen los combina igual (el `setup.tcl` del PDK lo decide).
- **Jerarquía distinta en cada lado.** El esquemático va por niveles; el layout se extrae aplanado por celda (tope: 2 millones de polígonos). Para un chip entero hace falta la extracción jerárquica que ya está en `pendientes.md`.
- **Dispositivos de relleno.** Los *dummies* del layout (transistores con compuerta a una fuente) aparecen como dispositivos de más si el esquemático no los tiene. Netgen tiene reglas para ignorarlos por PDK; hay que ver cuáles aplican.
- **Netgen como dependencia.** Es el estándar de SKY130, GF180 e IHP y ya lo usamos para verificar. Un comparador propio (grafo + emparejamiento por firma) evitaría la dependencia, pero es otro proyecto: primero Netgen.

## Reparto propuesto

| Parte | Quién |
|---|---|
| Netlist del esquemático (`xschem --netlist` con el PDK y los archivos de una versión) | esquemáticos (Carlos) |
| Netlist del layout con la posición de cada transistor y red por su nombre SPICE | layouts (Adriel) |
| `riku lvs` (emparejar archivos, correr Netgen, leer `comp.json`, JSON y `--ci`) | cualquiera; toca el núcleo |
| Historial del LVS y caché | núcleo |
| Visor de LVS (dos paneles, lista, cross-probing) | esquemáticos (Carlos), con la API de posiciones del layout |

## Decisiones abiertas

1. ¿Netgen para siempre, o un comparador propio más adelante?
2. ¿Dónde vive el emparejamiento esquemático ↔ layout por defecto: por nombre, o siempre en `.riku.toml`?
3. ¿El LVS entra en `status` y `log` por defecto (cuesta tiempo) o solo con `--lvs`?
4. ¿Se versiona el resultado del LVS (como el `.raw` de una simulación) o siempre se recalcula?
