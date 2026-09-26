# riku — crate principal

Motor de diff semántico y visual para archivos de diseño EDA. Lee el historial Git directamente, parsea los archivos de diseño y reporta cambios al nivel de componentes, conexiones y nets — no de texto crudo.

---

## Compilar

```bash
cargo build --release
cargo test
```

El binario queda en `target/release/riku`. No requiere ninguna herramienta EDA instalada.

---

## Shell interactivo

Ejecutar `riku` sin argumentos abre el shell interactivo:

```
    ██████╗ ██╗██╗  ██╗██╗   ██╗
    ██╔══██╗██║██║ ██╔╝██║   ██║
    ██████╔╝██║█████╔╝ ██║   ██║
    ██╔══██╗██║██╔═██╗ ██║   ██║
    ██║  ██║██║██║  ██╗╚██████╔╝
    ╚═╝  ╚═╝╚═╝╚═╝  ╚═╝ ╚═════╝

  v0.1.0  ·  PDK: sky130A [ok]  ·  /foss/designs/prueba

riku schematics (git)>
```

El prompt muestra el directorio actual y si hay un repositorio Git activo. Dentro del shell todos los comandos funcionan igual que en CLI, más los de navegación:

| Comando | Descripción |
|---------|-------------|
| `ls [ruta]` | Lista archivos `.sch`, `.sym`, `.gds` y `.oas` y subdirectorios. Marca `[git]` los que están bajo control de versiones. |
| `cd <ruta>` | Navega a otra carpeta sin salir del shell. Actualiza el repo Git activo automáticamente. |
| `help` | Muestra todos los comandos disponibles. |
| `exit` | Sale del shell. |

Ejemplo de sesión:

```
riku schematics (git)> ls
  [git]  circ_RM.sch
  [git]  prueba1_fuente.sch
         pruebaM1.sch

riku schematics (git)> log circ_RM.sch
  a3f2b1c  feat: ajustar valor resistor
  7d9e4a2  fix: corregir net VDD

riku schematics (git)> diff 7d9e4a2 a3f2b1c circ_RM.sch
  modified   R1

riku schematics (git)> cd ../layout
  → /foss/designs/prueba/memristor/layout

riku layout> ls
  (sin archivos .sch ni subdirectorios)
```

El historial de comandos persiste con ↑↓ durante la sesión. **Tab** completa comandos, flags de cada subcomando, carpetas (después de `cd`), ramas, tags, commits recientes y archivos de diseño.

---

## Comandos

### `riku diff`

Compara dos commits de un archivo de diseño y reporta los cambios semánticos.

```bash
riku diff <commit_a> <commit_b> <archivo> [--format text|json|json-v1|visual]
```

**Salida texto** (por defecto):
```
Archivo: design/op_amp.sch  (xschem)
Cambios: 3

  added      M5
  removed    R2
  modified   C1  [cosmetico]
```

**Salida JSON** (para CI/scripts):
```bash
riku diff HEAD~1 HEAD archivo.sch --format json
```
```json
{
  "schema": "riku-diff/v2",
  "file": "archivo.sch",
  "format": "xschem",
  "warnings": [],
  "changes": [
    { "kind": "added",    "element": { "type": "component", "name": "M5" }, "cosmetic": false },
    { "kind": "removed",  "element": { "type": "component", "name": "R2" }, "cosmetic": false },
    { "kind": "modified", "element": { "type": "component", "name": "C1" }, "cosmetic": true }
  ]
}
```

Esquema completo en el README principal; `--format json-v1` da la salida anterior durante una versión.

**Salida visual** — abre el visor (`riku gui`) con las vistas Diff, Before y After:
```bash
riku diff HEAD~1 HEAD archivo.sch --format visual
```

Código de colores de anotaciones:

| Color | Significado |
|-------|-------------|
| Verde | Componente o net añadido |
| Rojo | Componente o net removido |
| Amarillo | Componente modificado (valor, parámetro) |
| Gris | Cambio cosmético (solo reposicionamiento) |

---

### `riku log`

Lista el historial de commits con resumen semántico por archivo y refs anotadas.

```bash
riku log [archivo.sch] [--detail|--full] [--json [--compact]] [--paths PAT] [--branch REF] [-n <n>]
```

Por defecto muestra los últimos 20 commits. Cada commit anota refs (rama, tag, HEAD) y, para los archivos con driver (`.sch`), un resumen de componentes y nets cambiados respecto a su primer padre. Los merges se marcan con `[merge]` y no incluyen diff por archivo en v1.

Salida JSON estable bajo el schema `riku-log/v1`.

