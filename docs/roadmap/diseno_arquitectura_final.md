# Diseño: arquitectura final de riku_chip (monolito modular con microkernel)

Revisión a fondo de cómo se relacionan los crates hoy, qué salió bien, qué quedó mal acoplado, y la arquitectura objetivo para el ejecutable único. Este documento reemplaza la parte de crates de `diseno_ejecutable_unico.md`; el resto de ese diseño (proceso hijo para el visor, dependencias estáticas, `release.yml`) sigue vigente.

**Fecha:** 2026-09-26 · **Base:** `main` `1621fa3`

---

## 1. Cómo está hoy

### 1.1 Los crates y de quién dependen

```
                 riku-gui (bin)                       riku (lib + bin "riku")
                 ┌───────────────────────┐            ┌────────────────────────────────┐
                 │ app.rs: 2 caminos      │            │ core/   domain · git · analysis│
                 │  · .sch → sch_painter  │──usa lib──►│ adapters/ xschem_driver        │
                 │  · resto → ViewerBackend│            │           gds_driver · registry│
                 └──┬─────────┬────────┬──┘            │ cli/    comandos · shell · gui │
                    │         │        │               └──┬───────────┬─────────────────┘
                    │         │        │                  │           │
                    ▼         ▼        ▼                  ▼           ▼
       xschem-viewer-rust   viewer-core   gds-renderer ◄──┘   xschem-viewer-rust
       (submódulo, Carlos)  (contrato)    (diff GDS + GdsBackend + SVG + paletas + cache)
              │  feature viewer-core-compat        │
              └──── path "../../viewer-core"       ▼
                                                gdstk-rs (submódulo, binding cxx) → gdstk C++
```

Cinco crates propios (`viewer-core`, `gds-renderer`, `riku`, `riku-gui`) más dos motores externos como submódulos (`gdstk-rs`, `xschem-viewer-rust`). Sin workspace: cada crate compila solo, con su `Cargo.lock` y su `target/`.

### 1.2 Lo que salió bien

- **`viewer-core` es un contrato de verdad.** `ViewerBackend` + `Scene` neutros (elementos, capas, entradas, cambios) sin saber de GDS ni de Xschem. El diff de GDS entró en la GUI sin tocar `riku-gui` para nada específico de GDS. Es la pieza que hay que conservar y extender.
- **`RikuDriver`** hace lo mismo del lado de la CLI: `diff(bytes_a, bytes_b) -> DriverDiffReport`. El driver de GDS entró sin tocar `log`/`status`.
- **gdstk quedó aislado** detrás de `gds-renderer`: ni `riku` ni `riku-gui` usan `gdstk_rs` (la dependencia declarada en `riku-gui` está sin uso).
- Los motores externos siguen siendo crates útiles por sí solos.

### 1.3 Lo que quedó mal acoplado

