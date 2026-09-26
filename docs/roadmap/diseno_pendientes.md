# Diseño: warnings, CI y pendientes de prioridad media y baja

Diseño de implementación para los items de `docs/roadmap/pendientes.md`: warnings `f32` (#2), CI (#1), media (#4–#8) y baja (#9–#14). La paridad `.sch` (#3) queda fuera de este diseño.

**Fecha:** 2026-09-26 · **Base:** `main` `38f03c3`

---

## 0. Orden y dependencias

```
Fase 0  #2 warnings f32 ──┐
                          ▼
Fase 1  #1 CI (Linux, -D warnings) ──► #12 job Windows (no bloqueante)
                          │
Fase 2  #8 orden etiquetas   #6 cambios por instancia   #5 renombres   #4 OASIS
        #7 tools/verify (usa #4 para comparar .oas también)
                          │
Fase 3  #10 triangulación al cargar   #9 cache XOR   #11 autocompletado   #14 paletas .lyp
        #13 se cierra como limitación de egui
```

- La CI va antes que lo demás: cada cambio de las fases 2 y 3 entra ya verificado.
- `-D warnings` en la CI solo tiene sentido con la Fase 0 hecha.
- #4 toca el submódulo `external/gdstk` (repo `Adriel2503/gdstk_rust`): hay que hacer commit y push allí primero y luego actualizar el puntero del submódulo en riku_chip.
- Cada item es un commit propio con sus tests.

| # | Item | Crates que toca | Esfuerzo |
|---|---|---|---|
| 2 | Warnings `f32` | riku-gui | S |
| 1 | CI Linux | `.github/` | M |
| 8 | Orden al fusionar etiquetas | riku-gui | S |
| 6 | Cambio por instancia | gds-renderer, riku (formato texto) | M |
| 5 | Celdas renombradas | gds-renderer, riku, riku-gui | M |
| 4 | OASIS | gdstk-rs, gds-renderer, riku, riku-gui | M |
| 7 | `tools/verify/` | repo (Python) | S |
| 10 | Triangular al cargar | riku-gui | S |
| 9 | Cache del diff | gds-renderer, riku | M |
| 11 | Autocompletado del shell | riku | S |
| 14 | Paletas desde `.lyp` | tools, gds-renderer | S |
| 12 | Build Windows | `.github/`, docs | M (exploratorio) |
| 13 | Tracking tipográfico | — | cerrar |

---

## Fase 0 — #2 Warnings `float_literal_f32_fallback`

**Causa:** `Stroke::new(width: impl Into<f32>, …)`. Un literal `1.0` sin tipo cae a `f32` por un fallback que rustc va a retirar.

**Cambio:** sufijo explícito en los 18 sitios: `sch_painter.rs` (16) y `app.rs:1547` y `app.rs:1578` (2). `1.0` → `1.0_f32`. Donde se repita el mismo trazo, usar una constante `const HAIRLINE: f32 = 1.0;`.

**Listo cuando:** `cargo build` y `cargo test` de riku-gui terminan con 0 warnings (y lo mismo en riku, gds-renderer y viewer-core).

---

## Fase 1 — #1 Integración continua

### Workflow `.github/workflows/ci.yml`

```yaml
on: { push: { branches: [main] }, pull_request: {} }
concurrency: { group: ci-${{ github.ref }}, cancel-in-progress: true }
env: { CARGO_TERM_COLOR: always, RUSTFLAGS: "-D warnings" }

jobs:
  test:
    runs-on: ubuntu-24.04
    strategy:
      fail-fast: false
      matrix:
        crate: [viewer-core, gds-renderer, riku, riku-gui]
    steps:
      - uses: actions/checkout@v4
        with: { submodules: recursive }
      - run: sudo apt-get update && sudo apt-get install -y
               build-essential pkg-config zlib1g-dev libqhull-dev
               libxkbcommon-dev libwayland-dev libgl1-mesa-dev
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: "${{ matrix.crate }} -> target" }
      - run: cargo test --locked
        working-directory: ${{ matrix.crate }}

  xschem-compat:          # garantiza que el contrato viewer-core no rompe el submódulo
    runs-on: ubuntu-24.04
    steps: [checkout con submódulos, toolchain, cache,
            cargo check --features viewer-core-compat en external/xschem-viewer-rust
            (sin -D warnings: es código externo)]

  fmt:
    runs-on: ubuntu-24.04
    steps: [checkout, cargo fmt --check en los 4 crates]
```

**Decisiones:**
- **Matriz por crate:** no hay workspace. Cada crate tiene su `Cargo.lock` y su `target/`, y compila en paralelo.
- **`--locked`:** la CI falla si alguien cambió dependencias sin actualizar el lock (pasó una vez con `persistence`). `viewer-core` no tiene lock: se usará `cargo test` sin `--locked` o se versiona su `Cargo.lock`.
- **`RUSTFLAGS=-D warnings`** solo en los jobs de crates propios. Cambiar `RUSTFLAGS` invalida la cache, así que es el mismo valor en todos los jobs propios.
- **Paquetes de sistema:** `libqhull-dev` y `zlib1g-dev` son para gdstk (el build usa pkg-config con `qhull_r`). Las librerías X11/Wayland/GL son para enlazar eframe; los tests no abren ventanas.
- **`fmt` antes de activarlo:** correr `cargo fmt` una vez en los 4 crates (commit aparte, solo formato). Si el diff sale muy grande, se deja el job `fmt` para después.
- **Clippy:** fuera por ahora. Se agrega como job no bloqueante cuando esté limpio.

**Riesgos:**
- Los tests `riku/tests/*` crean repos git reales: puede faltar `user.name`/`user.email`. Paso de mitigación: `git config --global user.name ci && git config --global user.email ci@localhost`.
- Build frío largo (gdstk en C++ + egui): unos 8–10 min por crate la primera vez; con la cache, 2–3 min.

**Listo cuando:** push a `main` y PR muestran los checks en verde, y un warning o test roto los pone en rojo (probarlo con un commit en una rama).

---

## Fase 2 — Prioridad media

### #8 Orden al fusionar etiquetas

**Problema:** `merge_coincident` concatena en orden de llegada (`VPB · VPWR`).

**Diseño:** prioridad explícita en el candidato, calculada fuera del algoritmo de colocación.

```rust
// label_layout.rs
pub struct LabelCandidate { pub anchor: Pos2, pub text: String, pub color: Color32, pub rank: u8 }

pub fn label_rank(text: &str) -> u8  // 0 = alimentación, 1 = pin de señal, 2 = resto
```

- `rank 0`: nombres de alimentación conocidos: `VPWR VGND VDD VSS VCC VEE VDDIO VSSIO` y los que empiezan con `vdd`/`vss` (sin distinguir mayúsculas).
- `rank 1`: el resto de etiquetas de capas de pin (el backend ya las envía primero).
- `rank 2`: textos decorativos.
- Al fusionar, las partes se ordenan de forma estable por `(rank, orden de llegada)` y el candidato fusionado toma el menor `rank`. Con eso, `VPWR · VPB` y `VGND · VNB`.
- `place` también ordena por `rank` antes del anti-solapamiento: si falta espacio, lo que se omite son los textos decorativos, no la alimentación.

**Tests:** fusión `VPB`+`VPWR` → `VPWR · VPB`; el orden estable se conserva dentro del mismo rank; con espacio para una sola etiqueta, se coloca la de alimentación.

### #6 Un item por instancia

**Problema:** `origin_of_polygon` atribuye a la *celda* referenciada (`["TOP","inv_2"]`). Todas las instancias de `inv_2`, y todas las repeticiones de un AREF, caen en el mismo bucket con un solo bbox.

**Diseño:** atribuir a la *instancia concreta*.

```rust
// hier_walk.rs
pub struct Origin {
    pub path: OriginPath,                 // igual que hoy: ["TOP", "inv_2"]
    pub instance: Option<InstanceId>,     // None = geometría directa de la raíz
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId { pub reference: u32, pub repetition: u32 }

pub fn origin_of_polygon(cell: &Cell, poly: &OwnedPolygon) -> Origin
```

- **Bbox de una repetición sin FFI nuevo:** gdstk da el bbox de la reference con todas las repeticiones, que es la suma de Minkowski del bbox de la instancia 0 y el rango de offsets. Entonces `bbox0.min = all.min − min_offset` y `bbox0.max = all.max − max_offset`, y la instancia *i* es `bbox0 + offset_i`. Se prueba cada instancia y gana la de menor área, con desempate por `(reference, repetition)` para que el resultado sea determinista.
- **Guarda de rendimiento:** si `repetition_count > 4096`, no se itera y se atribuye a la reference completa (`repetition = u32::MAX`, que significa "todo el arreglo").
- **Bucket:** la clave pasa de `OriginPath` a `(OriginPath, Option<InstanceId>)`. `GdsGeomDiff` gana `instance: Option<InstanceId>` y su `bbox_um` queda por instancia de forma natural.
- **GUI** (`change_items`): etiqueta `met1 68/20 · en inv_2 #3`, con el bbox de esa instancia.
- **CLI:** la salida JSON agrega `"instance": {"reference": 0, "repetition": 3}`. La salida de texto agrupa los items iguales salvo la instancia en una sola línea con `×N instancias` para no inundar la terminal; `--detail` los lista uno por uno.

**Tests:** una celda con 2 SREF de `INV` y un cambio en `INV` da 2 items con bboxes disjuntos; un AREF 3×2 da 6 items; el e2e existente de `TOP/INV` sigue igual, con `instance` presente.

### #5 Celdas renombradas

**Problema:** un rename aparece como `celda eliminada A` + `celda añadida B`.

**Diseño:** emparejar dentro de `changed_cells`, que ya calcula la huella de la geometría aplanada.

```rust
pub enum CellChange { Added, Removed, Modified, Renamed { from: String } }   // pierde Copy → Clone
```

1. Después de la pasada actual, `removed = {cells solo en A}` y `added = {cells solo en B}`.
2. Se indexan las huellas de `removed`. Para cada celda de `added` con huella idéntica a una única celda de `removed`, se emparejan: `B → Renamed { from: A }` y `A` sale de la lista.
3. **Confirmación:** XOR de `A` contra `B` con todas las capas (la huella puede colisionar). Solo si queda vacío se marca rename.
4. **Ambigüedad:** si hay varias candidatas con la misma huella (celdas vacías o duplicadas), no se empareja y queda como está hoy. La celda vacía no se considera nunca.
5. **Efecto en los padres:** la reference del padre cambia de nombre pero la geometría aplanada no. La huella del padre no cambia y el padre no aparece como modificado, que es lo correcto.
- **Fuera de alcance:** rename *con* cambios (emparejar por similitud). Se documenta como posible extensión.
- **Consumidores:** la GUI muestra `↻` en el selector y el item "celda renombrada: A → B". El `GdsDriver` de la CLI emite un `Change` con `kind = Modified` y `after["renamed_from"] = A`, y el formateador de texto muestra `renombrada A → B`.

**Tests:** una library con `INV` renombrada a `INV2` y el padre actualizado da `{INV2: Renamed{from: INV}}` y ningún `Added`/`Removed`; dos celdas vacías renombradas no se emparejan; un rename con un polígono movido sigue como `Added` + `Removed`.

### #4 Soporte OASIS

**gdstk-rs (submódulo):**

```rust
// ffi
fn read_oas_with_error(filename: &str, out_error: &mut u8) -> UniquePtr<LibraryHandle>;
// API
impl Library {
    pub fn from_oas_bytes(data: &[u8]) -> Result<Self, Error>;   // TempPath ".oas", unit = 0 (conserva la del archivo)
    pub fn from_bytes_any(data: &[u8]) -> Result<Self, Error>;   // decide por magic
}
pub fn sniff_format(data: &[u8]) -> Option<Format>;              // Gds | Oasis
```

- **Magic de OASIS:** `%SEMI-OASIS\r\n` (13 bytes). GDS: el primer registro es `HEADER` (`00 06 00 02`).
- La unidad de OASIS siempre es 1e-6, así que `lib.unit() / 1e-6 = 1` y el `unit_factor` actual sirve sin cambios.
- **Tests en el submódulo:** una library pequeña escrita como GDS y como OASIS (el C++ de gdstk tiene `write_oas`) da los mismos polígonos por capa.

**riku_chip:**
- `GdsDriver::info().extensions` pasa a `[".gds", ".oas"]`. Todas las lecturas van por `from_bytes_any`.
- `GdsBackend`: `extensions: &["gds", "oas"]`, y `accepts` reconoce los dos magics.
- `riku-gui/src/project.rs`: `OPENABLE` agrega `oas`.
- **Mensajes:** los textos visibles para el usuario dicen "layout" o "GDS/OASIS" donde hoy dicen "GDS".
- **Tests:** e2e con un `.oas` en un repo git (diff de CLI con la misma área que su gemelo `.gds`) y un test de `GdsBackend` que carga `.oas` desde bytes.

### #7 Scripts de verificación en el repo

```
tools/verify/
  README.md               # cómo correrlos en el contenedor iic-osic-tools
  cmp_klayout.py          # geometría + labels por celda (hoy _cmp_klayout_all.py)
  cmp_xor_klayout.py      # XOR por capa contra KLayout
  dump_riku.rs → example  # `cargo run --example dump_cells -p gds-renderer` para el lado Riku
  run_all.sh              # SKY130 / GF180 / IHP (+ OASIS cuando exista #4)
  gui/xt.py               # driver XTest (clic, teclas, rueda, arrastre, captura)
  gui/shot.sh             # xwd → PNG
```

- Hoy los scripts leen la salida de Riku desde binarios de pruebas sueltos. El lado Riku pasa a un `examples/dump_cells.rs` versionado en gds-renderer, que emite JSON por celda.
- Las rutas de los PDKs salen de `$PDK_ROOT` (por defecto `/foss/pdks`), sin rutas fijas.
- La salida es una tabla por librería (`idéntico` / `N diferencias`), con código de salida ≠ 0 si hay diferencias.
- No entra en la CI (necesita KLayout y los PDKs). Queda documentado como verificación manual antes de tocar el render o el diff.

---

## Fase 3 — Prioridad baja

### #10 Triangular los polígonos cóncavos al cargar

**Problema:** `paint_filled_polygon` llama a `triangulate` en cada frame para cada cóncavo.

**Diseño:** una cache paralela a la escena, en la GUI, sin tocar `viewer-core`.

```rust
// polygon_fill.rs
pub struct FillCache { tris: Vec<Option<Arc<[u32]>>> }   // índice = índice del elemento
impl FillCache { pub fn build(scene: &RenderableScene) -> Self }  // solo Polygon rellenos, cóncavos y con ≥ 4 vértices
```

- Se construye en el mismo hilo de carga (`spawn_backend_load`), junto con la escena, y se guarda en `BackendState`. Una escena nueva trae su cache nueva.
- `paint_filled_polygon` recibe `Option<&[u32]>`: si viene, usa esos índices; si no, hace lo de hoy (convexo o triangulación al vuelo).
- **Medir antes y después:** tiempo de frame en `sky130_fd_sc_hd` completo (437 celdas) y en una celda grande. Si la ganancia es menor al 10 %, se descarta y se documenta.

### #9 Cache del diff

**Alcance:** la CLI y la carga de diff de la GUI con layouts grandes.

**Clave:** los blobs de git ya son hashes de contenido.

```
key = sha256( riku_version ‖ oid_before ‖ oid_after ‖ cell ‖ cosmetic_threshold )
dir = $XDG_CACHE_HOME/riku/diff/  (dirs::cache_dir(); en Windows %LOCALAPPDATA%)
```

- **Qué se guarda:** el `CellDiff` completo (métricas + polígonos XOR) y `changed_cells`, serializados con `bincode` (versionado por `riku_version`). Hay que derivar `Serialize` en `OwnedPolygon` (feature `serde` en gdstk-rs) o copiar a un tipo propio en gds-renderer; se elige el tipo propio para no tocar el submódulo.
- **Cuándo se usa:** solo si los dos blobs pesan más de 1 MiB en total. Los diffs chicos tardan milisegundos y no justifican la cache.
- **Fuera de git** (archivos del working tree): la clave usa el hash del contenido en lugar del OID.
- **Límites:** sin crecimiento infinito. Se usa LRU por `mtime` con tope de 512 MiB y limpieza oportunista al escribir. Opción `--no-cache` en `riku diff` y `RIKU_NO_CACHE=1`.
- **Corrupción:** si una entrada no se puede deserializar, se borra y se recalcula. Nunca es un error para el usuario.
- **Tests:** hit y miss de la cache (el segundo diff no llama al XOR, medido con un contador), invalidación por versión y entrada corrupta.

### #11 Autocompletado en el shell

`DefaultEditor` pasa a `Editor<RikuHelper, DefaultHistory>`:

```rust
#[derive(Helper, Hinter, Highlighter, Validator)]
struct RikuHelper { files: FilenameCompleter, cwd: Rc<RefCell<PathBuf>> }
impl Completer for RikuHelper { … }
```

- **Primera palabra:** comandos del shell (`cd ls help exit status log diff doctor open`).
- **Después de `cd`:** solo directorios.
- **Después de `diff`, `log` y `open`:** ramas y tags del repo (`git2`), commits cortos recientes (los últimos 20) y archivos `.sch`/`.gds`/`.oas`.
- **Flags:** `--detail --full --json --compact --paths --branch --format`.
- La completion es relativa al `cwd` del shell (que cambia con `cd`), no al del proceso.
- **Tests:** una función pura `complete(line, pos, ctx) -> Vec<String>` sobre un repo temporal.

### #14 Paletas completas desde los `.lyp`

```
tools/palettes/gen_palettes.py   # lee .lyp (XML) → gds-renderer/src/palette/generated_{gf180,ihp}.rs
```

- **Del `.lyp`** (`<properties>`) se extrae `source` (`L/D@1`), `name`, `fill-color`, `dither-pattern` y `visible`.
- **Rol:** la tabla manual actual gana siempre (son las capas curadas, con rol y rank). Para las capas generadas, el rol sale del datatype con las reglas actuales de fallback, y además `dither-pattern` hueco o `visible=false` da `Outline`. El rank va detrás de las curadas, en orden del `.lyp`.
- Los archivos generados llevan cabecera `// @generated por tools/palettes/gen_palettes.py desde <lyp> — no editar`. El script se corre a mano en el contenedor cuando cambie el PDK.
- `layer_spec` busca primero en la tabla curada, después en la generada y al final usa el fallback genérico.
- **Tests:** cada capa curada sigue igual; una capa solo generada de IHP (p.ej. una de las 376) tiene nombre y color del `.lyp`; no hay entradas `(layer, datatype)` duplicadas.

### #12 Build en Windows

- **Job CI `windows-latest`** (VS 2022) con `continue-on-error: true`:
  - `vcpkg install zlib:x64-windows qhull:x64-windows` con cache de `VCPKG_DEFAULT_BINARY_CACHE`;
  - `cargo build` de `riku` y `riku-gui`.
- **Si el job pasa:** se documenta en la sección §6 de `integracion_gds_estado.md` que la vía soportada es VS 2022. Después de una semana en verde se quita `continue-on-error`.
- **Si falla por el linker:** probar `-C link-arg=-fuse-ld=lld` / `rust-lld` en `.cargo/config.toml` solo para Windows.
- El MSVC 2019 local no se intenta arreglar: queda como no soportado.

### #13 Tracking tipográfico

Se cierra: egui 0.34 no expone el espaciado entre letras. En `pendientes.md` pasa a "Limitaciones conocidas", sin esfuerzo asignado.

---

## Validación del diseño

| Criterio | Cómo se cumple |
|---|---|
| El contrato neutro no se rompe | Ningún cambio toca `viewer-core`, salvo que haga falta un campo nuevo en `ChangeItem`, y tampoco hace falta. La CI verifica el submódulo Xschem |
| Compatibilidad de la CLI | En JSON solo se agregan campos (`instance`, `renamed_from`). El texto conserva el formato, y `×N` es solo agrupación |
| Verificación contra KLayout | #7 la hace repetible; se corre después de #4, #5 y #6 |
| Rendimiento | #6 tiene guarda de repeticiones. #9 y #10 se miden antes de quedarse |
| Mantenibilidad | Cada item es un commit con tests. El código generado está marcado y es reproducible |

## Documentación al cerrar

- `pendientes.md`: mover los items hechos a "Hecho", #13 a limitaciones y actualizar el hash de `main`.
- `integracion_gds_estado.md`: OASIS, renombres, instancias, CI y tablas de tests actualizadas.
- `README.md`: badge de CI y `.oas` en la lista de formatos.
