# Riku — VCS semántico para diseño de chips

Riku es una herramienta de control de versiones semántico construida sobre Git, diseñada para archivos de diseño EDA. En lugar de mostrar diffs de texto crudo sobre formatos propietarios, Riku interpreta los cambios al nivel de **componentes, conexiones y nets** — el vocabulario real del diseño de circuitos.

**Implementación completa en Rust. Sin dependencia del binario `xschem` ni de ninguna herramienta EDA instalada.**

---

## Por qué existe

Los archivos de diseño EDA (`.sch`, `.gds`, `.mag`) son difíciles de revisar en Git. Un `git diff` sobre un archivo Xschem muestra líneas de coordenadas numéricas que no comunican nada significativo. Riku parsea esos archivos y responde preguntas como:

- ¿Qué componentes se añadieron o eliminaron entre estos dos commits?
- ¿Cambió el valor de algún resistor o transistor?
- ¿Se conectaron o desconectaron nets?
- ¿Fue este cambio solo un reordenamiento visual (Move All) o hubo modificaciones reales?

Y para el caso de Xschem, también muestra un **diff visual** — dos paneles con el esquemático renderizado antes y después, con los cambios marcados en colores.

---

## Características

| | |
|---|---|
| **Diff semántico** | Detecta componentes añadidos, removidos y modificados. Distingue cambios funcionales de cambios puramente cosméticos (Move All). |
| **Diff visual** | Genera un HTML con dos paneles SVG lado a lado (antes/después). Los cambios se anotan con bounding boxes de colores sobre el esquemático renderizado. |
| **Render nativo** | Renderiza `.sch` a SVG sin abrir xschem. Usa `xschem-viewer` como librería Rust. |
| **Caché de renders** | Cada render se guarda por hash SHA-256 del contenido. Si el archivo no cambió, el SVG se reutiliza instantáneamente. |
| **Historial semántico** | `riku log` muestra el historial de commits anotado con un resumen de cambios (`+2 -1 ~3`) por cada revisión. |
| **Detección de PDK** | Detecta rutas de símbolos automáticamente desde `.xschemrc`, variables de entorno `PDK_ROOT`/`PDK`, y `$TOOLS`. |

---

## Formatos soportados

| Formato | Extensión | Diff semántico | Render |
|---------|-----------|:--------------:|:------:|
| Xschem  | `.sch`    | ✓ | ✓ |
| KLayout | `.gds` / `.oas` | — | — |
| Magic   | `.mag`    | — | — |
| NGSpice | `.raw`    | — | — |

La arquitectura de drivers está lista para extender cualquier formato. Xschem es el primero completamente implementado.

---

## Instalación

```bash
git clone https://github.com/riku-chip/riku_chip
cd riku_chip/riku
cargo build --release
# Binario en: target/release/riku
```

Requiere Rust 1.75+. No requiere `xschem`, KLayout, ni ninguna otra herramienta EDA instalada.

---

## Uso

### Diff semántico (texto)

```bash
riku diff <commit_a> <commit_b> ruta/archivo.sch
```

Salida de ejemplo:

```
Archivo: design/op_amp.sch  (xschem)
Cambios: 3

  added      M5
  removed    R2
  modified   C1
```

### Diff semántico (JSON — para CI/scripts)

```bash
riku diff <commit_a> <commit_b> archivo.sch --format json
```

```json
{
  "file_type": "xschem",
  "warnings": [],
  "changes": [
    { "kind": "added",    "element": "M5",  "cosmetic": false },
    { "kind": "removed",  "element": "R2",  "cosmetic": false },
    { "kind": "modified", "element": "C1",  "cosmetic": false, "before": {"value": "1p"}, "after": {"value": "2p"} }
  ]
}
```

### Diff visual

```bash
riku diff <commit_a> <commit_b> archivo.sch --format visual
```

Abre un HTML en el navegador con dos paneles SVG lado a lado. Los cambios se marcan con:
- **Verde** — componente o net añadido
- **Rojo** — componente o net removido
- **Amarillo** — componente modificado (valor, parámetro)
- **Gris** — cambio cosmético (solo reposicionamiento)

### Renderizar un archivo local

```bash
riku render archivo.sch
```

Renderiza el esquemático a SVG y lo abre en el visor del sistema. Útil para inspeccionar un archivo sin hacer un diff.

### Historial semántico

