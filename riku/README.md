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
| `ls [ruta]` | Lista archivos `.sch` y subdirectorios. Marca `[git]` los que están bajo control de versiones. |
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

El historial de comandos persiste con ↑↓ durante la sesión.

---

## Comandos

### `riku diff`

Compara dos commits de un archivo de diseño y reporta los cambios semánticos.

```bash
riku diff <commit_a> <commit_b> <archivo.sch> [--format text|json|visual]
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
  "file_type": "xschem",
  "warnings": [],
  "changes": [
    { "kind": "added",    "element": "M5",  "cosmetic": false },
    { "kind": "removed",  "element": "R2",  "cosmetic": false },
    { "kind": "modified", "element": "C1",  "cosmetic": true  }
  ]
}
```

**Salida visual** — abre un HTML con dos paneles SVG lado a lado:
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

Lista el historial de commits, opcionalmente filtrado por archivo.

```bash
riku log [archivo.sch] [--semantic] [--limit <n>]
```

Con `--semantic`, cada commit muestra un resumen de cambios detectados en el archivo.

---

### `riku render`

Renderiza un archivo `.sch` a SVG y lo abre con el visor del sistema.

```bash
riku render archivo.sch
```

El SVG se guarda en caché por SHA-256 del contenido — si el archivo no cambió, se reutiliza instantáneamente.

---

### `riku doctor`

Verifica el estado del entorno.

```bash
riku doctor
```

Comprueba:
- PDK detectado (`$PDK_ROOT`/`$PDK` o `.xschemrc`)
- Repositorio Git válido
- Directorio de caché accesible

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

## Estructura

```
src/
  main.rs               — punto de entrada
  cli.rs                — subcomandos y lógica de presentación
  lib.rs                — módulos públicos
  core/
    models.rs           — Component, Wire, Schematic, DiffReport
    driver.rs           — trait RikuDriver
    git_service.rs      — blobs y commits via git2
    analyzer.rs         — orquestador: Git + driver + report
    registry.rs         — despacho de driver por extensión
    semantic_diff.rs    — diff semántico de Schematics
    svg_annotator.rs    — inyección de anotaciones SVG
    ports.rs            — traits GitRepository, SchematicParser
  parsers/
    xschem.rs           — delega en xschem_viewer
  adapters/
    xschem_driver.rs    — implementa RikuDriver para .sch
tests/
  basic.rs              — 9 tests de integración
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
| `xschem-viewer` | Parser PEG + renderer SVG nativo |
| `git2` | Acceso a blobs y commits sin fork de proceso |
| `clap` | CLI con subcomandos tipados |
| `sha2` | Hash SHA-256 para caché de renders |
| `serde` / `serde_json` | Serialización JSON |
| `tempfile` | HTML temporal para diff visual |
| `dirs` | Home y caché del sistema |
| `thiserror` | Tipos de error ergonómicos |
| `rustyline` | Shell interactivo con historial y edición de línea |
