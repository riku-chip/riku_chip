# Especificación: `riku status` y `riku log`

> Documento de diseño previo a implementación. Sujeto a iteración con feedback de usuarios.
> Versión: borrador 1 — 2026-04-25

---

## 1. Objetivo y no-objetivos

### Objetivo

Dar al usuario de Riku visibilidad **semántica** sobre el estado de su repositorio Git de diseño de chips: qué cambió en términos del dominio (transistores, nets, capas), no en términos de bytes.

### No-objetivos (explícitos)

- **No** reemplazar `git status` ni `git log`. Riku los complementa, no los sustituye.
- **No** modificar el repo (sin `add`, `commit`, `merge`). Estos comandos son de solo lectura.
- **No** interpretar todos los formatos del repo. Lo que Riku no entiende, lo ignora silenciosamente.
- **No** introducir configuración obligatoria. Funciona en cualquier repo Git al que se le apunte.

---

## 2. Audiencia y casos de uso

Riku es open source con audiencia heterogénea. El diseño debe servir a:

| Caso | Necesidad principal |
|---|---|
| Diseñador solitario experimentando con ramas | "¿qué cambié desde el último commit?" |
| Equipo pequeño (2-5) en bloques distintos | "¿qué tocó mi compañero esta semana?" |
| Contribuyente a PDK open source | "¿este PR cambia el comportamiento o solo cosmética?" |
| Mantenedor revisando PRs | log legible por archivo, exportable a JSON |

El denominador común: **separar cambios cosméticos de cambios funcionales**, y hacerlo legible tanto para humanos como para scripts.

---

## 3. Información semántica que vale la pena mostrar

Esta es la pregunta más importante. Mostrar todo es ruido. Mostrar poco es inútil.

### Criterios de inclusión

Un cambio merece reportarse si cumple **al menos uno** de:

1. **Afecta la netlist**: añade, elimina o reconecta componentes/nets.
2. **Afecta parámetros eléctricos**: cambio de W, L, valor de R/C, modelo SPICE.
3. **Afecta la jerarquía**: instanciación o eliminación de subcircuitos.
4. **Afecta la interfaz**: pins añadidos/eliminados/renombrados.

### Criterios de exclusión (cosmético)

Un cambio **no se reporta como semántico** si solo afecta:

- Posición visual de un símbolo (sin cambiar conexiones).
- Reordenamiento de líneas en el archivo.
- Comentarios, espacios en blanco.
- Color de capa, grosor de línea (futuro `.gds`).

### Niveles de detalle

Para mantener output usable en distintos contextos, se definen tres niveles:

| Nivel | Flag | Uso típico |
|---|---|---|
| **Resumen** | (default) | "amp_ota.sch: +2 transistores, -1 net" |
| **Detalle** | `--detail` | "amp_ota.sch: +M5 (nmos, W=4u L=180n), +M6 (...), -net vbias_int" |
| **Completo** | `--full` | Reporte por elemento con before/after de cada parámetro |

El nivel `--full` reusa el `DriverDiffReport` ya existente. Resumen y Detalle son agregaciones del mismo report.

### Tabla de eventos semánticos por formato

Cada driver decide qué eventos reportar. Tabla de referencia para `.sch`:

| Evento | Resumen | Detalle |
|---|---|---|
| Componente añadido | `+1 transistor` | `+M5 (nmos)` |
| Componente eliminado | `-1 transistor` | `-M3 (nmos)` |
| Parámetro cambiado | `1 cambio param` | `M3.W: 4u → 8u` |
| Net añadida | `+1 net` | `+net vbias_int` |
| Net renombrada | `1 net renombrada` | `vin → vin_diff` |
| Pin añadido al símbolo | `+1 pin` | `+pin VDD` |
| Reposicionamiento | (silencioso) | (silencioso) |

Para `.gds` (cuando esté disponible):

