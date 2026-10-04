# Ronda 1: requisitos

Cada requisito tiene un criterio de aceptación comprobable. Los números siguen el orden de [`plan.md`](plan.md).

## R1. Formato y Clippy en la CI

**Historia.** Como quien revisa un PR, quiero que el formato sea uniforme y lo verifique la CI, para que los commits no mezclen cambios de código con cambios de espacios.

- **R1.1** Existe un `rustfmt.toml` en la raíz que describe el estilo que el código ya usa (líneas largas, `if`/`else` cortos en una línea), de modo que el commit de formato cambie lo mínimo.
- **R1.2** Los cuatro crates del workspace (`viewer-core`, `riku-kernel`, `riku-mod-layout`, `riku`) pasan `cargo fmt --check`.
- **R1.3** `external/` no se formatea (es otro proyecto, fuera del workspace).
- **R1.4** La CI corre `cargo fmt --check` y falla si algo no está formateado.
- **R1.5** La CI corre Clippy sobre el workspace **sin bloquear** (`continue-on-error: true`): los avisos se ven en el log, el job no se pone en rojo.
- **R1.6** El commit de formato no cambia comportamiento: `cargo test --workspace --locked` da el mismo resultado antes y después.
- **R1.7** El commit de formato queda en `.git-blame-ignore-revs`.
- **R1.8** `docs/desarrollo.md` dice cómo formatear y cómo correr Clippy en local.

## R2. `actions/upload-artifact` en Node 24

**Historia.** Como mantenedor, quiero que el Release no dependa de Node 20, que GitHub deja de soportar en las acciones.

- **R2.1** `release.yml` usa una versión de `actions/upload-artifact` que corre en Node 24.
- **R2.2** Ninguna otra acción de `ci.yml` ni `release.yml` queda en Node 20 (se revisan `checkout`, `rust-toolchain`, `rust-cache`).
- **R2.3** El artefacto se sigue llamando `riku-<versión>-linux-x86_64` y contiene lo mismo (`*.tar.gz`, `*.deb`, `SHA256SUMS`).
- **R2.4** Una corrida del Release (o `workflow_dispatch`) termina en verde y sin el aviso de "Node.js 20 actions are deprecated".

## R3. `riku render` respeta las capas ocultas

**Historia.** Como quien exporta un layout, quiero que la imagen se vea como el visor al abrir, sin la capa "Transistores" encima.

- **R3.1** En SVG y PNG, los elementos de una capa con `LayerPaint::hidden == true` no se dibujan: ni polígonos ni textos de esa capa (las etiquetas de tipo, W y L de los transistores).
- **R3.2** Una capa sin `LayerPaint` registrado se dibuja como hasta ahora.
- **R3.3** Las capas visibles se dibujan igual que antes (mismos colores, mismo orden, mismo encuadre).
- **R3.4** Las anotaciones del diff y la versión anterior (`ghost`) no cambian.
- **R3.5** Con `riku demo ota`, `riku render` de un layout con transistores reconocidos no muestra el amarillo de la capa "Transistores".
- **R3.6** Hay una prueba automática: una escena con una capa oculta y otra visible produce un SVG solo con la visible.

## R4. `log --graph --color`

**Historia.** Como quien usa `riku log --graph` en un `less -R`, en una CI o en una terminal sin detección correcta, quiero decidir si hay color sin depender de variables de entorno.

- **R4.1** `riku log --graph --color <auto|always|never>`. Sin la opción vale `auto`, que es el comportamiento de hoy (terminal y sin `NO_COLOR`, o `CLICOLOR_FORCE=1`).
- **R4.2** `always` pone color aunque stdout no sea una terminal; `never` no pone aunque lo sea.
- **R4.3** La opción gana sobre `NO_COLOR` y `CLICOLOR_FORCE`.
- **R4.4** `--color` requiere `--graph` (igual que `--ascii`) y aparece en `riku log --help` y en las completaciones de la shell.
- **R4.5** Con `-f json` no hay color, con cualquier valor.
- **R4.6** `docs/cli.md` documenta la opción y su prioridad frente a las variables de entorno.
- **R4.7** Hay pruebas automáticas de la decisión (`color_mode` × terminal × variables de entorno) y de que `never` no emite `\x1b[`.

## R5. Historial: Tab pasa a la lista de archivos

**Historia.** Como quien navega con el teclado, quiero ir de la lista de commits a los archivos del commit elegido y abrir uno sin tocar el mouse.

- **R5.1** Con el Historial abierto y ningún campo de texto con foco, **Tab** pasa el foco del teclado de la lista de commits a la lista de archivos del commit seleccionado; y vuelve con **Tab** o **Shift+Tab** (alternan entre las dos listas).
- **R5.2** Con el foco en los archivos, **↑/↓** cambian de archivo (con tope en los extremos, sin dar la vuelta) y **Enter** abre el diff del archivo marcado contra el primer padre, como un clic.
- **R5.3** Con el foco en los commits, **↑/↓** y **Enter** funcionan como hoy (Enter abre el primer archivo abrible).
- **R5.4** El archivo marcado se ve (resaltado) y se desplaza hasta quedar a la vista. Se ve también en qué lista está el foco.
- **R5.5** Los archivos que no se pueden abrir (`openable() == false`) se saltan al recorrer.
- **R5.6** Cambiar de commit pone el archivo marcado en el primero abrible; si el commit no tiene archivos, Tab no hace nada.
- **R5.7** Con el Historial cerrado, Tab no hace nada propio de Riku (no roba el foco de otros controles).
- **R5.8** `docs/gui.md` lista **Tab** junto a **↑/↓** y **Enter**.
- **R5.9** Hay pruebas automáticas del modelo (mover la selección de archivos, saltar los no abribles, topes).

## No funcionales

- **NF1** Ningún cambio de la ronda altera el formato de salida de los comandos (`json`, `text`) salvo lo pedido en R4.
- **NF2** Los mensajes nuevos van por `i18n` (`tr!`), como el resto.
- **NF3** Sin dependencias nuevas.
