# Ronda 1: diseño

Cómo se cumple cada requisito de [`requirements.md`](requirements.md), con los archivos que se tocan (revisados el 2026-10-03 sobre `main`).

## D1. Formato y Clippy en la CI (R1)

**Hallazgo.** No hay `rustfmt.toml` y el código usa líneas muy largas (varias de más de 140 columnas) y `if`/`else` cortos en una línea (`riku/src/cli/format/color.rs`). Con la configuración por defecto, `cargo fmt` reescribiría casi todo. Por eso el primer paso es **elegir la configuración que menos cambie**, no formatear a ciegas.

**Cómo se elige.** En el contenedor, probar candidatas y quedarse con la que deje menos líneas por cambiar:

```text
# candidata A                     # candidata B
max_width = 140                   max_width = 140
use_small_heuristics = "Max"      (resto por defecto)
```

Se mide con `cargo fmt --check -p riku … | grep -c '^Diff in'`. Solo opciones estables (la CI usa `stable`; `fn_single_line` y similares son de nightly y no sirven). El resultado se escribe en `rustfmt.toml` de la raíz.

**Alcance.** El workspace excluye `external/`, pero `cargo fmt --all` también formatea las dependencias por path. Por eso la CI y la documentación usan los paquetes por nombre:

```text
cargo fmt --check -p viewer-core -p riku-kernel -p riku-mod-layout -p riku
```

**CI (`.github/workflows/ci.yml`).** Dos jobs nuevos, aparte de `test` para que fallen y se lean por separado:

| Job | Qué hace | Bloquea |
|---|---|---|
| `fmt` | `dtolnay/rust-toolchain@stable` con `components: rustfmt`; el `cargo fmt --check` de arriba. Sin dependencias del sistema: es rápido | sí |
| `clippy` | las mismas dependencias del sistema que `test`; `components: clippy`; `cargo clippy --workspace --all-targets --locked` sin `-D warnings` (el `RUSTFLAGS: -D warnings` es solo del job `test`) | no (`continue-on-error: true`) |

El Release llama a la CI con `workflow_call`; con `continue-on-error` en `clippy` y `fmt` bloqueando, un Release con formato roto no sale. Es lo esperado.

**Después del commit de formato.** `.git-blame-ignore-revs` con el hash del commit (GitHub lo respeta solo; en local, `git config blame.ignoreRevsFile .git-blame-ignore-revs`). Se documenta en `docs/desarrollo.md`.

**Pasar Clippy a bloqueante** (otra ronda): cuando el log quede en cero, se quita `continue-on-error` y se agrega `-D warnings`.

## D2. `upload-artifact` en Node 24 (R2)

Solo hay un uso: `release.yml`, `actions/upload-artifact@v4`. Los pasos:

1. Ver qué versión corre en Node 24: en el `action.yml` de cada tag, `runs.using: node24` (`gh api repos/actions/upload-artifact/contents/action.yml?ref=<tag>`). No se asume el número de versión: se lee.
2. Subirla en `release.yml`. Los parámetros que usamos (`name`, `path` con varias rutas) no cambian entre versiones mayores; se confirma en las notas de la versión.
3. Mismo chequeo para las otras acciones: `actions/checkout@v5` (ya Node 24), `Swatinem/rust-cache@v2`, y `dtolnay/rust-toolchain` (compuesta, sin Node). Si `rust-cache@v2` sigue en Node 20, se sube a la versión que no.
4. Verificación: `workflow_dispatch` del Release en una rama (no se publica: `gh release create` solo corre con tag `v*`) y se revisa que no salga el aviso de Node 20.

## D3. `riku render` y las capas ocultas (R3)

**Causa.** `scene_svg` (`riku/src/export/svg.rs`, línea ~142) recorre la escena con `scene.visit(...)` y dibuja todo elemento. `LayerPaint::hidden` solo lo lee el visor (`gui/content.rs:173` arma el conjunto inicial de capas ocultas); el exportador no lo mira. La capa "Transistores" (`riku-mod-layout/src/viewer_core_compat.rs:272`) tiene `hidden: true`, así que sale en la imagen con su amarillo y sus etiquetas.