| Evento | Resumen | Detalle |
|---|---|---|
| Polígonos añadidos en capa | `+50 polígonos en capa M1` | (lista de bboxes) |
| Polígonos eliminados | `-12 polígonos en capa POLY` | (lista de bboxes) |
| Capa nueva referenciada | `+1 capa: VIA2` | |
| Bbox total cambiada | `bbox: 100×80 → 120×80` | |
| Reposicionamiento sin cambio de geometría | (silencioso) | (silencioso) |

---

## 4. `riku status` — especificación

### Sinopsis

```
riku status [--repo <PATH>] [--detail] [--full] [--json] [--color=<auto|always|never>]
            [--include-unknown] [--paths <pat>...]
```

### Comportamiento

1. Identifica la rama actual y el commit HEAD.
2. Para cada archivo en el working tree con cambios respecto a HEAD:
   - Si Riku tiene driver para él → ejecuta diff semántico contra HEAD y clasifica.
   - Si no → omite del reporte (a menos que `--include-unknown`).
3. Reporta por categorías: `Modificados (semánticos)`, `Modificados (cosméticos)`, `Sin cambios semánticos`, `No reconocidos`.

### Salida — formato texto (default)

```
En rama feature-amp (3 commits adelante de main)
HEAD: 4f2a1c — "ajustar bias del OTA" (hace 2 horas)

Modificados con cambios semánticos:
  amp_ota.sch         +2 transistores, -1 net, 3 nets renombradas
  filtro.sch          +1 capacitor

Modificados sin cambios semánticos:
  layout_top.sch      (solo reposicionamiento)

No reconocidos por Riku (3):  use --include-unknown para listarlos
```

### Salida — formato JSON

```json
{
  "branch": "feature-amp",
  "head": { "oid": "4f2a1c...", "short": "4f2a1c", "subject": "ajustar bias del OTA", "timestamp": 1714060800 },
  "ahead_of": { "ref": "main", "commits": 3 },
  "files": [
    {
      "path": "amp_ota.sch",
      "category": "semantic",
      "summary": { "components_added": 2, "nets_removed": 1, "nets_renamed": 3 },
      "format": "xschem"
    },
    { "path": "layout_top.sch", "category": "cosmetic", "format": "xschem" }
  ],
  "unrecognized": ["Makefile", "README.md", "scripts/run.sh"]
}
```

### Flags

| Flag | Efecto |
|---|---|
| `--repo PATH` | Repo a inspeccionar (default `.`). Reusa `GitService::open`. |
| `--detail` | Eleva el nivel de cada entrada al modo Detalle. |
| `--full` | Imprime `DriverDiffReport` completo por archivo. |
| `--json` | Salida JSON estable (ver §6). |
| `--color=auto\|always\|never` | Color en stderr/stdout. `auto` detecta TTY. |
| `--include-unknown` | Lista archivos sin driver. |
| `--paths PAT...` | Filtra por glob (`amp_*.sch`). |

### Códigos de salida

- `0`: sin cambios semánticos
- `1`: hay cambios semánticos
- `2`: error (repo no encontrado, driver crash, etc.)

Esto permite scripts: `riku status -q && echo "limpio"`.

---

## 5. `riku log` — especificación

### Sinopsis

```
riku log [<paths>...] [--repo <PATH>] [-n <N>] [--since <DATE>] [--branch <REF>]
         [--detail] [--full] [--graph] [--json] [--color=<auto|always|never>]
```

### Comportamiento

1. Resuelve el rango de commits (default: HEAD, `-n 20` últimos).
2. Para cada commit:
   - Computa `parent..commit` para los archivos que tocó.
   - Para cada archivo con driver disponible: ejecuta diff semántico.
   - Anota el commit con resumen agregado.
3. Imprime en orden cronológico inverso.

### Salida — formato texto (default)

```
* 4f2a1c  feature-amp ← HEAD   ajustar bias del OTA
          carlos · hace 2 horas
          amp_ota.sch  +2 transistores, -1 net

* 9b3e2a                       aumentar ganancia
          carlos · hace 1 día
          amp_ota.sch  param: M3.W 4u → 8u

* 1a8f7c  main                 merge filtro pasa-bajos
          maria · hace 3 días
          (merge commit, sin diff semántico directo)
```

