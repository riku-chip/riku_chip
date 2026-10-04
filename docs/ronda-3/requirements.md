# Ronda 3: requisitos

Numerados como las tareas de [`plan.md`](plan.md) (9–15). Cada uno con un criterio comprobable. "Par" es un esquemático con su layout (`lvs::Pair`).

## R9. Discrepancias con identidad y su delta

**Historia.** Como diseñador, quiero saber qué discrepancia apareció, cuál se arregló y cuál cambió de valor en cada versión, no solo si el veredicto empeoró.

- **R9.1** Cada discrepancia de un resultado tiene una **clave estable entre versiones**, armada con los nombres del **esquemático** (los del layout son índices que se corren cuando se agrega un transistor):
  - parámetro: `(instancia del esquemático, parámetro)` → `M3 · w`;
  - red sin pareja: los nombres del esquemático del grupo, ordenados (o los del layout si el esquemático no tiene ninguno);
  - dispositivo sin pareja: igual que las redes;
  - pines: cada pin que está de un solo lado.
- **R9.2** Entre dos resultados del mismo par: **apareció** (está ahora, no antes), **se arregló** (antes sí, ahora no) y **cambió** (misma clave, otros valores: `w 18 ≠ 20` → `w 18 ≠ 19`).
- **R9.3** Un parámetro numérico muestra también el error del layout respecto del esquemático, que es la intención: `(layout − esquemático) / esquemático`. `M1 w 4 ≠ 2` → `−50 %` (al layout le falta la mitad del ancho); `M3 w 18 ≠ 19` → `+5,6 %`. Uno no numérico, solo los dos valores.
- **R9.4** El delta no depende del orden en que Netgen lista las cosas.

## R10. Caché por dependencias

**Historia.** Como diseñador, quiero que un resultado guardado se reuse solo si de verdad nada de lo que lo determina cambió.

- **R10.1** Cada corrida registra **todos los archivos del proyecto que leyó** (el esquemático, sus sub-esquemáticos y símbolos del proyecto, el layout y sus sub-celdas) con el id de contenido de Git de cada uno, y **los que buscó y no encontró** (un símbolo que falta y se agrega después cambia el resultado).
- **R10.2** Y una **huella del entorno**: nombre y carpeta del PDK, contenido de su `setup.tcl` de Netgen y de su `xschemrc`, la versión del PDK si la declara (`.config/nodeinfo.json` de open_pdks), el ejecutable de Netgen (ruta, tamaño, fecha), la versión de Riku y `spice::VERSION`.
- **R10.3** Un resultado guardado se reusa para una versión (commit o working tree) **solo si** cada archivo registrado tiene el mismo contenido en esa versión, los no encontrados siguen sin estar y la huella del entorno es la misma.
- **R10.4** Funciona igual para un commit (ids del árbol de Git, sin escribir archivos) y para el working tree (id calculado del archivo en disco).
- **R10.5** Comprobar una entrada de la caché cuesta milisegundos (sin Netgen ni netlists).
- **R10.6** `RIKU_NO_CACHE` y `RIKU_CACHE_DIR` siguen valiendo. Las entradas de la caché vieja (por carpetas) no se usan.
- **R10.7** Cambiar un símbolo del proyecto que está en otra carpeta que el esquemático invalida el resultado (hoy no).

## R11. `riku log --lvs`

**Historia.** Como diseñador, quiero ver en el `log` de siempre en qué commit cambió el LVS y por qué.

- **R11.1** `riku log --lvs` agrega a cada commit, por cada par que existe en ese commit: el veredicto, la transición respecto de su **primer padre** (`dejó de coincidir`, `empeoró`, `mejoró`, `volvió a coincidir`) y el delta de R9.
- **R11.2** Funciona con `--graph` (las ramas también) y con `-n`, `--branch`, `--paths` y el archivo posicional. Con un archivo o `--paths`, solo los pares que lo incluyen.
- **R11.3** Cantidad de detalle según el nivel que ya tiene `log`: sin flags, el veredicto, la transición y hasta 3 líneas de delta (`… y N más`); `--detail`, todo el delta; `--full`, también la lista completa de discrepancias de ese commit.
- **R11.4** Un commit cuyo par no cambió respecto de su padre no repite el delta (no hay); muestra el veredicto en una línea corta, o nada si tampoco cambió el veredicto (`--detail` lo muestra igual).
- **R11.5** `-f json`: cada commit lleva un campo opcional `lvs` (lista por par: par, veredicto o estado `missing`/`error`, transición, delta). El schema sigue `riku-log/v2` (campo nuevo opcional).
- **R11.6** Sin Netgen, o con un error en un commit, el `log` sale igual: un aviso arriba y `LVS: error (…)` en ese commit. Nunca falla el comando por el LVS.
- **R11.7** Solo con `--lvs`: sin la opción, `log` no cambia en nada (ni tiempo ni salida).

