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
| **Detección de PDK**   | Descubre rutas de símbolos desde `.xschemrc`, `$PDK_ROOT`/`$PDK` y `$TOOLS` sin configuración manual. |
| **Arquitectura plugin**| Trait `ViewerBackend` común a todos los formatos. Añadir un nuevo formato (GDS, KiCad, etc.) no toca riku-gui ni riku-cli. |

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
    Para ejecutar (no compilar) `riku-gui`, agregá las DLLs al PATH:
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

Cada producto vive en su propio crate y se compila desde adentro. No hay
workspace raíz: `riku` es el producto principal y los demás crates
(`gds-renderer`, `viewer-core`, `xschem-viewer-rust`, `gdstk`) entran como
librerías por path.

```bash
# CLI (riku)
cd riku_chip/riku
cargo build --release
# Binario: riku_chip/riku/target/release/riku

# GUI (riku-gui)
cd riku_chip/riku-gui
cargo build --release
# Binario: riku_chip/riku-gui/target/release/riku-gui
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

### Diff semántico — salida JSON (para CI)

```bash
riku diff <commit_a> <commit_b> archivo.sch --format json
```

```json
{
  "file": "design/op_amp.sch",
  "warnings": [],
  "components": [
    { "kind": "added",    "name": "M5", "cosmetic": false },
    { "kind": "removed",  "name": "R2", "cosmetic": false },
    { "kind": "modified", "name": "C1", "cosmetic": false,
      "before": {"value": "1p"}, "after": {"value": "2p"} }
  ],
  "nets_added": [],
  "nets_removed": [],
  "is_move_all": false
}
```

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
riku-gui archivo.sch
riku-gui layout.gds
riku-gui chip.oas
riku-gui sky130_fd_sc_hd.gds --cell sky130_fd_sc_hd__inv_1   # una celda concreta
```

### Verificar el entorno

```bash
riku doctor
```

Reporta estado de: repo Git, `.xschemrc`, variables `PDK_ROOT` / `PDK` / `TOOLS` y drivers cargados.

---

## GUI de escritorio

<div align="center">
<em>riku-gui — navegación por árbol de proyecto, render vectorial, zoom/pan con la rueda del mouse, diff semántico con colores.</em>
</div>

La GUI nativa (`riku-gui`) está construida con [egui](https://github.com/emilk/egui) / `eframe` sobre el backend `glow`. Características:

- **Árbol de proyecto** lateral con los archivos del directorio raíz.
- **Render vectorial** con pan (arrastrar), zoom anclado al cursor (rueda) y **Fit**.
- **Modo diff** con selector Before / After / Diff y panel de cambios con colores.
- **Fantasmas** (Xschem) — el commit A se muestra tenue debajo del B en modo Diff.
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
├── riku/                                 ← CLI: diff, log, status, doctor, open
├── riku-gui/                             ← GUI nativa egui con runtime Tokio para cargas async
├── gds-renderer/                         ← backend GDS: escena, diff geométrico, paletas PDK, SVG
├── external/
│   ├── gdstk/               (submodule)  ← gdstk-rs: binding Rust de gdstk (C++)
│   └── xschem-viewer-rust/  (submodule)  ← backend Xschem: parser PEG, semantic, renderer
└── examples/                             ← esquemáticos de referencia
```

### Flujo de datos

```text
┌──────────┐   ┌────────────────┐   ┌───────────┐   ┌───────────┐
│ *.sch    │──▶│ XschemBackend  │──▶│ Scene     │──▶│ riku-gui  │
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

Cada crate se compila desde adentro (no hay workspace unificado):

```bash
cd riku_chip/riku       && cargo build --release   # CLI
cd riku_chip/riku-gui   && cargo build --release   # GUI
cd riku_chip/gds-renderer && cargo build           # lib (consumida por los anteriores)
```

### Tests

```bash
cd riku_chip/riku           && cargo test    # CLI + integración (incluye tests/gds_e2e.rs)
cd riku_chip/gds-renderer   && cargo test    # lógica GDS: diff, paletas, escena
cd riku_chip/riku-gui       && cargo test    # transformaciones, relleno, selector, tooltip
cd riku_chip/viewer-core    && cargo test    # contrato neutro
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

1. Asegúrate de que `cargo test` pasa en cada crate que tocaste (`riku/`, `riku-gui/`, `gds-renderer/`, `viewer-core/`; no hay workspace raíz).
2. Sigue el estilo de commits convencional (`feat:`, `fix:`, `refactor:` …).
3. Abre el PR contra `main`; los cambios grandes pueden necesitar discusión previa en un issue.

---

## Licencia

[MIT](LICENSE)

---

<div align="center">

Hecho con Rust, en el ecosistema open-source de diseño de chips.

</div>