### Salida — formato gráfico (`--graph`)

Reusa el grafo que pinta `git2`/`gix`. Riku no reinventa, solo añade anotaciones:

```
* 4f2a1c  feature-amp ← HEAD   ajustar bias del OTA
|         amp_ota.sch  +2 transistores
* 9b3e2a                       aumentar ganancia
|         amp_ota.sch  M3.W 4u → 8u
| * 7c2d1b  main                hotfix DRC
|/          amp_ota.sch  reposicionamiento (sin cambio semántico)
* 1a8f7c                        ...
```

### Salida — JSON

```json
{
  "commits": [
    {
      "oid": "4f2a1c...",
      "short": "4f2a1c",
      "subject": "ajustar bias del OTA",
      "author": "carlos",
      "timestamp": 1714060800,
      "refs": ["feature-amp", "HEAD"],
      "parents": ["9b3e2a..."],
      "files": [
        {
          "path": "amp_ota.sch",
          "summary": { "components_added": 2, "nets_removed": 1 }
        }
      ]
    }
  ]
}
```

### Flags

| Flag | Efecto |
|---|---|
| `<paths>...` | Filtra commits que tocan esos archivos (positional). |
| `-n N` | Límite de commits (default 20). Reusa el ya existente. |
| `--since DATE` | Pasa al revwalk (`--since="2 weeks ago"`). |
| `--branch REF` | Empieza desde otra ref distinta de HEAD. |
| `--detail` / `--full` | Como en `status`. |
| `--graph` | Imprime con grafo ASCII. |
| `--json` | Salida JSON estable. |

### Anotación de refs

Cada commit muestra qué refs apuntan a él (rama actual, otras ramas locales, tags). Para responder la pregunta original del usuario: **"¿qué commit pertenece a qué rama?"**

Implementación: al cargar el log, recolectar `repo.references()` y mapear `oid → [refs]`. Mostrar como `branch1, branch2 ← HEAD` junto al short OID.

---

## 6. Estabilidad del JSON

El formato JSON es contrato público desde la primera versión.

- Versión del schema en cada salida: `"schema": "riku-status/v1"` o `"riku-log/v1"`.
- Cambios incompatibles bumpan la versión.
- Cambios compatibles (campos nuevos opcionales) no la bumpan.

Esto permite que CI/CD, otras tools y forks dependan de la salida sin riesgo de ruptura silenciosa.

---

## 7. Arquitectura propuesta

### Principio rector

**Reusar lo que ya existe, no duplicar.** La arquitectura actual de Riku ya tiene:

- `GitService` (port `GitRepository`) con `git2` — devuelve commits, blobs, deltas.
- `RikuDriver` con `diff()` que produce `DriverDiffReport`.
- Registry `get_driver_for(path)` para resolver formato → driver.
- `OutputFormat::{Text, Json, Visual}` ya en CLI.

Lo nuevo se construye **encima**, sin tocar lo de abajo.

### Módulos nuevos

```
riku/src/core/
  ├─ status.rs       (nuevo) — orquesta status semántico
  ├─ log.rs          (nuevo) — orquesta log semántico con anotaciones
  ├─ summary.rs      (nuevo) — agregación: DriverDiffReport → Summary
  ├─ refs.rs         (nuevo) — mapeo oid → [ref names]
  └─ git_service.rs  (extender, no romper)

riku/src/cli/
  ├─ commands.rs     (extender: run_status, mejorar run_log)
  └─ format/         (nuevo, opcional)
       ├─ mod.rs
       ├─ text.rs    — formateo de texto humano
       └─ json.rs    — formateo JSON con schema versionado
```

### Diagrama de capas