**Cambio.** Antes del `visit`, armar el conjunto de capas ocultas con la misma fuente que usa el visor, y saltarlas:

```rust
let hidden: HashSet<Layer> =
    scene.layer_list().into_iter().filter(|(_, p)| p.hidden).map(|(l, _)| l).collect();
scene.visit(&bbox.inflate(…), &mut |el| {
    if hidden.contains(&el.layer()) {
        return true; // seguir con el siguiente
    }
    …
});
```

- Los textos de esa capa también se saltan: están en la misma capa, por eso no hace falta tocar `labels`.
- `ghost()` y `annotations()` no pasan por ahí y no cambian (R3.4).
- Una capa sin `LayerPaint` no está en `layer_list()`, así que se sigue dibujando (R3.2).
- El encuadre (`scene_bbox`) queda igual: la compuerta de un transistor está dentro del layout y no lo agranda. Se confirma con el demo `ota`.
- No se agrega opción para forzar las capas ocultas (ver `plan.md`, fuera de la ronda).

**Prueba.** En el módulo de pruebas de `svg.rs`, junto a `draws_elements_annotations_and_escapes_text`: una escena con la capa 7 visible y la 8 oculta; el SVG tiene los elementos de la 7 y ninguno de la 8.

## D4. `log --graph --color` (R4)

**Estado actual.** La decisión de color está escrita dos veces con la misma fórmula: `color::enabled()` (`riku/src/cli/format/color.rs`, con `OnceLock`) y `log_graph::Style::detect` (`format/log_graph.rs:44`). `log --graph` solo usa la segunda.

**Cambio.**

1. En `color.rs`, un tipo y una función pura:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorMode { #[default] Auto, Always, Never }

/// `Always`/`Never` mandan; `Auto` es la regla de hoy.
pub fn resolve(mode: ColorMode, is_tty: bool, clicolor_force: bool, no_color: bool) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => clicolor_force || (is_tty && !no_color),
    }
}
```

2. `color::enabled()` y `Style::detect` llaman a `resolve` con los datos reales del entorno: una sola fórmula. `color::set_mode(mode)` guarda el modo antes de que `enabled()` se evalúe (el `OnceLock` se llena en el primer uso).
3. `Style::detect(ascii, mode)`.
4. CLI (`cli/mod.rs`, junto a `ascii`): `#[arg(long, value_enum, requires = "graph", value_name = "WHEN", help = tr!("help.color"))] color: Option<ColorMode>`. Es `Option` y no un valor por defecto para que `requires = "graph"` solo se active cuando la persona escribe la opción. `dispatch.rs` la pasa en los dos sitios donde pasa `ascii`; `LogArgs` en `commands.rs` recibe `color`.
5. Con `-f json` no se llega a `log_graph`, así que no hay color (R4.5).
6. `i18n`: la clave `help.color` en los dos idiomas.
7. `docs/cli.md`: la línea de uso (`[--graph [--ascii] [--color auto|always|never]]`) y la frase de la línea ~171, con la prioridad: opción > `CLICOLOR_FORCE`/`NO_COLOR` > terminal.

**Pruebas.** Tabla de `resolve` (modo × tty × `CLICOLOR_FORCE` × `NO_COLOR`); `render` con `Style { color: false, .. }` no contiene `\x1b[` y con `color: true` sí (ya hay pruebas de `render` en `log_graph.rs` que se pueden copiar). Completaciones: se confirma que `--color` aparece en `shell_complete`.

## D5. Historial: Tab a la lista de archivos (R5)

**Estado actual.** `gui/history/mod.rs`, en `show` (~línea 262): si el Historial está abierto y `!egui_wants_keyboard_input()`, **↑/↓** llaman a `model.move_selection(delta)` (commits) y **Enter** a `model.open_first()`. Los archivos del commit se dibujan en `details()` → `file_row()` como botones; solo un clic los abre (`model.open(&path)`). No hay noción de "archivo marcado".

