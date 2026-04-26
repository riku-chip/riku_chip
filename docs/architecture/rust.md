# Arquitectura Rust de riku_rust

## Decisión: monolito modular + hexagonal

Se eligió un único crate con módulos bien delimitados en lugar de un workspace multi-crate.

**Razones:**
- El proyecto es pequeño: ~3000 líneas de Rust
- Un workspace añade fricción de build y versionado sin beneficio real a este tamaño
- La separación de responsabilidades se logra con módulos, no con crates separados

**Frontera hexagonal:**
- `core/` — dominio puro (sin I/O, sin git2, sin xschem)
- `core/ports.rs` — traits que el dominio necesita (GitRepository, SchematicParser)
- `adapters/` — implementaciones concretas (xschem_driver)
- `cli.rs` — única entrada de I/O al sistema

## Módulos

```
src/
  main.rs           → punto de entrada, llama cli::run()
  lib.rs            → re-exports de módulos para tests
  cli.rs            → clap, subcomandos, formateo de salida
  core/
    models.rs       → tipos de dominio (Component, Wire, Schematic, DiffReport...)
    error.rs        → RikuError con thiserror
    ports.rs        → traits GitRepository, SchematicParser, RendererPort
    driver.rs       → trait RikuDriver, DiffEntry, DriverDiffReport, DriverInfo
    registry.rs     → get_drivers(), get_driver_for()
    git_service.rs  → impl GitRepository con git2
    analyzer.rs     → analyze_diff() — orquesta git + parser + driver
    semantic_diff.rs → diff() puro sobre dos &[u8]
    svg_annotator.rs → annotate(), Transform, mooz calibration
  adapters/
    xschem_driver.rs  → impl RikuDriver para Xschem
    xschem_adapter.rs → helper de bajo nivel (render_svg, available)
  parsers/
    xschem.rs       → parse(), detect_format()
```

## Traits principales

### `RikuDriver` (core/driver.rs)
```rust
pub trait RikuDriver: Send + Sync {
    fn info(&self) -> DriverInfo;
    fn diff(&self, content_a: &[u8], content_b: &[u8], path_hint: &str) -> DriverDiffReport;
    fn normalize(&self, content: &[u8], path_hint: &str) -> Vec<u8>;
    /// Devuelve el SVG en memoria. `None` si el driver no soporta render.
    fn render(&self, content: &[u8], path_hint: &str) -> Option<String>;
}
```

### `GitRepository` (core/ports.rs)
```rust
pub trait GitRepository {
    fn get_blob(&self, commit_ish: &str, file_path: &str) -> Result<Vec<u8>, GitError>;
    fn get_commits(&self, file_path: Option<&str>) -> Result<Vec<CommitInfo>, GitError>;
    fn get_changed_files(&self, commit_a: &str, commit_b: &str) -> Result<Vec<ChangedFile>, GitError>;
}
```

## Decisiones de implementación

### OnceLock para versión de xschem
```rust
pub struct XschemDriver {
    cached_info: std::sync::OnceLock<DriverInfo>,
}
```
La versión de xschem se consulta una vez por sesión. `OnceLock` es thread-safe sin Mutex.

### BTreeMap en lugar de HashMap
Se usa `BTreeMap<String, Component>` en `Schematic` para orden determinista en tests y JSON.

### thiserror para errores
Cada subsistema tiene su propio tipo de error (`GitError`, `AnalyzeError`, `RikuError`) con conversiones automáticas via `#[from]`.

## Tests

| Archivo | Qué prueba |
|---------|-----------|
| `tests/basic.rs` | Parser, diff semántico, git service con repos temporales |
| `tests/parity.rs` | Paridad JSON exacta con Python (se salta si Python no disponible) |
| `tests/stress.rs` | Throughput, GDS binario, stress de git bajo carga, large blob |
| `src/**/*.rs` | Unit tests inline en cada módulo |

## Cómo correr los tests

```bash
# todos los tests
cargo test

# solo stress
cargo test --test stress

# solo parity (requiere Python + pygit2 + typer)
cargo test --test parity

# con output de println!
cargo test -- --nocapture
```
