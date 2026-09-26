<div align="center">

# Riku

**VCS semántico para diseño de chips.**
Revisa cambios en esquemáticos y layouts al nivel del circuito, no del texto.

[![CI](https://github.com/riku-chip/riku_chip/actions/workflows/ci.yml/badge.svg)](https://github.com/riku-chip/riku_chip/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#licencia)
[![Status](https://img.shields.io/badge/status-alpha-yellow)](#estado-del-proyecto)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey)](#)

[Qué hace](#qué-hace) ·
[Inicio rápido](#inicio-rápido) ·
[Uso](#uso) ·
[GUI](#gui-de-escritorio) ·
[Arquitectura](#arquitectura) ·
[Roadmap](#roadmap)

</div>

---

## Qué hace

Los archivos de diseño EDA (`.sch`, `.gds`, `.mag`) son difíciles de revisar en Git. Un `git diff` sobre un Xschem muestra coordenadas numéricas que no comunican nada. Riku interpreta los cambios y responde preguntas reales:

- ¿Qué componentes se añadieron, eliminaron o modificaron entre dos commits?
- ¿Cambió el valor de un resistor o transistor?
- ¿Se conectaron o desconectaron nets?
- ¿Fue solo un reordenamiento visual (Move All) o hubo cambios funcionales?

Para esquemáticos Xschem, además, genera un **diff visual interactivo** con los cambios resaltados en colores sobre el circuito renderizado.

Para layouts GDS responde las preguntas equivalentes en términos geométricos:

- ¿Qué área se añadió o eliminó, en qué capa y en qué celda?
- ¿El cambio está en la propia celda o viene de una sub-celda instanciada?
- En una librería de cientos de celdas, ¿cuáles cambiaron?
- ¿Es un cambio real o ruido por debajo de la grilla (cosmético)?

> Implementación 100 % Rust. No requiere `xschem`, KLayout, Magic ni ninguna otra herramienta EDA instalada en el sistema.

---

## Características

|   |   |
|---|---|
| **Diff semántico**     | Componentes añadidos, removidos, modificados. Distingue cambios funcionales de cosméticos (Move All). |
| **Diff visual**        | GUI nativa con paneles Before / After / Diff. Componentes anotados en verde (añadido), rojo (removido), amarillo (modificado), cyan (trasladado). |
| **Diff GDS**           | XOR geométrico por celda y capa, incluyendo cambios dentro de sub-celdas; áreas en µm², bbox y umbral cosmético. En la GUI: overlay verde/rojo, lista de cambios y celdas cambiadas marcadas. Verificado contra KLayout. |
| **Visor GDS**          | Paletas de SKY130, GF180MCU e IHP SG13G2 (de sus `.lyp` oficiales), selector de celdas con buscador, capas activables y tooltip con capa y área. |
| **Render nativo**      | Renderiza `.sch` a SVG sin abrir xschem. Usa `xschem-viewer` como librería Rust. |
| **Status semántico**   | `riku status` lista cambios del working tree clasificados como semánticos vs cosméticos por driver. |
| **Historial semántico**| `riku log` anota cada commit con un resumen por archivo (componentes/nets) y refs anotadas. |
| **Salida JSON estable**| `--json` con schemas versionados (`riku-status/v1`, `riku-log/v1`) para CI y scripts. |
| **Detección de PDK**   | Descubre rutas de símbolos desde `.xschemrc`, `$PDK_ROOT`/`$PDK` y `$TOOLS`. Si `$PDK` no está definida, elige el PDK instalado que tiene los símbolos de cada esquemático (o varios, si el diseño los mezcla). |
| **Arquitectura modular**| Núcleo (`riku-kernel`) que no conoce ningún formato y módulos que se registran en él (`FormatModule`: detectar, comparar y mostrar). Añadir un formato (Magic, KiCad…) es un módulo nuevo en `riku/src/modules/`; no toca el núcleo, la CLI ni el visor. |

---

## Formatos soportados

| Formato  | Extensión     | Diff semántico | Render GUI | Render SVG |
|----------|---------------|:--------------:|:----------:|:----------:|
| Xschem   | `.sch`, `.sym`| ✓              | ✓          | ✓          |
| GDS      | `.gds`        | ✓ geométrico (XOR) | ✓      | ✓ (librería `gds-renderer`) |
| OASIS    | `.oas`        | ✓ geométrico (XOR) | ✓      | ✓ (librería `gds-renderer`) |
| Magic    | `.mag`        | planificado    | planificado | — |
| NGSpice  | `.raw`        | planificado    | —          | — |

---

## Instalación (Linux)

Riku es **un solo ejecutable** (`riku`) con la CLI, el shell y el visor adentro, incluidos gdstk, el motor de Xschem y las paletas de los PDKs. Solo necesita glibc 2.35 o más nueva (Ubuntu 22.04+, Debian 12+, Fedora 36+, iic-osic-tools) y, para el visor, un escritorio con X11 o Wayland.

Desde [Releases](https://github.com/riku-chip/riku_chip/releases):

```bash
tar xf riku-<versión>-linux-x86_64.tar.gz
./riku-<versión>-linux-x86_64/install.sh          # en ~/.local/bin (o --system para /usr/local/bin)
# o bien
sudo apt install ./riku_<versión>-1_amd64.deb
```

```bash
riku                 # shell interactivo (Tab completa)
riku gui chip.gds    # visor
riku --version
```

Plataforma oficial: **Linux x86_64**. Windows compila en la CI, sin instaladores ni soporte.

---

## Inicio rápido

### Prerrequisitos

- **Rust 1.75+** (`rustup default stable`)
- **Git** (`riku` lee el repo con `libgit2`, no requiere el binario `git`)
- **Toolchain C++** + **zlib** + **qhull** (los necesita el backend GDS via `gdstk-rs`):
  - **Windows**: VS 2019 BuildTools+ y vcpkg (`vcpkg install zlib qhull --triplet x64-windows`).
    Por defecto se asume `VCPKG_ROOT=C:\vcpkg`; si tu vcpkg vive en otra ruta,
    sobrescribí la env var antes de `cargo build`. El workspace ya configura
    `VCPKGRS_DYNAMIC=1` via `.cargo/config.toml` para evitar el conflicto
    LNK2005 entre el zlib vendored de `libz-sys` y el zlib dinámico de vcpkg.
    Para ejecutar (no compilar) `riku`, agregá las DLLs al PATH:
    `set PATH=%VCPKG_ROOT%\installed\x64-windows\bin;%PATH%`.
  - **Linux**: paquetes `zlib1g-dev` y `libqhull-dev` (Debian/Ubuntu) o equivalentes.
  - **macOS**: `brew install zlib qhull pkg-config`.

> **Recomendado:** compilar en Linux. El contenedor [iic-osic-tools](https://github.com/iic-jku/iic-osic-tools) trae todo lo necesario (zlib, qhull, KLayout y los PDKs en `/foss/pdks`); basta `rustup default stable`. En Windows con MSVC 2019 gdstk-rs puede fallar por memoria o por DLLs de vcpkg: ver `docs/integracion_gds_estado.md`.

### Clonar el repo

Riku usa [`xschem-viewer`](https://github.com/carloscl03/xschem-viewer-rust) como **submodule git**. Cloná con `--recurse-submodules` y todo queda listo en un paso:

```bash
git clone --recurse-submodules https://github.com/riku-chip/riku_chip
cd riku_chip
```

Si ya cloñaste sin el flag, ejecutá:

```bash
git submodule update --init --recursive
```

### Compilar

Es un workspace de Cargo: un solo `Cargo.lock`, un solo `target/` y **un solo ejecutable**, `riku`, que trae la CLI, el shell y el visor. Los crates `gds-renderer`, `viewer-core`, `xschem-viewer-rust` y `gdstk-rs` entran como librerías.

```bash
cd riku_chip
cargo build --release
# Ejecutable: riku_chip/target/release/riku  (CLI + shell + visor)

# Versión solo de terminal (sin egui; servidores, CI):
cargo build --release -p riku --no-default-features
```

### Primer comando

```bash
cd tu-proyecto-xschem
riku doctor                                         # verifica el entorno
riku status                                         # cambios pendientes con resumen semántico
riku log                                            # historial con resumen por commit
riku diff HEAD~1 HEAD design/op_amp.sch             # diff de texto
riku diff HEAD~1 HEAD design/op_amp.sch -f visual   # diff visual (GUI)
```

---

## Uso

### Diff semántico — salida de texto

```bash
riku diff <commit_a> <commit_b> ruta/archivo.sch
```

```text
Archivo : design/op_amp.sch
Cambios : 3

  + M5
      symbol: sky130_fd_pr/nfet_01v8_lvt.sym
  - R2
  ~ C1
      value: 1p → 2p
```

### Diff — salida JSON (para CI)

```bash
riku diff <commit_a> <commit_b> archivo.sch --format json      # riku-diff/v2
riku diff <commit_a> <commit_b> archivo.sch --format json-v1   # forma anterior (transición)
```

Cada cambio dice qué le pasó (`kind`), a qué elemento (`element`, con su `type`) y cómo eran sus propiedades antes y después (`details`, con números reales, no texto):

```json
{
  "schema": "riku-diff/v2",
  "file": "design/op_amp.sch",
  "format": "xschem",
  "warnings": [],
  "changes": [
    { "kind": "added",   "element": { "type": "component", "name": "M5" }, "cosmetic": false },
    { "kind": "renamed", "element": { "type": "component", "name": "vin_diff" }, "renamed_from": "vin", "cosmetic": false },
    { "kind": "modified", "element": { "type": "component", "name": "C1" }, "cosmetic": false,
      "details": [ { "key": "value", "before": "1p", "after": "2p" } ] },
    { "kind": "added",   "element": { "type": "net", "name": "vbias" }, "cosmetic": false },
    { "kind": "added",   "element": { "type": "geometry", "cell": "TOP", "layer": 68, "datatype": 20,
                                      "via": { "path": ["INV"], "instances": 2 } },
      "cosmetic": false, "location": { "min_x": 12.0, "min_y": 10.0, "max_x": 13.0, "max_y": 11.0 },
      "details": [ { "key": "added_area_um2", "after": 0.25 } ] }
  ]
}
```

Tipos de `element`: `component`, `net`, `whole` (todo el archivo, p. ej. un Move All), `cell` y `geometry` (layouts). `--format json-v1` produce exactamente la salida anterior (`components`, `nets_added`, `nets_removed`, `is_move_all`) y se mantiene durante una versión. `status` y `log` siguen en `riku-status/v1` y `riku-log/v1`.

### Diff visual

```bash
riku diff <commit_a> <commit_b> archivo.sch --format visual
```

Abre la GUI con tres vistas: **Before** (commit A solo), **After** (commit B solo), **Diff** (B con anotaciones).

Leyenda:

| Color       | Significado                         |
|-------------|-------------------------------------|
| Verde       | componente o net añadido            |
| Rojo        | componente o net removido           |
| Amarillo    | componente modificado (valor, símbolo) |
| Cyan        | componente trasladado (solo posición) |
| Amarillo + borde cyan | modificado **y** trasladado |

### Diff de layouts GDS y OASIS

```bash
riku diff <commit_a> <commit_b> layout.gds                # texto
riku diff <commit_a> <commit_b> layout.gds -f json        # JSON para CI
riku diff <commit_a> <commit_b> layout.gds -f visual      # GUI
riku diff <commit_a> <commit_b> layout.gds --cosmetic-threshold-um2 0.05
riku diff <commit_a> <commit_b> chip.oas                  # OASIS: mismo diff
riku diff <commit_a> <commit_b> layout.gds --no-cache     # sin la cache de diffs
```

```text
Archivo : layout.gds
Cambios : 3
Cosméticos: 1

  ~ sky130_fd_sc_hd__inv_1:L66/20
      +1 polys / +0.125 µm²
      -3 polys / -0.125 µm²
      bbox: (0.320, 0.105) → (0.800, 2.615) µm
  - sky130_fd_sc_hd__inv_1:L67/44
      +0 polys / +0.000 µm²
      -1 polys / -0.029 µm²
      bbox: (0.605, 2.635) → (0.775, 2.805) µm
  + sky130_fd_sc_hd__inv_1:L68/20
      +1 polys / +0.230 µm²
      -0 polys / -0.000 µm²
      bbox: (0.100, 1.350) → (1.250, 1.550) µm
```

Cada cambio es `celda:Lcapa/datatype`; si nace en una sub-celda se añade su nombre (`TOP:L1/0:INV`) y el bbox queda en coordenadas de la celda que la instancia. Un cambio con área total bajo el umbral (por defecto 0,01 µm², debajo del piso DRC de sky130/gf180) se marca **cosmético**. Si el archivo no existía en `commit_a`, todas sus celdas aparecen como añadidas.

Más detalles del diff:

- **Instancias:** si cambia una sub-celda instanciada varias veces, la CLI lo agrupa (`origen: TOP → INV (en 6 instancias)`) y la GUI muestra un cambio por instancia, con su posición y su recuadro.
- **Renombres:** una celda renombrada sin cambios de geometría aparece como `r cell:INV → INV_X1`, no como baja + alta.
- **Formatos:** `.gds` y `.oas` se pueden mezclar entre versiones (el lector se elige por la firma del archivo).
- **Cache:** en layouts de más de 1 MiB el resultado se guarda en `~/.cache/riku/diff` (tope 512 MiB); repetir el diff o abrirlo en la GUI sale de ahí. Se desactiva con `--no-cache` o `RIKU_NO_CACHE=1`.

En la GUI (`-f visual`), las vistas son las mismas que para Xschem:

| Vista  | Muestra |
|--------|---------|
| Diff   | layout "después" atenuado; **verde** = área añadida, **rojo** = área eliminada |
| Before | versión A |
| After  | versión B |

La vista se conserva al cambiar de pestaña. El panel **Cambios** lista los cambios por capa (clic para encuadrar uno) y el selector de celdas marca las que cambiaron (`+` añadida, `−` eliminada, `~` modificada).

### Status del working tree

```bash
riku status                                    # cambios actuales con clasificación semántica
riku status --detail                           # entrada por componente/net cambiada
riku status --json --compact                   # salida JSON para CI (schema riku-status/v1)
```

### Historial semántico

```bash
riku log                                       # últimos 20 commits anotados
riku log design/op_amp.sch -n 10               # filtrado por archivo
riku log --json                                # JSON estable (schema riku-log/v1)
```

### Abrir un archivo en la GUI

```bash
riku open archivo.sch
# o directamente:
riku gui archivo.sch
riku gui layout.gds
riku gui chip.oas
riku gui sky130_fd_sc_hd.gds --cell sky130_fd_sc_hd__inv_1   # una celda concreta
```

### Verificar el entorno

```bash
riku doctor
```

Reporta estado de: repo Git, `.xschemrc`, variables `PDK_ROOT` / `PDK` / `TOOLS` y drivers cargados.

---

## GUI de escritorio

<div align="center">
<em>riku gui — navegación por árbol de proyecto, render vectorial, zoom/pan con la rueda del mouse, diff semántico con colores.</em>
</div>

La GUI nativa (`riku gui`) está construida con [egui](https://github.com/emilk/egui) / `eframe` sobre el backend `glow`. Características:

- **Árbol de proyecto** lateral con los archivos del directorio raíz.
- **Render vectorial** con pan (arrastrar), zoom anclado al cursor (rueda) y **Fit**.
- **Modo diff** con selector Before / After / Diff y panel de cambios con colores.
- **Fantasmas** — en modo Diff la versión anterior de lo que se movió o se eliminó se ve tenue debajo (esquemáticos).
- **Anotaciones de componente** (Xschem) — bounding boxes coloreados sobre los componentes cambiados.

Para GDS además:

- **Colores por PDK** con rol de capa: dispositivo (relleno), pozo (tinte tenue), implantes/marcadores/boundary/pines (solo contorno), en orden de apilado físico. El PDK se detecta por la ruta del archivo o por las capas presentes.
- **Selector de celdas** con buscador y filtros (solo top cells, solo con cambios).
- **Detalles** en secciones plegables: resumen (celda, PDK, conteos, tamaño), cambios y capas con checkbox (las capas ocultas se mantienen al cambiar de celda).
- **Tooltip** con capa, tamaño y área del polígono bajo el cursor.
- **Etiquetas legibles**: tamaño fijo en pantalla, fusionadas si comparten punto, sin solaparse.
- Polígonos cóncavos (earcut) y labels de toda la jerarquía con su anchor.

Usabilidad general:

- **Tema claro / oscuro / sistema** (se recuerda), con fundido suave al cambiar.
- **Atajos**: `F` encuadrar, `L` etiquetas, `+`/`−` zoom. Encuadrar se anima y el arrastre suelta con inercia ("Reducir movimiento" en Ajustes los desactiva).
- **Orientación**: ruta `commits › archivo › celda › vista` sobre el lienzo, pantalla inicial con archivos recientes y barra de estado con coordenadas en µm.
- **Feedback**: mensajes temporales sobre el lienzo; los errores quedan hasta cerrarlos, en lenguaje claro.
- **Arrastrar un archivo** a la ventana lo abre.

Se abre sola desde `riku diff ... --format visual` o como programa standalone.

---

## Detección automática de PDK

`riku` descubre los paths de símbolos en este orden:

### 1. `.xschemrc` del proyecto o de `~`

| Directiva                                | Efecto                                    |
|------------------------------------------|-------------------------------------------|
| `set PDK_ROOT /path`                     | Base del PDK                              |
| `set PDK sky130A`                        | Resuelve `$PDK_ROOT/$PDK/libs.tech/xschem`|
| `set XSCHEM_SHAREDIR /path`              | Añade `$XSCHEM_SHAREDIR/xschem_library/devices` |
| `append XSCHEM_LIBRARY_PATH :/path`      | Añade cada path separado por `:`          |

Solo se añaden paths existentes en disco.

### 2. Variables de entorno

| Variable              | Efecto                                        |
|-----------------------|-----------------------------------------------|
| `$PDK_ROOT` + `$PDK`  | `$PDK_ROOT/$PDK/libs.tech/xschem`             |
| `$TOOLS`              | `$TOOLS/xschem/share/xschem/xschem_library/devices` |

Útil en entornos Docker como `iic-osic-tools`, donde `sak-pdk sky130A` configura estas variables automáticamente.

---

## Arquitectura

Riku es un workspace de varios crates con una separación clara entre **contratos** (traits neutros) y **backends** (implementaciones por formato).

```
riku_chip/
├── viewer-core/                          ← trait ViewerBackend, RenderableScene, DrawElement neutros
├── riku-kernel/                          ← núcleo: tipos de cambio (FileChange, Change, Element), FormatModule y Registry; sin formatos
├── riku/                                 ← ejecutable: análisis y git (src/core), módulos de formato (src/modules: xschem, layout), CLI y visor (src/gui)
├── gds-renderer/                         ← backend GDS: escena, diff geométrico, paletas PDK, SVG
├── external/
│   ├── gdstk/               (submodule)  ← gdstk-rs: binding Rust de gdstk (C++)
│   └── xschem-viewer-rust/  (submodule)  ← backend Xschem: parser PEG, semantic, renderer
└── examples/                             ← esquemáticos de referencia
```

### Flujo de datos

```text
┌──────────┐   ┌────────────────┐   ┌───────────┐   ┌───────────┐
│ *.sch    │──▶│ XschemBackend  │──▶│ Scene     │──▶│ visor     │
│ *.gds    │──▶│ GdsBackend     │──▶│ (neutro)  │──▶│ riku      │
└──────────┘   └────────────────┘   └───────────┘   └───────────┘
               (impl ViewerBackend)  (viewer-core)    (consumidor)
```

Cualquier formato futuro solo necesita implementar `ViewerBackend` en su propio crate; los consumidores lo reciben como `Box<dyn ViewerBackend>` y no cambian.

### Dependencias clave

| Crate                                                                           | Rol                                                                  |
|---------------------------------------------------------------------------------|----------------------------------------------------------------------|
| [`xschem-viewer`](https://github.com/carloscl03/xschem-viewer-rust) (submodule) | Parser PEG y renderer SVG para `.sch` / `.sym`                       |
| `viewer-core`                                                                   | Trait neutro `ViewerBackend` y primitivas comunes de dibujo          |
| `git2` (libgit2)                                                                | Blobs, commits y diffs sin fork de proceso                           |
| `eframe` + `egui` (con backend `glow`)                                          | GUI nativa multiplataforma sin stack Vulkan/wgpu                     |
| `tokio` + `poll-promise`                                                        | Runtime async + integración con el loop de egui                      |
| `clap`                                                                          | CLI con subcomandos tipados                                          |
| `serde` / `serde_json`                                                          | Serialización JSON estable (`riku-status/v1`, `riku-log/v1`)         |
| `glob`                                                                          | Filtrado por patrones en `--paths` de `status` y `log`               |

---

## Desarrollo

### Compilación

Todo se compila desde la raíz del workspace:

```bash
cd riku_chip && cargo build --release          # target/release/riku
```

### Tests

```bash
cargo test --workspace                    # todo: núcleo, CLI, visor, gds-renderer, viewer-core
cargo test -p riku                        # núcleo + CLI + visor (incluye tests/gds_e2e.rs)
cargo test -p gds-renderer                # lógica GDS: diff, paletas, escena, cache
```

Cada crate tiene su propio `target/`. Esto evita acoplamiento de workspace y permite compilar `riku` aislado en entornos Docker restringidos, a costa de recompilar deps compartidas si trabajás en varios crates a la vez.

### Estructura de commits

Formato convencional: `tipo(scope): descripción`. Tipos comunes: `feat`, `fix`, `refactor`, `docs`, `chore`, `test`.

---

## Estado del proyecto

**Alpha.** Funciona end-to-end para Xschem (diff semántico, GUI y render vectorial) y para GDS (diff geométrico en CLI y GUI, visor con paletas SKY130/GF180/IHP). Detalle del estado GDS en `docs/integracion_gds_estado.md`.

### Roadmap

| Feature                                                             | Estado        |
|---------------------------------------------------------------------|---------------|
| Diff semántico Xschem (texto + JSON)                                | ✓ Estable     |
| Render GUI Xschem con anotaciones                                   | ✓ Estable     |
| Detección automática de PDK                                         | ✓ Estable     |
| `riku status` con clasificación semantic/cosmetic/unknown           | ✓ Estable     |
| `riku log` con resumen semántico y refs anotadas                    | ✓ Estable     |
| Salida JSON estable con schema versionado                           | ✓ Estable     |
| Diff GDS geométrico (texto + JSON), jerárquico, umbral cosmético    | ✓ Estable     |
| Visor y diff visual GDS en la GUI                                   | ✓ Estable     |
| OASIS, celdas renombradas, cambio por instancia, cache del diff    | ✓ Estable     |
| Shell interactivo (`riku` sin argumentos) con historial y Tab       | ✓ Estable     |
| Paquetes instalables (`.tar.gz`, `.deb`) y build en Windows         | planificado   |
| Driver Magic / NGSpice                                              | planificado   |
| `--graph` ASCII en `riku log`                                       | planificado   |
| Modo `--ci` (exit code por severidad)                               | planificado   |
| `riku show <commit> <file>`                                         | planificado   |

Pendientes técnicos priorizados (paridad de la vista `.sch`, empaquetado, layouts muy grandes…): [`docs/roadmap/pendientes.md`](docs/roadmap/pendientes.md).

---

## Contribuir

Las contribuciones son bienvenidas. Antes de abrir un PR:

1. Asegúrate de que `cargo test --workspace` pasa (la CI lo corre con `-D warnings`).
2. Sigue el estilo de commits convencional (`feat:`, `fix:`, `refactor:` …).
3. Abre el PR contra `main`; los cambios grandes pueden necesitar discusión previa en un issue.

---

## Licencia

[MIT](LICENSE)

---

<div align="center">

Hecho con Rust, en el ecosistema open-source de diseño de chips.

</div>