```bash
# Todos los commits del repositorio
riku log

# Filtrado por archivo, con resumen semántico por commit
riku log ruta/archivo.sch --semantic --limit 10
```

### Verificar el entorno

```bash
riku doctor
```

Muestra el estado del PDK detectado, el repositorio Git y el directorio de caché.

---

## Arquitectura

```
riku/
  src/
    main.rs               — punto de entrada
    cli.rs                — subcomandos: diff, log, render, doctor
    lib.rs                — módulos públicos
    core/
      models.rs           — Component, Wire, Schematic, DiffReport, ComponentDiff
      driver.rs           — trait RikuDriver (interfaz de cada formato)
      git_service.rs      — lectura de blobs y commits via git2
      analyzer.rs         — orquestador: Git + driver + report
      registry.rs         — despacho de driver por extensión de archivo
      semantic_diff.rs    — diff semántico de Schematics
      svg_annotator.rs    — inyección de anotaciones en SVGs
      ports.rs            — traits GitRepository, SchematicParser
    parsers/
      xschem.rs           — delega parsing y netlist en xschem_viewer
    adapters/
      xschem_driver.rs    — implementa RikuDriver para .sch
  tests/
    basic.rs              — 9 tests de integración (git, parser, diff)
    stress.rs             — 13 tests de rendimiento y casos límite

gds-renderer/             — motor de render GDS → SVG (crate separado)
examples/
  SH/op_sim.sch           — esquemático de referencia (sky130A op-amp)
  GDS/                    — ejemplos de layout GDS
```

---

## Detección automática de PDK

`riku render` y `riku diff --format visual` detectan automáticamente los paths de símbolos del PDK en el siguiente orden de prioridad:

### 1. `.xschemrc` del proyecto o de `~`

| Directiva | Efecto |
|-----------|--------|
| `set PDK_ROOT /path` | Base del PDK |
| `set PDK sky130A` | Nombre del PDK → resuelve `$PDK_ROOT/$PDK/libs.tech/xschem` |
| `set XSCHEM_SHAREDIR /path` | Añade `$XSCHEM_SHAREDIR/xschem_library/devices` |
| `append XSCHEM_LIBRARY_PATH :/path` | Añade cada path separado por `:` |

Solo se añaden los paths que existen en disco. Si no se encuentra ningún `.xschemrc`, se continúa con los siguientes fallbacks.

### 2. Variables de entorno del sistema

| Variable | Efecto |
|----------|--------|
| `$PDK_ROOT` + `$PDK` | Resuelve `$PDK_ROOT/$PDK/libs.tech/xschem` |
| `$TOOLS` | Resuelve `$TOOLS/xschem/share/xschem/xschem_library/devices` |

Útil en entornos Docker como `iic-osic-tools` donde `sak-pdk sky130A` configura estas variables automáticamente.

---

## Dependencias clave

| Crate | Rol |
|-------|-----|
| [`xschem-viewer`](https://github.com/carloscj03/xschem-viewer-rust) | Parser PEG + renderer SVG nativo para `.sch` y `.sym` |
| `git2` | Acceso a blobs, commits y diffs Git (sin fork de proceso) |
| `clap` | CLI con subcomandos y argumentos tipados |
| `sha2` | Hash SHA-256 para la clave de caché de renders |
| `serde` / `serde_json` | Serialización del output JSON |
| `tempfile` | Archivos temporales para el diff visual |
| `dirs` | Detección del directorio home y caché del sistema |

`xschem-viewer` se importa directamente desde GitHub como dependencia git — no requiere publicación en crates.io.

---

## Lo que falta implementar

| Feature | Estado | Notas |
|---------|--------|-------|
| **Drivers KLayout / Magic / NGSpice** | Pendiente | Arquitectura lista; falta implementar el parsing y diff para cada formato |
| **Diff visual multi-archivo** | Pendiente | Actualmente solo compara un archivo por invocación |
| **Pin-to-net topology** | Pendiente | El diff semántico compara nets por nombre; conectividad pin-a-pin requiere resolver los `.sym` |
| **Integración con CI** | Pendiente | Modo `--ci` que falle con exit code 1 si hay cambios funcionales, ignorando cosméticos |
| **`riku show <commit> <archivo>`** | Pendiente | Renderizar y abrir el esquemático en un commit específico sin checkout |

---

## Licencia

MIT
