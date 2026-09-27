# Arquitectura

**Monolito modular con microkernel:** un solo ejecutable, varios crates. Un núcleo que no conoce ningún formato define los contratos; cada formato es un módulo que se registra en él. Los módulos se enlazan al compilar (features de Cargo), no como `.so`: en Rust es lo estable y permite distribuir un solo archivo.

## Crates

```
riku_chip/
├── riku/                 ejecutable: main → cli | gui
│   ├── src/core/         git (git2) y análisis: diff entre commits, show, log, status
│   ├── src/modules/      módulos de formato; mod.rs::registry() es el ÚNICO lugar que los lista
│   │   ├── xschem.rs, xschem_view.rs, xschem_pdk.rs   (feature `xschem`)
│   │   ├── layout.rs                                  (feature `layout`)
│   │   └── spice/        raw.rs, compare.rs           (feature `spice`)
│   ├── src/cli/          comandos, shell, formatos de salida
│   └── src/gui/          visor egui (feature `gui`)
├── riku-kernel/          tipos neutros (FileChange, Change, Element, ChangeKind, Detail…),
│                         trait FormatModule, Registry; legacy.rs = JSON v1 exacto
├── riku-mod-layout/      módulo GDS/OASIS/Magic: diff geométrico, cache, estilo por PDK, GdsBackend;
│                         mag.rs = de dónde salen las sub-celdas de un .mag (commit, PDK)
├── viewer-core/          contrato del visor: ViewerBackend, Scene, DrawElement, SceneIndex
├── external/gdstk/                 motor: gdstk-rs, binding de gdstk (C++) y lector de Magic (Rust)   [submódulo]
└── external/xschem-viewer-rust/    motor: parser y semántica de Xschem      [submódulo]
```

```
            riku (cli · gui)
                 │ Registry: detectar, diff, visor
     ┌───────────┼──────────────────┬───────────────┐
     ▼           ▼                  ▼               ▼
 modules/xschem  riku-mod-layout   modules/spice   (futuro: KiCad…)
     │           │  (.gds .oas .mag)
     ▼           ▼
 xschem-viewer   gdstk-rs → gdstk C++ · magic (Rust)
            │    │
            ▼    ▼
   riku-kernel · viewer-core   (no conocen ningún formato)
```

## Contratos

```rust
// riku-kernel
pub trait FormatModule: Send + Sync {
    fn info(&self) -> ModuleInfo;                         // nombre, versión, extensiones (para `doctor`)
    fn extensions(&self) -> &'static [&'static str];       // `["gds", "oas"]`: lo que consulta `for_path`
    fn detect(&self, content: &[u8]) -> bool;              // por firma del archivo
    fn diff(&self, a: &[u8], b: &[u8], path: &str, opts: &DiffOptions) -> FileChange;
    // Con los otros archivos de cada versión; por defecto llama a `diff`.
    fn diff_with(&self, a: &[u8], b: &[u8], path: &str, opts: &DiffOptions, files: &DiffFiles) -> FileChange;
    fn viewer(&self) -> Option<Arc<dyn ViewerBackend>>;   // None = sin visor
}

// viewer-core
pub trait FileSource: Send + Sync {                       // otros archivos de una versión
    fn read(&self, path: &str) -> Option<Vec<u8>>;        // ruta relativa a la raíz del repo
}
pub struct DiffFiles { pub before: Option<Arc<dyn FileSource>>, pub after: Option<Arc<dyn FileSource>> }
```

- **Formatos de varios archivos** (Magic: una celda por archivo): el núcleo le pasa al módulo un `FileSource` por versión. `diff`, `show` y `log` usan `GitFiles` (el mismo commit; abre su conexión a Git la primera vez que se le pide un archivo); `status`, HEAD antes y el disco después (`DiskFiles`). El módulo decide qué leer; el núcleo no sabe de `use` ni de celdas.