## R12. `riku status --lvs`

**Historia.** Como diseñador, antes de commitear quiero saber si mis cambios rompen el LVS, y poder ponerlo en un hook o en la CI.

- **R12.1** `riku status --lvs` compara, por par, el working tree contra `HEAD`: veredicto de cada lado, transición y delta.
- **R12.2** Los pares que no tocaron los cambios se muestran en una línea (`ota-5t: coincide, sin cambios`), usando el resultado de `HEAD`.
- **R12.3** Un par nuevo (no está en `HEAD`) muestra solo el resultado del working tree.
- **R12.4** Con `--lvs`, **el código de salida lo decide el LVS**: 0 si ningún par empeoró (igual o mejor), 1 si alguno empeoró (de "coincide" a otra cosa, o de "parámetros" a "no coincide"), 2 si hubo un error. Si un par sigue igual de mal pero **aparecieron discrepancias nuevas**, sale 0 con un aviso (la política de `lvs.md`: bloquear lo que se rompe, avisar lo que ya estaba roto). Sin `--lvs`, `status` sigue como hoy.
- **R12.5** `-f json`: campo opcional `lvs` en `riku-status/v2`, con `head`, `worktree`, `transition` y `delta` por par.

## R13. `riku lvs --log` con el delta

- **R13.1** Cada commit con transición o con delta muestra sus líneas de delta (mismo formato que R11.3).
- **R13.2** `riku-lvs-log/v1` agrega el campo opcional `delta` por commit.
- **R13.3** Usa la caché nueva de R10 (y por lo tanto deja de reusar resultados cuando cambió un archivo fuera de las carpetas del par).

## R14. Decisiones escritas

- **R14.1** La sección "Decisiones abiertas" de `lvs.md` pasa a "Decisiones", con las cuatro respuestas del plan y su porqué; la 1 dice qué haría reabrirla y que se conversa con Carlos.
- **R14.2** `riku lvs` avisa cuando el emparejamiento por nombre eligió entre varios layouts del mismo nombre (`amp.gds` y `amp.mag`).

## R15. Validado

- **R15.1** En el demo `ota`, `riku log --lvs --graph` cuenta la historia: `5338fc4` coincide; `a603147` deja de coincidir con `+ M1 w 4 ≠ 2`, `+ M2 w 4 ≠ 2`; la rama `narrow-input-pair` agrega `M3`/`M4` y el merge los trae a `main`; `120ee0b` empeora con `+ red Vout, Vp`; `6184836` mejora con `− red Vout, Vp`. (Los valores exactos se anotan al correrlo; esto es lo esperado por los mensajes de los commits.)
- **R15.2** `riku status --lvs` en el demo: sin cambios → 0; con un cambio de W en el esquemático que agrega una discrepancia → 0 y aviso (ya había parámetros distintos); con el corto de `120ee0b` aplicado al disco → 1.
- **R15.3** Un símbolo del proyecto movido a otra carpeta y modificado invalida la caché (antes no); sin cambios, la segunda corrida de `log --lvs` no ejecuta Netgen.
- **R15.4** Pruebas automáticas de R9 (claves y delta), R10 (validar una entrada contra un árbol de Git armado en la prueba, sin Netgen) y de las transiciones y códigos de salida de R12.

## No funcionales

- **NF1** Sin cambios en `xschem-viewer-rust` ni en `riku-kernel`.
- **NF2** Sin `--lvs`, `log` y `status` no cambian (salida, JSON y tiempo).
- **NF3** El LVS es opcional en la compilación como hoy (`xschem` + `layout`): sin esas features, `--lvs` no existe.
- **NF4** Sin dependencias nuevas.