| # | Problema | Dónde | Efecto |
|---|---|---|---|
| A | **El dominio del núcleo es el de Xschem.** `ChangeKind`, `DiffReport`, `ComponentDiff`, `Schematic` y `Wire` del núcleo son re-exportaciones de `xschem_viewer::semantic` | `riku/src/core/domain/models.rs` | El "core neutro" depende de un módulo. Un cambio en el crate de Carlos cambia el JSON de `riku status`. El diff de GDS tiene que disfrazarse de `ComponentDiff` |
| B | **Protocolo driver → core por convención de strings.** `element = "net:X"`, `"layout"`, `"cell:INV"`, `"TOP:L1/0:INV"`, `"A → B"`; `before/after` son `BTreeMap<String, String>` | `driver.rs`, `summary/build.rs`, `diff_view.rs`, `diff_text.rs` | Cada consumidor vuelve a parsear strings (`is_gds_geom_element`, `is_net_element`); las áreas viajan como texto `"1.000"` |
| C | **El núcleo conoce los formatos.** `core/format.rs` llama a `gds_renderer::is_layout`; `core/rendering/svg_annotator.rs` es de Xschem; `DiffView` tiene `svg_a/svg_b/sch_a/sch_b` (forma de Xschem) | `riku/src/core/` | El núcleo no compila sin los dos módulos; agregar Magic o KiCad obliga a tocar el core |
| D | **La CLI decide por extensión y tiene dos caminos.** `is_gds_path` → `run_diff_gds`, con un `DiffView` "placeholder" para reutilizar los printers | `cli/commands.rs` | El tercer formato agrega un tercer camino |
| E | **La GUI tiene dos caminos.** `.sch` → `sch_painter` + `XschemDriver` + `DiffView` (fantasmas, anotaciones); el resto → `ViewerBackend`. Decide por extensión (`self.sch`) en 25 sitios de `app.rs` | `riku-gui/src/app.rs` | Es la causa del pendiente #1 (paridad `.sch`): tooltip, etiquetas, animación y atajos existen solo en un camino |
| F | **El adaptador de viewer-core vive en el crate de Carlos** con `path = "../../viewer-core"` | `external/xschem-viewer-rust/Cargo.toml` | El submódulo depende de la carpeta donde se lo clone. `XschemBackend` existe pero la GUI no lo usa para `.sch` (no lleva fantasmas ni anotaciones) |
| G | **`gds-renderer` hace cinco cosas:** diff geométrico, backend del visor, render SVG, paletas y cache | `gds-renderer/src/` | El nombre ya no describe al crate; el render SVG solo lo usa la ruta `visual` heredada |
| H | **Registro de módulos duplicado y a mano.** La CLI lista drivers en `adapters/registry.rs`; la GUI lista backends en `app.rs` | dos listas | Un formato nuevo se registra dos veces; nada garantiza que el driver y el backend sean del mismo módulo |
| I | Dos binarios con las mismas librerías adentro; `riku` tiene que *buscar* `riku-gui` | `cli/gui.rs` | Ver `diseno_ejecutable_unico.md` |

**Diagnóstico:** la idea "un core y módulos acoplados como librerías" se cumplió a medias. Los módulos sí son librerías, pero el core mira hacia ellos (A, C) y los shells (CLI, GUI) los conocen por nombre (D, E, H). Está invertida la dirección de la dependencia en tres lugares. Eso es lo que hay que dar vuelta; no hace falta reescribir nada de lo que funciona (diff, render, paletas, motion de la GUI).

---

## 2. Arquitectura objetivo

**Estilo:** monolito modular (un ejecutable, varios crates) con **microkernel**: un núcleo pequeño que solo define contratos y orquesta Git, y módulos de formato que se registran en él. Los módulos se enlazan en tiempo de compilación (features de Cargo), no como `.so`: en Rust es lo estable, lo rápido y lo que permite un solo archivo.

```
                              ┌──────────────────────────────────────────┐
   Shells (solo hablan        │   riku  (bin)  main → cli | gui          │
   con el kernel)             │   cli/  comandos · shell · formatters    │
                              │   gui/  ventana egui  [feature "gui"]    │
                              └───────────────┬──────────────────────────┘
                                              │ Kernel API
                              ┌───────────────▼──────────────────────────┐
   Kernel                     │   riku-kernel                            │
   (sin conocer formatos)     │   domain: FileChange, Element, Change,   │
                              │           Summary  (tipos propios)       │
                              │   git: GitService (blobs, log, status)   │
                              │   analysis: diff de commits, log, status │
                              │   ports: trait FormatModule, Registry    │
                              │   view: re-exporta viewer-core           │
                              └───────────────▲──────────────────────────┘
                                              │ implementan FormatModule
                 ┌────────────────────────────┼─────────────────────────┐
                 │                            │                         │
   Módulos       ▼                            ▼                         ▼
   ┌────────────────────────┐   ┌─────────────────────────┐   ┌──────────────────────┐
   │ riku-mod-xschem         │   │ riku-mod-layout          │   │ (futuro) riku-mod-   │
   │ diff semántico          │   │ diff geométrico GDS/OASIS│   │ magic · kicad · raw  │
   │ XschemBackend + overlays│   │ GdsBackend · paletas     │   └──────────────────────┘
   │ (fantasmas, anotaciones)│   │ cache · verificación     │
   └───────────┬────────────┘   └───────────┬─────────────┘
               │ usa                        │ usa
               ▼                            ▼
   Motores     xschem-viewer-rust           gdstk-rs → gdstk C++
   (submódulos, │ parser · semántica · render │ lectura GDS/OASIS · booleanas
   sin saber    │ (SIN viewer-core-compat)     │
   de riku)     └──────────────────────────────┘
```