- `FileChange` tiene `Change`s tipados: `kind` (añadido, eliminado, modificado, renombrado), `element` (`Component`, `Net`, `Whole`, `Cell`, `Geometry` con `layer_name` opcional, `Port`, `Signal`), `cosmetic`, `location` (para "ir al cambio") y `details` con valores antes/después. De ahí salen el texto y el JSON (`riku-diff/v2`); `legacy.rs` reproduce el JSON v1 byte a byte.
- Un `Detail` que es la **ubicación** del elemento en el dibujo (en Xschem: `x`, `y`, `rotation`, `mirror`) lo marca el módulo con `placement: true` (`Change::with_placement`); las listas de parámetros cambiados (`status`/`log --detail`, el texto de `diff`, el visor) usan `Change::params()`, que la omite. Así el núcleo no sabe qué claves son de ubicación en cada formato. En el JSON v2 el campo aparece solo cuando es `true`.
- `Registry` resuelve el módulo por extensión o firma (`for_path`, `detect`). `log`, `status`, `show` y `diff` reciben el registro: el análisis no sabe qué formatos existen. `extensions()` es lo que se compara y `openable()` suma lo que solo muestra un visor (los `.sym` de Xschem): de ahí salen el `ls` y el autocompletado del shell, el árbol del visor y los textos que nombran los formatos.
- Comparar un archivo entre dos versiones es **un solo flujo**, `core/analysis/diff_pair.rs`: cada lado es una `Version` (`Rev`, `WorkTree` o `Absent`) con su ruta, y `OnError` dice si un error de Git se propaga (`diff`, `show`) o queda en el archivo (`status`, `log`). `diff`, `show`, `status`, `log` y el visor pasan por ahí.
- Las rutas que escribe el usuario se nombran desde donde está, como en Git (`core/repo_path.rs`, para la CLI y el shell).
- Un archivo del formato que no se puede leer (truncado, con ciclos) es `ViewerError::Corrupt`: el visor lo explica en lenguaje claro por el tipo del error, sin buscar palabras en el mensaje. Una carga cancelada es `ViewerError::Cancelled`.
- `ViewerBackend` (`viewer-core`): `load`, `load_entry` (una sub-vista, p. ej. una celda) y `load_diff`, y sus variantes con los archivos de cada versión (`load_with`, `load_diff_with`, que por defecto delegan). Devuelven una `Scene` neutra: elementos, capas con su estilo, metadatos, entradas, cambios, fantasmas y anotaciones de diff, avisos, y un índice espacial opcional. El visor solo conoce esto: dibuja `.sch` y `.gds` por la misma ruta.
- Todo lo que se agregó a `viewer-core` después de la primera versión tiene valor por defecto, así que quien lo implementa por su cuenta (el crate de Carlos, con su feature `viewer-core-compat`) sigue compilando. La CI lo verifica.

## Reglas de dependencia

1. `riku-kernel` no depende de ningún módulo ni motor (la CI lo verifica con `cargo tree`).
2. Un módulo depende del kernel, de `viewer-core` y de su motor; nunca de otro módulo.
3. Los motores no saben nada de Riku; el adaptador vive del lado del módulo.
4. Agregar un formato es un módulo nuevo en `riku/src/modules/` (o un crate `riku-mod-*`) y una línea en `registry()`. El núcleo, la CLI y el visor no cambian. Excepción: las formas de onda (`spice`) no son planos y no pasan por `ViewerBackend`; el visor tiene una vista propia para ellas (`gui/wave_view.rs`).

## Features

| Feature | Qué suma |
|---|---|
| `xschem` | módulo de esquemáticos |
| `layout` | módulo de layouts (`riku-mod-layout`) |
| `spice` | módulo de simulaciones de ngspice (`.raw`); con `gui`, la vista de curvas (`egui_plot`) |
| `gui` | visor egui; arma el índice de las escenas en paralelo (`viewer-core/parallel`) |

Por defecto van las cuatro. `--no-default-features` da un `riku` solo de terminal; la CI compila cada combinación.

## Rendimiento

- **Diff de layouts:** huella jerárquica (árbol de Merkle sobre la jerarquía), instancias gemelas, huella por capa en forma canónica y XOR solo de lo que cambió, aplanando por pedazos ([`layouts.md`](layouts.md)).
- **Visor:** cada backend arma un `SceneIndex` al cargar (grillas por tamaño, relleno triangulado una vez, pirámide de cobertura). Por cuadro se consulta solo lo visible; si pasa de 60 000 elementos, lo diminuto se pinta como una imagen por capa ([`gui.md`](gui.md)).
- Hilos: un solo pool de `rayon` para el cálculo (un hilo del sistema por núcleo, o `--jobs N`/`RIKU_JOBS`; lo configura `riku` al arrancar), compartido por el diff y el índice del visor; `tokio` para la carga asíncrona del visor. `gdstk-rs` se lee desde varios hilos a la vez (`Library` es `Send + Sync`). `log`, `show` y `status` reparten commits y archivos (`core/analysis/parallel.rs`): una conexión a Git por hilo (`GitRepository::reopener`) y tandas planificadas por memoria, sin bloquear hilos.

## Por qué así

- **No plugins dinámicos:** Rust no tiene ABI estable; habría que pasar por C, perder tipos ricos como `Scene` y distribuir varios archivos.
- **Crates separados para lo que tiene un motor pesado** (`riku-mod-layout` con gdstk): Cargo verifica la regla de dependencia y cada parte compila por separado.
- **Una sola ruta de dibujo:** antes el visor tenía un camino para `.sch` y otro para el resto, y cada mejora se hacía dos veces.

El plan de migración que llevó hasta acá (fases 0–5) está en [`archivo/plan_migracion_microkernel.md`](archivo/plan_migracion_microkernel.md).
