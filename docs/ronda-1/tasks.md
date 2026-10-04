# Ronda 1: tareas

Marcar `[x]` al terminar. Cada tarea es un commit (salvo que se diga otra cosa). Requisitos en [`requirements.md`](requirements.md), diseño en [`design.md`](design.md). Compilar y probar en el contenedor (`docs/desarrollo.md`); editar y commitear en `Documents`.

Antes de empezar: `git pull`, rama `ronda-1` desde `main`, y `cargo test --workspace --locked` en verde como línea de base (anotar cuántas pruebas pasan).

## T1. `cargo fmt` y Clippy en la CI (R1) · commit `style:` + commit `ci:`

- [ ] **T1.1** Probar las candidatas de `rustfmt.toml` (`design.md` D1) y elegir la que deje menos `Diff in`. Anotar los conteos en el mensaje del commit.
- [ ] **T1.2** Crear `rustfmt.toml` en la raíz y correr `cargo fmt -p viewer-core -p riku-kernel -p riku-mod-layout -p riku`. Revisar `git diff --stat`: nada dentro de `external/`.
- [ ] **T1.3** `cargo test --workspace --locked` (con `RUSTFLAGS=-D warnings`): mismo resultado que la línea de base.
- [ ] **T1.4** Commit **solo de formato**: `style: cargo fmt con rustfmt.toml` (el `rustfmt.toml` va aquí).
- [ ] **T1.5** Crear `.git-blame-ignore-revs` con el hash de T1.4 y commitear (`chore: ignorar el commit de formato en git blame`).
- [ ] **T1.6** `ci.yml`: job `fmt` (bloqueante) y job `clippy` (`continue-on-error: true`, sin `-D warnings`), según D1. Commit `ci: cargo fmt --check y Clippy sin bloquear`.
- [ ] **T1.7** `docs/desarrollo.md`: cómo formatear, cómo correr Clippy y cómo activar `blame.ignoreRevsFile`.
- [ ] **T1.8** Empujar la rama y revisar la CI: `fmt` verde; `clippy` con avisos en el log pero el job sin romper el resultado. Contar los avisos y anotarlos en `docs/pendientes.md` (para la ronda que lo vuelva bloqueante).

**Listo cuando:** la CI de la rama está verde y `cargo fmt --check` no da diferencias.

## T2. `upload-artifact` en Node 24 (R2) · commit `ci:`

- [ ] **T2.1** Buscar la versión de `actions/upload-artifact` con `runs.using: node24` (`design.md` D2).
- [ ] **T2.2** Subirla en `release.yml`; confirmar en las notas de la versión que `name`/`path` siguen igual.
- [ ] **T2.3** Revisar `checkout`, `rust-cache` y `rust-toolchain` en `ci.yml` y `release.yml`; subir los que sigan en Node 20.
- [ ] **T2.4** Correr el Release con `workflow_dispatch` en la rama. Comprobar: artefacto `riku-<versión>-linux-x86_64` con `tar.gz`, `deb` y `SHA256SUMS`; sin el aviso de Node 20; no se creó ningún release (no hay tag).

**Listo cuando:** el Release de la rama termina en verde sin avisos de Node 20.

## T3. `riku render` respeta `hidden` (R3) · commit `fix(render):`

- [ ] **T3.1** Escribir primero la prueba en `riku/src/export/svg.rs`: capa 7 visible, capa 8 con `hidden: true`; el SVG no contiene nada de la 8. Ver que **falla**.
- [ ] **T3.2** En `scene_svg`, armar `HashSet<Layer>` con `scene.layer_list()` y saltar esos elementos en el `visit` (D3).
- [ ] **T3.3** Ver que la prueba pasa y que el resto de `svg.rs` sigue igual.
- [ ] **T3.4** A mano: `riku demo ota` y `riku render` de un layout con transistores reconocidos, SVG y PNG. Sin amarillo ni etiquetas "W/L"; mismo encuadre que antes.
- [ ] **T3.5** Quitar de `docs/pendientes.md` la fila de `riku render` y "Transistores" (en "En curso: demos y README").