**Reglas de dependencia (se verifican en la CI con `cargo tree`):**
1. `riku-kernel` no depende de ningún módulo ni motor. Solo de `viewer-core`, `git2`, `serde`.
2. Un módulo depende del kernel, de `viewer-core` y de su motor. Nunca de otro módulo ni de los shells.
3. Los motores externos no dependen de nada de riku. El adaptador va del lado del módulo.
4. Los shells (`cli/`, `gui/`) dependen solo del kernel. La lista de módulos existe en **un** lugar: `riku/src/modules.rs`.

### 2.1 El contrato de módulo (`riku-kernel::ports`)

```rust
/// Un formato de archivo de diseño, con todo lo que Riku sabe hacer con él.
pub trait FormatModule: Send + Sync {
    fn info(&self) -> ModuleInfo;                       // nombre, versión, extensiones
    fn detect(&self, bytes: &[u8], path: Option<&str>) -> bool;   // por firma o extensión
    fn diff(&self, a: Option<&[u8]>, b: &[u8], cfg: &DiffConfig) -> FileChange;
    fn viewer(&self) -> Option<Arc<dyn ViewerBackend>>; // None = formato sin visor
}

pub struct FileChange {                  // reemplaza a DriverDiffReport + DiffReport
    pub module: String,
    pub changes: Vec<Change>,
    pub warnings: Vec<String>,
}

pub struct Change {                      // reemplaza a DiffEntry + ComponentDiff
    pub kind: ChangeKind,                // Added · Removed · Modified · Renamed{from} · Moved
    pub element: Element,                // tipado, sin prefijos en strings
    pub severity: Severity,              // Functional · Cosmetic
    pub location: Option<Location>,      // bbox en unidades del módulo, para "ir al cambio"
    pub details: Vec<Detail>,            // (clave, antes, después) con valores tipados
}

pub enum Element {
    Component { name: String, symbol: Option<String> },
    Net(String),
    Wire,
    Cell(String),
    Geometry { cell: String, layer: LayerRef, via: Option<Instance> },
    Whole,                               // "todo el archivo" (era el marcador "layout")
}
```

- Los tipos del kernel son **propios**: el JSON de `riku status`/`log`/`diff` sale de ellos, con schema versionado (`riku-diff/v2`), y ya no cambia si cambia el crate de Carlos.
- `Element` tipado elimina las convenciones `"net:"`, `"cell:"`, `"TOP:L1/0:INV"` y los `is_*_element`. Los formatters de texto hacen `match`.
- `viewer()` une lo que hoy son dos listas (drivers y backends): un módulo trae ambos, o solo el diff.

### 2.2 Cómo entra cada módulo

**`riku-mod-xschem`** (nuevo crate, ~el `xschem_driver.rs` de hoy + `viewer_core_adapter.rs` + `sch_painter.rs` reescrito como overlays):
- `diff`: llama a `xschem_viewer::semantic::diff` y **traduce** `ComponentDiff` → `Change`. La traducción es el único lugar que conoce los tipos de Carlos.
- `viewer`: `XschemBackend` (se muda desde el submódulo; con eso desaparecen la feature `viewer-core-compat` y el `path = "../../viewer-core"`). Los **fantasmas** del commit A y las **anotaciones por componente** pasan a ser parte de `Scene` (ver 2.3), así que la GUI los dibuja con el mismo código que dibuja el diff de GDS.

