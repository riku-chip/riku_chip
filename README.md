<div align="center">

# Riku

**VCS semántico para diseño de chips.**
Revisa cambios en esquemáticos y layouts al nivel del circuito, no del texto.

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

> Implementación 100 % Rust. No requiere `xschem`, KLayout, Magic ni ninguna otra herramienta EDA instalada en el sistema.

---

## Características

|   |   |
|---|---|
| **Diff semántico**     | Componentes añadidos, removidos, modificados. Distingue cambios funcionales de cosméticos (Move All). |
| **Diff visual**        | GUI nativa con paneles Before / After / Diff. Componentes anotados en verde (añadido), rojo (removido), amarillo (modificado), cyan (trasladado). |
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
| GDS      | `.gds`, `.oas`| —              | en desarrollo | — |
| Magic    | `.mag`        | planificado    | planificado | — |
| NGSpice  | `.raw`        | planificado    | —          | — |

---

## Inicio rápido

### Prerrequisitos

- **Rust 1.75+** (`rustup default stable`)
- **Git** (`riku` lee el repo con `libgit2`, no requiere el binario `git`)

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

```bash
# CLI (riku)
cd riku_chip/riku
cargo build --release
# Binario: riku_chip/riku/target/release/riku

# GUI (riku-gui) — opcional
cd riku_chip
cargo build --release -p riku-gui
# Binario: riku_chip/target/release/riku-gui
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

- **Árbol de proyecto** lateral con todos los `.sch` del directorio raíz.
- **Render vectorial** con pan (arrastrar) y zoom (rueda).
- **Modo diff** con selector Before / After / Diff y panel de cambios con colores.
- **Fantasmas** — el commit A se muestra tenue debajo del B en modo Diff.
- **Anotaciones de componente** — bounding boxes coloreados sobre los componentes cambiados.

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
├── gds-renderer/                         ← backend GDS (en desarrollo)
├── external/
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

### Compilación completa

```bash
cd riku_chip
cargo build                    # CLI + GUI + viewer-core
cargo build --release          # binarios optimizados
```

### Tests

```bash
cd riku_chip/riku
cargo test                     # 22 tests (integración + stress)
```

Riku está **excluido del workspace** de Cargo porque se compila independientemente en entornos Docker restringidos. Los tests se corren desde dentro del crate.

### Estructura de commits

Formato convencional: `tipo(scope): descripción`. Tipos comunes: `feat`, `fix`, `refactor`, `docs`, `chore`, `test`.

---

## Estado del proyecto

**Alpha.** Funciona end-to-end para Xschem con diff semántico, GUI y render vectorial. La integración GDS está en desarrollo activo.

### Roadmap

| Feature                                                             | Estado        |
|---------------------------------------------------------------------|---------------|
| Diff semántico Xschem (texto + JSON)                                | ✓ Estable     |
| Render GUI Xschem con anotaciones                                   | ✓ Estable     |
| Detección automática de PDK                                         | ✓ Estable     |
| `riku status` con clasificación semantic/cosmetic/unknown           | ✓ Estable     |
| `riku log` con resumen semántico y refs anotadas                    | ✓ Estable     |
| Salida JSON estable con schema versionado                           | ✓ Estable     |
| Backend GDS (`gds-renderer`)                                        | en desarrollo |
| Driver Magic / NGSpice                                              | planificado   |
| `--graph` ASCII en `riku log`                                       | planificado   |
| Modo `--ci` (exit code por severidad)                               | planificado   |
| `riku show <commit> <file>`                                         | planificado   |

---

## Contribuir

Las contribuciones son bienvenidas. Antes de abrir un PR:

1. Asegúrate de que `cargo test` pasa en `riku/` y `cargo build` pasa en la raíz del workspace.
2. Sigue el estilo de commits convencional (`feat:`, `fix:`, `refactor:` …).
3. Abre el PR contra `main`; los cambios grandes pueden necesitar discusión previa en un issue.

---

## Licencia

[MIT](LICENSE)

---

<div align="center">

Hecho con Rust, en el ecosistema open-source de diseño de chips.

</div>