**Listo cuando:** la prueba nueva pasa y la imagen del demo no muestra la capa "Transistores".

## T4. `log --graph --color` (R4) · commit `feat(cli):`

- [ ] **T4.1** Pruebas primero: `resolve` (tabla modo × tty × `CLICOLOR_FORCE` × `NO_COLOR`) en `color.rs`. Ver que fallan.
- [ ] **T4.2** `ColorMode` + `resolve` + `set_mode` en `riku/src/cli/format/color.rs`; `enabled()` usa `resolve`.
- [ ] **T4.3** `log_graph::Style::detect(ascii, mode)` usa `resolve` (se borra la fórmula duplicada).
- [ ] **T4.4** Opción `--color` en `cli/mod.rs` (`Option<ColorMode>`, `requires = "graph"`), cableada en `dispatch.rs` (dos sitios) y `LogArgs`/`run_log` en `commands.rs`.
- [ ] **T4.5** `help.color` en las tablas de `i18n` (los dos idiomas).
- [ ] **T4.6** Prueba de `render`: sin color no hay `\x1b[`; con color sí.
- [ ] **T4.7** `docs/cli.md`: línea de uso y prioridad (opción > `CLICOLOR_FORCE`/`NO_COLOR` > terminal).
- [ ] **T4.8** A mano: `riku log --graph --color always | cat` (con códigos), `--color never` en terminal (sin códigos), `--color always -f json` (JSON limpio), `--color always` sin `--graph` (error de clap). Revisar que las completaciones de la shell traen `--color`.

**Listo cuando:** las pruebas pasan y las cuatro comprobaciones de T4.8 dan lo esperado.

## T5. Historial: Tab a la lista de archivos (R5) · commit `feat(gui):`

- [ ] **T5.0** **Prueba corta del riesgo de Tab** (`design.md` D5): con el Historial abierto, ver qué hace egui con Tab hoy (¿mueve el foco entre los botones de archivo?) y probar las opciones 1 y 2 de ese apartado. Elegir y anotar cuál funcionó. Si ninguna, parar y preguntar por otra tecla.
- [ ] **T5.1** Pruebas del modelo primero (`history/model.rs`): `toggle_focus` sin detalles o sin archivos abribles no hace nada; `move_file` salta no abribles y se detiene en los extremos; cambiar de commit vuelve a `Commits`. Ver que fallan.
- [ ] **T5.2** `Focus`, `file_selected`, `toggle_focus`, `move_file`, `open_marked` en `HistoryModel`.
- [ ] **T5.3** Teclas en `history/mod.rs::show`: tabla de D5 (Tab/Shift+Tab, ↑/↓, Enter según el foco), con la solución de T5.0.
- [ ] **T5.4** `file_row` marca el archivo elegido y hace `scroll_to_me` al cambiar.
- [ ] **T5.5** `docs/gui.md` (línea del Historial): Tab, ↑/↓ y Enter en los archivos.
- [ ] **T5.6** A mano en Xvfb: abrir el Historial (**H**), elegir un commit, **Tab**, **↓** hasta un archivo, **Enter** (se abre su diff), **Tab** de vuelta; un commit sin archivos abribles; con el Historial cerrado, Tab no hace nada; con el campo "Filtrar archivos" con foco, Tab sigue siendo de ese campo.

**Listo cuando:** las pruebas del modelo pasan y el recorrido de T5.6 funciona solo con el teclado.

## Cierre de la ronda

- [ ] **C1** `cargo test --workspace --locked` y `cargo fmt --check` en verde; CI de la rama en verde.
- [ ] **C2** Actualizar `docs/pendientes.md`: quitar las filas hechas (render, `upload-artifact`, `cargo fmt` y Clippy, `--color`, Tab); dejar la de "tope de ramas con `…`" y agregar la de "Clippy bloqueante" con el conteo de T1.8.
- [ ] **C3** Revisar el orden de commits: `style` va primero y solo con formato. PR hacia `main`.