**Modelo (`history/model.rs`).**

```rust
pub enum Focus { Commits, Files }

pub struct HistoryModel {
    …
    pub focus: Focus,
    pub file_selected: Option<usize>, // índice en details[oid].files
}
```

- `toggle_focus()`: de `Commits` a `Files` solo si el commit elegido tiene detalles cargados y al menos un archivo abrible; si no, no hace nada (R5.6). Al pasar a `Files`, marca el primer abrible.
- `move_file(delta)`: avanza saltando los que no son `openable()`, con tope en los extremos (R5.2, R5.5).
- `open_marked()`: `open(&path)` del archivo marcado.
- Cambiar de commit (`select`, `move_selection`, `set_graph`) devuelve `focus` a `Commits` y limpia `file_selected`.

Todo esto es lógica sin egui y se prueba en el módulo de pruebas de `model.rs` (R5.9), como ya se prueba `move_selection`.

**Teclas (`mod.rs`, `show`).** Se amplía el bloque que ya existe:

| Foco | ↑/↓ | Enter | Tab / Shift+Tab |
|---|---|---|---|
| `Commits` | `move_selection` | `open_first` | `toggle_focus` |
| `Files` | `move_file` | `open_marked` | `toggle_focus` |

Tab y Shift+Tab hacen lo mismo: solo hay dos listas, así que "ir y volver" no necesita dirección.

**Dibujo.** `file_row` recibe si está marcado y lo pinta con `Button::selectable(true, …)` (el mismo estilo de la fila de commit elegida). Si el foco está en `Files`, la fila marcada hace `scroll_to_me` una vez por cambio (como `scroll_to_selection` en la lista de commits). La lista de commits mantiene su fila elegida; el cambio de foco se ve porque el marcado de archivo solo existe en `Files`.

**Riesgo principal: Tab en egui.** egui usa Tab para mover el foco entre widgets (los botones de `file_row` lo son). Si no se evita, a la vez que nuestro `toggle_focus` egui salta el foco a otro botón y se ve un anillo de foco raro, o se ejecuta el botón con Enter. Hay que resolverlo con una prueba corta (T5.0) antes de escribir lo demás. Opciones, de más a menos preferida:

1. Registrar un widget del panel con `ctx.memory_mut(|m| m.set_focus_lock_filter(id, EventFilter { tab: true, ..Default::default() }))` mientras el Historial esté abierto y sin campo de texto con foco, de modo que egui entregue el Tab a la aplicación.
2. Quitar a los botones de archivo la capacidad de tomar el foco por teclado (se manejan solo con nuestra marca) y consumir el evento con `input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab))`.
3. Si ninguna basta en la versión de egui que usamos, otra tecla que no esté tomada (queda a decidir; no se usa otra sin preguntar).

Con el Historial cerrado, nada de esto corre (R5.7).

**Docs.** `docs/gui.md`, línea 66: "**↑/↓** cambian de commit" pasa a incluir "**Tab** pasa a los archivos del commit; ahí **↑/↓** eligen y **Enter** abre". Clave nueva en `i18n` solo si se agrega un texto de ayuda en pantalla.

## Cómo se prueba cada cosa

| Tarea | Automática | A mano |
|---|---|---|
| 1 | `cargo fmt --check`; `cargo test --workspace --locked` antes y después | la CI de la rama |
| 2 | — | `workflow_dispatch` del Release; sin aviso de Node 20 |
| 3 | prueba nueva en `svg.rs` | `riku demo ota` → `riku render` de un layout con transistores: sin el amarillo |
| 4 | pruebas de `resolve` y de `render` | `riku log --graph --color always \| cat` con códigos; `--color never` en una terminal sin códigos |
| 5 | pruebas del modelo | GUI en Xvfb: abrir el Historial, Tab, ↑/↓, Enter, Tab de vuelta |