**`riku-mod-layout`** (es `gds-renderer` renombrado, menos lo que no es del módulo):
- `diff`: `diff_gds_cached` → `Change` con `Element::Geometry` (áreas como `f64`, `Instance` tipada, renombres como `ChangeKind::Renamed`).
- `viewer`: `GdsBackend` tal cual.
- Se queda con paletas y cache. El **render SVG** (`renderer.rs`, `composition.rs`, `output.rs`, `style.rs`: ~600 líneas) sale a un crate `gds-svg` o se borra si nadie lo usa: la CLI ya abre la GUI para `-f visual`.

**Motores:** `xschem-viewer-rust` vuelve a ser un crate sin dependencias hacia riku (se le quita la feature y el path). `gdstk-rs` no cambia.

### 2.3 Extensiones a `viewer-core` (una sola ruta de dibujo en la GUI)

Para que `.sch` deje de necesitar su propio painter, `Scene` gana lo que hoy solo tiene el camino rico:

```rust
pub struct Scene {
    …
    pub ghost: Option<Vec<DrawElement>>,      // versión "antes", atenuada (fantasmas)
    pub annotations: Vec<Annotation>,         // recuadros/etiquetas sobre elementos cambiados
}
pub struct Annotation { pub bbox: BoundingBox, pub kind: ChangeKind, pub label: String }
```

Con esto `app.rs` pierde `self.sch` y sus 25 ramas: **todo archivo entra por `FormatModule::viewer()`**, y tooltip, etiquetas, animación, atajos y diff visual funcionan igual para `.sch` y `.gds`. Esto cierra el pendiente #1 (paridad `.sch`) como consecuencia del diseño, no como trabajo aparte.

### 2.4 Registro de módulos y el binario

```rust
// riku/src/modules.rs — el ÚNICO lugar que sabe qué módulos existen
pub fn registry() -> Registry {
    let mut r = Registry::new();
    #[cfg(feature = "xschem")] r.add(Arc::new(riku_mod_xschem::Module::new()));
    #[cfg(feature = "layout")] r.add(Arc::new(riku_mod_layout::Module::new()));
    r
}
```

```toml
# riku/Cargo.toml
[features]
default = ["xschem", "layout", "gui"]
xschem = ["dep:riku-mod-xschem"]
layout = ["dep:riku-mod-layout"]
gui    = ["dep:eframe", "dep:tokio", "dep:poll-promise", "dep:earcutr"]
```

- `cargo build` → el `riku` completo. `--no-default-features --features layout` → un `riku` solo de layouts y sin ventana (servidor de CI).
- El kernel resuelve el módulo por `detect()` (firma del archivo) y después por extensión; la CLI y la GUI dejan de decidir por `.gds`/`.sch`.
- `riku doctor` lista los módulos compilados y sus versiones (commit de cada submódulo, inyectado en el build).
- La CLI y la GUI van dentro de `riku` como shells (`cli/`, `gui/`), como en `diseno_ejecutable_unico.md` §3.1 opción B; el lanzamiento del visor (`riku gui`, proceso hijo) y el empaquetado no cambian.

### 2.5 Layout final del repositorio

```
riku_chip/
├── Cargo.toml                 workspace: kernel, viewer-core, mod-*, riku
├── viewer-core/               contrato del visor (sin cambios de nombre)
├── riku-kernel/               dominio · git · análisis · FormatModule · Registry
├── riku-mod-xschem/           módulo Xschem (diff + backend + overlays)
├── riku-mod-layout/           módulo GDS/OASIS (hoy gds-renderer)
├── riku/                      binario: modules.rs · cli/ · gui/
├── external/gdstk/            motor (submódulo)
├── external/xschem-viewer-rust/  motor (submódulo, sin código de riku)
├── tools/verify · tools/palettes
└── docs/
```

Cuatro crates propios en lugar de cinco, con una dirección de dependencia sola: `riku → mod-* → kernel → viewer-core`.

---

## 3. Migración (sin parar de funcionar)

Cada fase deja `main` verde y con los mismos tests o más. El orden va de lo que más duele a lo que menos, y cada fase habilita la siguiente.