```
┌──────────────────────────────────────────────────┐
│                    CLI (clap)                     │
│           run_status / run_log / run_diff         │
└────────────┬─────────────────────────────────────┘
             │ usa
┌────────────▼─────────────────────────────────────┐
│           Orquestadores (core)                    │
│   status::analyze()    log::walk_with_summary()   │
└──┬─────────────────────────────┬─────────────────┘
   │                             │
   ▼                             ▼
┌──────────────────┐   ┌─────────────────────────┐
│  GitRepository   │   │   RikuDriver (existente)│
│   (port)         │   │   .diff() → Report      │
│                  │   │                         │
│  + working_tree_ │   │   summary.rs agrega    │
│    changes()     │   │   Report → Summary      │
│  + refs_at_oid() │   │                         │
└──────────────────┘   └─────────────────────────┘
   │
   ▼
┌──────────────────┐
│  GitService      │
│  (adapter git2)  │
└──────────────────┘
```

### Extensiones al port `GitRepository`

Para no romper lo existente, se **añaden** métodos:

```rust
pub trait GitRepository {
    // existentes...
    fn get_blob(&self, commit_ish: &str, file_path: &str) -> Result<Vec<u8>, GitError>;
    fn get_commits(&self, file_path: Option<&str>) -> Result<Vec<CommitInfo>, GitError>;
    fn get_changed_files(&self, a: &str, b: &str) -> Result<Vec<ChangedFile>, GitError>;

    // nuevos (default impl posible para no forzar)
    fn working_tree_changes(&self) -> Result<Vec<WorkingChange>, GitError>;
    fn current_branch(&self) -> Result<Option<BranchInfo>, GitError>;
    fn refs_for_oid(&self, oid: &str) -> Result<Vec<String>, GitError>;
    fn commit_with_parents(&self, oid: &str) -> Result<CommitWithParents, GitError>;
}
```

### Modelo de `Summary`

`DriverDiffReport` ya tiene todo lo necesario. `Summary` es una **vista derivada**:

```rust
#[derive(Serialize)]
pub struct FileSummary {
    pub path: String,
    pub format: FileFormat,
    pub category: SummaryCategory,
    pub counts: BTreeMap<String, i64>,
}

pub enum SummaryCategory {
    Semantic,
    Cosmetic,
    Unchanged,
    Unknown,
    Error,
}

impl FileSummary {
    pub fn from_report(report: &DriverDiffReport, path: &str) -> Self { /* agregar */ }
}
```

`counts` es un mapa flexible (`"components_added": 2, "nets_renamed": 3`) — cada driver decide qué claves usa. El formateador de texto las traduce; el JSON las preserva.

**Por qué `BTreeMap` y no struct fijo**: cada formato (sch, gds, spice) tiene eventos distintos. Un struct con campos fijos para todos crece sin control. El mapa permite que cada driver reporte lo suyo y la UI se adapte.

### Manejo de errores

Reutilizar `RikuError` y `GitError` existentes. Para `status` y `log`:

- Driver crash en un archivo → marcar como `SummaryCategory::Error`, continuar con el resto.
- Blob demasiado grande → ya está manejado en analyzer, igual aquí.
- Repo corrupto → fallar duro al inicio (exit code 2).

**Nunca** abortar todo el log porque un commit tiene un archivo problemático.

### Rendimiento

- `riku log -n 20` con archivos `.sch` típicos: presupuesto **<1s** en repo mediano.
- `riku status` en working tree: **<300ms**.

Si un commit no tiene archivos reconocibles, no se invoca driver alguno (early skip por extensión vía `registry::get_driver_for`).

**Cache**: no en versión 1. Si después emerge un caso real (`riku log -n 1000`), se añade en `.git/riku-cache/` con clave `(oid, file_path) → Summary`.

### Concurrencia

Por commit, los archivos son independientes — paralelizables con `rayon` si hace falta. Versión 1: secuencial. Optimización futura.

---

## 8. Compatibilidad con la arquitectura existente

### Lo que **no se toca**