---

### `riku status`

Reporta el estado del working tree comparado con `HEAD`.

```bash
riku status [--detail|--full] [--json [--compact]] [--paths PAT] [--include-unknown]
```

Cada archivo modificado se clasifica como `semantic` (cambios funcionales), `cosmetic` (solo reposicionamiento), `unchanged` (driver no detecta cambios) o `unknown` (sin driver). Salida JSON estable bajo el schema `riku-status/v1`.

Códigos de salida: `0` limpio, `1` con cambios semánticos, `2` error.

---

### `riku doctor`

Verifica el estado del entorno.

```bash
riku doctor
```

Comprueba:
- PDK detectado (`$PDK_ROOT`/`$PDK` o `.xschemrc`)
- Repositorio Git válido
- Drivers cargados

---

## Detección de PDK

El renderer busca símbolos en el siguiente orden:

1. **`.xschemrc`** en el directorio actual o en `~`
   - `set PDK_ROOT /path` + `set PDK sky130A` → `$PDK_ROOT/$PDK/libs.tech/xschem`
   - `set XSCHEM_SHAREDIR /path` → `$XSCHEM_SHAREDIR/xschem_library/devices`
   - `append XSCHEM_LIBRARY_PATH :/path` → paths adicionales separados por `:`

2. **Variables de entorno** (fallback cuando no hay `.xschemrc`)
   - `$PDK_ROOT` + `$PDK` → `$PDK_ROOT/$PDK/libs.tech/xschem`
   - `$TOOLS` → `$TOOLS/xschem/share/xschem/xschem_library/devices`

Solo se añaden los paths que existen en disco. En entornos como `iic-osic-tools`, `sak-pdk sky130A` configura estas variables automáticamente — no se necesita ningún archivo extra.

---

### `open`

```text
riku open [archivo.sch | archivo.gds | archivo.oas]
```

Abre el visor con el archivo, en un proceso aparte (sin argumento, el árbol del directorio actual). `riku gui archivo` hace lo mismo bloqueando la terminal.

## Estructura

```
src/
  main.rs               — punto de entrada
  lib.rs                — módulos públicos
  cli/                  — subcomandos, shell (con Tab), doctor, gui y formatos de salida (format/)
  core/
    git/                — blobs, commits, ramas y working tree via git2
    analysis/           — diff entre commits, status y log; reciben el registro de módulos
    domain/             — modelos, errores y puertos (traits)
  modules/
    mod.rs              — registry(): el único lugar que lista los módulos de formato
    xschem.rs           — módulo .sch (diff semántico)
    xschem_view.rs      — visor .sch (escena neutra, fantasmas, anotaciones)
    xschem_pdk.rs       — ruta de símbolos del PDK y detección por símbolos
    layout.rs           — módulo .gds/.oas (diff geométrico, visor)
  gui/                  — visor egui (feature `gui`)
tests/
  basic.rs              — 9 tests de integración
  gds_e2e.rs            — 6 tests end-to-end de layouts
  stress.rs             — 13 tests de rendimiento y casos límite
```

---

## Tests

```bash
cargo test                  # todos
cargo test --test basic     # integración
cargo test --test stress    # rendimiento
```

---

## Dependencias

| Crate | Rol |
|-------|-----|
| `riku-kernel` | Tipos de cambio neutros, `FormatModule` y `Registry` |
| `xschem-viewer` (submodule) | Parser PEG y semántica de `.sch` / `.sym` |
| `git2` | Acceso a blobs y commits sin fork de proceso |
| `clap` | CLI con subcomandos tipados |
| `serde` / `serde_json` | Serialización JSON estable (`riku-status/v1`, `riku-log/v1`) |
| `glob` | Filtros `--paths` en status / log |
| `dirs` | Home del sistema (lookup de `.xschemrc`) |
| `thiserror` | Tipos de error ergonómicos |
| `rustyline` | Shell interactivo con historial y edición de línea |

---

## Notas

- `diff --format visual` abre el visor (`riku gui`) con los argumentos del diff.
- No hay comando de render a archivo: el visor dibuja la escena en memoria. Exportar a SVG/PNG desde el visor está pendiente (`docs/roadmap/pendientes.md`).
- `diff` soporta Xschem `.sch` (diff semántico) y layouts `.gds`/`.oas` (diff geométrico por celda y capa, con `--cosmetic-threshold-um2`; renombres, cambios por instancia y cache en disco que se apaga con `--no-cache`). Detalle GDS en `docs/integracion_gds_estado.md`. Magic y NGSpice están en roadmap.