| Fase | Qué | Cierra | Esfuerzo |
|---|---|---|---|
| 0 | Ejecutable único + workspace (`diseno_ejecutable_unico.md`, opción B): `riku-gui/src` → `riku/src/gui`, subcomando `gui`, `.tar.gz`/`.deb` | I | M |
| 1 | **Tipos propios en el kernel.** Crear `riku-kernel` con `Change`/`Element`/`FileChange`; `riku-mod-*` los producen; formatters y JSON (`v2`) los consumen. `models.rs` deja de re-exportar Xschem | A, B | L |
| 2 | **Sacar formatos del kernel.** `detect()` en cada módulo; `format.rs` y `svg_annotator.rs` fuera del kernel; `DiffView` neutro; la CLI sin `is_gds_path` | C, D | M |
| 3 | **Registro único.** `Registry` con `FormatModule::viewer()`; `modules.rs`; features por módulo | H | S |
| 4 | **`Scene` con `ghost` y `annotations`; `XschemBackend` al módulo.** La GUI usa una sola ruta; `sch_painter.rs` se reduce a producir overlays; se quita la feature del submódulo | E, F, pendiente #1 | L |
| 5 | Renombrar `gds-renderer` → `riku-mod-layout`; borrar el render SVG (nunca se conectó a un comando; la exportación futura sale de la escena neutra, ver pendientes #11) | G | S |

**Qué NO cambia:** los algoritmos (diff semántico de Carlos, XOR de gdstk, paletas, etiquetas, motion), `viewer-core` (solo gana campos), la CLI para el usuario (mismos comandos; el JSON cambia de schema una sola vez, en la Fase 1, y se documenta).

**Riesgos:**
- La Fase 1 toca el JSON que consumen scripts: se publica `v2` y se mantiene `--format json-v1` durante una versión.
- La Fase 4 es la más grande de GUI: se hace con capturas antes/después (`tools/verify/gui`) y los 43 tests actuales.
- El submódulo de Carlos pierde la feature `viewer-core-compat`: se coordina con él; su crate queda más simple, no más complejo.

---

## 4. Por qué no otras opciones

- **Plugins dinámicos (`.so` cargados en runtime):** Rust no tiene ABI estable; habría que pasar por C o `abi_stable`, perder tipos ricos como `Scene`, y distribuir varios archivos. Contradice el ejecutable único. Los módulos como features de Cargo dan el mismo desacople sin ese costo.
- **Un solo crate con módulos internos:** compila más lento (todo o nada) y no impide que el kernel importe un módulo por descuido. Los crates separados hacen que la regla de dependencia la verifique Cargo.
- **Dejar la GUI con dos caminos:** es la fuente del pendiente #1 y del `self.sch` en 25 sitios. Cada mejora de la GUI hay que hacerla dos veces.

---

## Avance

| Fase | Estado | Notas |
|---|---|---|
| 0 | Hecha (2026-09-26) | Ejecutable único `riku` (visor en `riku/src/gui`), workspace, git2 sin OpenSSL, `GDSTK_STATIC`, `release.yml` (`.tar.gz` 5,6 MB y `.deb` 4,1 MB, probados en Ubuntu 22.04 limpio) |
| 1 | Hecha (2026-09-26) | Crate `riku-kernel` con `FileChange`/`Change`/`Element`/`ChangeKind` (con `Renamed`)/`Detail`/`Value`. Los drivers traducen a esos tipos (Xschem: `component_change`; layouts: `geom_change`); el núcleo ya no re-exporta `xschem_viewer::semantic`. `riku diff -f json` sale en `riku-diff/v2`; `-f json-v1` y el `full_report` de `status`/`log` se generan con `riku_kernel::legacy` y son **idénticos byte a byte** a la salida anterior (verificado contra el binario de la fase 0 en `.sch` con renombre, valor cambiado, traslado y net nueva, y en `.gds` con renombre e instancias). Se borró `core/rendering` (anotador SVG sin uso). Mejora colateral: el visor encuentra el componente renombrado para anotarlo. Diferencias con el plan: `cosmetic` sigue siendo `bool` (no hizo falta `Severity`), y `git`/`analysis` siguen en `riku` hasta la fase 2 |