- `RikuDriver` trait, `XschemDriver`, registry.
- `commit_diff::analyze_diff` — sigue siendo el camino de `riku diff`.
- `GitService::get_blob/get_commits/get_changed_files`.
- `OutputFormat` enum.
- Shell REPL — los nuevos comandos heredan parser y se exponen automáticamente.

### Lo que **se extiende**

- Trait `GitRepository`: añadir métodos (default impl donde sea posible para no romper tests existentes).
- `Commands` enum del CLI: añadir variante `Status`, mejorar variante `Log`.

### Lo que **se añade**

- Módulos `core::{status, log, summary, refs}`.
- Módulo `cli::format` (opcional, podría vivir inline en `commands.rs` si queda pequeño).

---

## 9. Fases de implementación

Cada fase entrega valor por sí sola. No hay dependencias hacia atrás.

### Fase 1 — Esqueleto y `riku status` básico

- Extender `GitRepository` con `working_tree_changes`, `current_branch`.
- Implementar `status::analyze()` reusando `XschemDriver::diff`.
- Añadir variante `Commands::Status` y `run_status`.
- Salida texto solo.

**Criterio de éxito**: en un repo con `.sch` modificado, `riku status` reporta cambios semánticos vs cosméticos.

### Fase 2 — JSON estable + flags

- Implementar formateador JSON con `schema: riku-status/v1`.
- Flags `--detail`, `--full`, `--paths`.
- Tests de snapshot (insta) sobre la salida JSON.

**Criterio de éxito**: la salida JSON es parseable, estable, documentada.

### Fase 3 — `riku log` con anotaciones semánticas

- Extender `GitRepository` con `refs_for_oid`, `commit_with_parents`.
- Implementar `log::walk_with_summary()`.
- Mejorar `Commands::Log`: añadir `--detail`, `--full`, `--graph`, `--json`, `<paths>` posicionales.
- Anotación de refs.

**Criterio de éxito**: `riku log` muestra commits con resumen semántico y refs.

### Fase 4 — Refinamiento

- Color en TTY, deshabilitable.
- `--graph` ASCII (pendiente decidir librería: `git2` o reimplementar).
- Documentación en `riku/docs/cli-reference.md`.
- Recomendación de `__git_ps1` en README para prompt de shell.

### Fase 5 — Multi-formato (depende de drivers)

- Cuando exista `GdsDriver`, `riku status`/`log` lo usa automáticamente vía registry.
- No requiere cambios en `status`/`log` — la arquitectura ya está abierta.

---

## 10. Decisiones abiertas

Estas se resuelven con feedback de uso, no en el papel:

1. **¿`--graph` reusa el grafo de `git log --graph` por subprocess o se reimplementa?** Recomendación inicial: subprocess de `git log --graph --oneline` y anotar líneas. Trade-off: simple pero acoplado a salida de Git. Decisión final: probar primero, refactorizar si duele.

2. **¿`status` también reporta archivos staged distintos a working tree?** Versión 1: solo working tree vs HEAD. Si emerge necesidad, añadir `--staged`.

3. **¿`log` debe seguir merges por defecto?** Recomendación: sí (como `git log` default). Flag `--first-parent` para suprimir.

4. **¿Cuál es el límite default de `riku log`?** Hoy es 20. Mantener.

---

## 11. Lo que queda explícitamente fuera

- `riku merge`, `riku branch`, `riku commit` — no son alcance.
- TUI interactiva — no en versión 1.
- Cache persistente — no en versión 1.
- Configuración via `.rikurc` — no.
- Análisis de Verilog/VHDL — no en versión 1.

---

## 12. Glosario rápido

- **Cambio semántico**: cambio que afecta el comportamiento eléctrico/funcional del diseño.
- **Cambio cosmético**: cambio sin efecto en simulación o fabricación (posición, color).
- **Driver**: implementación de `RikuDriver` para un formato (Xschem, GDS, ...).
- **Summary**: vista agregada de un `DriverDiffReport`, optimizada para listas.
- **Working tree**: archivos en disco, lo que el editor está modificando ahora.
- **HEAD**: el commit en el que el working tree se basa.
