# Decisiones del Binding Rust de gdstk

> Registro consolidado de decisiones arquitectónicas, scope descartado,
> gaps conocidos y cambios que se pospusieron. Actualizado al cierre de
> la Fase 8.8 (2026-04-19).

## Propósito

Documentar el "por qué" detrás del estado actual del binding Rust de
gdstk para que cualquier persona que retome el proyecto (o el futuro yo)
entienda qué se hizo, qué no, y qué razones motivaron cada elección.

---

## 1. Decisiones arquitectónicas principales

### 1.1 Lenguaje y stack
- **Rust + cxx crate** para el binding. No Python, no Go, no FFI manual con bindgen.
- **Python-bindings de gdstk se preservan** (no se tocan). Rust es una capa paralela.
- **vcpkg en Windows** para resolver zlib + qhull (dependencias de gdstk).
- **MSVC toolchain** (no MinGW). Requiere VS2019 BuildTools o superior.

### 1.2 Patrón de bindings
- **Shim C++ (`gdstk/rust/src/shims.{h,cpp}`)** traduce entre cxx-friendly API y la API template-pesada de gdstk.
- **Handles opacos** (marker structs + `reinterpret_cast` a punteros gdstk):
  - `CellHandle`, `PolygonHandle`, `LabelHandle`, `ReferenceHandle`,
    `FlexPathHandle`, `RobustPathHandle`, `RawCellHandle`, `RepetitionHandle`.
- **PIMPL** para tipos que requieren ownership complejo:
  - `LibraryHandle`, `TopLevelView`, `GdsInfoHandle`, `FlattenedPolygonsHandle`.
- **`#ifndef` guards** (no `#pragma once`) en headers del shim porque
  cxx-build duplica archivos en su crate dir.

### 1.3 Convenciones del API Rust
- **Read-only.** No hay setters ni constructores públicos para tipos de gdstk.
- **Lifetime-based borrowing.** `Cell<'a>`, `Polygon<'a>`, etc. tienen lifetime
  de la Library padre. El compilador impide use-after-free.
- **Iteradores lazy** en vez de eager `Vec<T>`. Consistente con estilo Rust.
- **Shared structs cxx** para tipos POD (`Point2D`, `BoundingBox`, `GdsTag`, `XorMetrics`).
- **Enums explícitos** en vez de magic numbers (`Anchor`, `EndType`, `JoinType`,
  `BendType`, `RepetitionType`, `ErrorCode`).
- **Convención de `repetition_count()==1` cuando no hay repetición** — gdstk
  devuelve 0; normalizamos a 1 para iteración consistente.

### 1.4 Miku es read-only
Decisión central: Miku como VCS no **construye** GDSs, solo los lee y compara.
Esto justifica descartar toda la API de modificación (translate, rotate, scale,
add, remove, set_*, etc.) que gdstk Python sí expone.

---

## 2. Fases ejecutadas

| # | Fase | Entregable | Tests |
|---|---|---|---|
| 1 | PoC `read_gds` + `cell_count` | API mínimo, vcpkg setup | — |
| 2 | Cell/Polygon iteration + area/layer/bbox | Foundation read API | — |
| 3 | Labels (text, anchor, origin) | Rename detection base | — |
| 4 | References + Boolean XOR (`cell.xor_with`) | Diff geométrico | — |
| 5 | **Fix correctness**: XOR incluye paths | Corrige bug silencioso | — |
| 5.5 | Accessors completos FlexPath/RobustPath | API de inspección | — |
| 6 | Library metadata + `write_gds` + `gds_info` | Roundtrip + peek rápido | 16 |
| 7 | Tests integración + snapshots + criterion bench | Regression suite | 16 |
| 8 | `perimeter`, `signed_area`, `cell.bbox`, `reference.bbox`, `Repetition`, `RawCell` | Gaps read-only | 21 |
| 8.5 | `Repetition` completo (kind, columns, rows, spacing, v1, v2, coords) | Inspección de patterns | 23 |
| 8.6 | `get_extrema` (corners de repetitions) | Optimización bbox | 24 |
| 8.7 | `polygon.point(idx)` / `polygon.points()` | Acceso vertex-level | 26 |
| 8.8 | `cell.get_polygons()` / `reference.get_polygons()` flatten jerárquico | Diff con refs anidadas | 31 |

**31 tests de integración pasan al cierre actual.**

---

## 3. Scope expandido (agregado más allá del plan original)

El roadmap original era 7 fases. Agregué fases 8, 8.5, 8.6, 8.7, 8.8 porque
durante la implementación surgieron bugs silenciosos o gaps que afectaban
la correctness del diff:

- **Fase 5** agregó el fix de XOR + paths **sin estar en el roadmap** — lo
  descubrí al explorar y decidí pararlo antes que seguir con otras cosas.
- **Fase 8** cerró gaps de lectura (perimeter, bbox, RawCell, Repetition)
  que identifiqué en auditoría honesta post-Fase 7.
- **Fase 8.5-8.8** fueron gaps progresivos descubiertos por preguntas del
  usuario durante review.

Lección: cada fase de "tests" revela gaps de las anteriores. El roadmap
inicial subestimó el scope real para un binding production-ready.

---

## 4. Cosas explícitamente DESCARTADAS

### 4.1 Archivos Python no portados (5 de 15)

| Archivo Python | Razón descarte |
|---|---|
| `curve_object.cpp` | Curve es utility transient. `FlexPath::spine` ya expone sus puntos vía `flexpath.spine_point(i)`. Exponerlo por separado sería redundante. |
| `raithdata_object.cpp` | Específico para máquinas de litografía e-beam Raith. <1% de GDSs lo usan. Fuera de scope para VCS general. |
| `gdswriter_object.cpp` | Streaming writer solo útil para GDSs >100 MB. `Library::write_gds()` de Fase 6 cubre writes normales. |

### 4.2 Métodos de construcción / modificación

Todos los métodos `set_*`, `translate`, `rotate`, `scale`, `mirror`, `fillet`,
`fracture`, `segment`, `arc`, `cubic`, `bezier`, `turn`, `rename_cell`,
`replace_cell`, `remap_tags`, `apply_repetition` **se descartan**.

Razón: Miku es read-only. Agregarlos sería código muerto (nunca usado) y
aumenta superficie de bugs.

### 4.3 OASIS format

- `read_oas`, `write_oas`, `oas_precision`, `oas_validate` → no expuestos.
- GDSII domina el 95%+ del uso industrial; OASIS solo aparece en foundries
  modernas y para chips muy grandes.

### 4.4 Paridad byte-a-byte con numpy

- Python devuelve `polygon.points` como numpy array 2D. Rust devuelve iterador lazy.
- Funcionalmente equivalente, no buscamos paridad exacta de representación.

### 4.5 Caché de flatten (`GeometryInfo` map)

- gdstk internamente tiene cache entre llamadas de `get_polygons`.
- No lo expusimos: Miku típicamente llama una vez por celda, no itera.

### 4.6 Linaje de transforms en polygons aplanados

- Cuando `reference.get_polygons()` devuelve polygons transformados, se pierde
  la info de "este polygon viene de la reference X rotada 90°".
- gdstk no trackea esto; agregarlo requeriría un sistema de traza propio (~8h).
- No lo implementamos. Diff reporta "polygon nuevo en (x,y)" sin procedencia.

---

## 5. Cosas POSPUESTAS (no descartadas, pero sin caso de uso hoy)

### 5.1 `Cell::get_labels` / `Cell::get_paths` aplanados jerárquicos

- gdstk los tiene. No los expuse porque el modelo de diff cell-by-cell de
  Miku detecta cambios en labels/paths al comparar cada celda individualmente.
- **Se agregarían si:** Miku cambia a un modelo "flat diff" (aplanar todo
  antes de comparar), o si agrega detección de "cells inlined/outlined".
- Costo estimado: ~1h cada uno (mismo patrón que `get_polygons`).

### 5.2 `Option<T>` en getters variant-específicos de Repetition

- Hoy `rep.spacing()` devuelve `Point2D{0,0}` si no es Rectangular.
- Más idiomatic Rust sería `Option<Point2D>` (Some solo si Rectangular).
- **Se cambiaría si:** publicamos `gdstk-rs` como crate público en crates.io.
- Costo: ~2h (breaking change de API — toca todos los call sites).

### 5.3 Publicación como crate público

- Si se hace público en crates.io, revisar el diseño por idiomatic Rust
  más estricto (Option, Error idiomático, más doc-tests).
- Hoy es consumido solo por Miku, convenciones internas funcionan.

### 5.4 CI remoto con GitHub Actions

- Fase 7 tiene `run_tests.sh` local. No hay `.github/workflows/*.yml`.
- **Se agrega cuando:** haya fork público de gdstk o Miku se publique.

### 5.5 DLLs copiadas automáticamente por `build.rs`

- Hoy el usuario copia `zlib1.dll` y `qhull_r.dll` manualmente (o via `run_tests.sh`).
- Alternativa: triplet `x64-windows-static` de vcpkg que estaticamente linkea.
- **Se arregla cuando:** alguien cansado de la fricción de DLLs protesta.

### 5.6 Métodos de conveniencia (`repetition.bbox()`, `reference.bbox_with_repetition()`)

- Se pueden computar desde lo que ya expusimos (extrema + min/max).
- Se agregan si un caso de uso lo repite tres veces.

---

## 6. Gaps CONOCIDOS (documentados)

### 6.1 `explicit_offset_count()` asimetría (resuelto en sesión actual)

- Inicialmente expuse `explicit_offset_count` (sin origen) al lado de
  `count` (con origen) → dos convenciones de conteo en la misma API.
- **Eliminé los métodos `explicit_*`** — `offsets()` con `skip(1)` cubre
  el mismo caso sin confusión.

### 6.2 Convención `repetition_count() == 0 vs 1`

- gdstk devuelve 0 para `RepetitionType::None`.
- Nuestro shim normaliza a 1 (instancia en origen).
- **Documentado** en `repetition_effective_count`.

### 6.3 Paths de Windows con caracteres no-UTF-8

- `Library::write_gds(path)` usa `fopen()` en C++.
- Para paths con chars no-UTF-8 puede fallar silenciosamente.
- **No mitigado** (raro en la práctica).

### 6.4 Performance de `flattened_polygons_at(i)` por índice

- Cada call es un FFI trip. Para vista con 10k polygons iterados, son 10k calls.
- **Aceptable** (~100 ns cada uno). Si se vuelve hot path, considerar
  iterador que toma snapshot en batch.

### 6.5 Ciclos de references

- Si un GDS tiene reference cycle (raro), `get_polygons(depth=-1)` podría
  loopear. gdstk supuestamente maneja esto pero no lo verifiqué.
- **Mitigación sugerida:** si un usuario reporta hang, setear `depth` finito.

---

## 7. Modelo de diff actual y sus limitaciones

### 7.1 Modelo actual: cell-by-cell

El diff de Miku hoy (implementado en `gdstk/rust/examples/diff_gds.rs`)
opera así:

1. Matchea celdas por **nombre** entre `lib_a` y `lib_b`.
2. Para cada par de celdas con el mismo nombre, corre `cell_a.xor_with(cell_b, layer)`.
3. `xor_with` **solo compara polygons y paths directos** de cada celda —
   no expande references.
4. Celdas que solo existen en A → "ELIMINADA".
5. Celdas que solo existen en B → "AGREGADA".

### 7.2 Blind spots conocidos

**Caso A: reference movida sin cambiar contenido**

Si `top` tiene una reference a `inverter` que se mueve de (0,0) a (10,5),
pero `inverter` en sí no cambió:
- `top` vs `top`: XOR directo dice "sin cambios" (los polygons de top son iguales, la reference en sí no aparece en polygon_array)
- `inverter` vs `inverter`: sin cambios

**Resultado:** Miku no reporta nada, pero el layout físico **sí** cambió.

**Caso B: celda renombrada**

Si `inverter` se renombra a `inv_v2` entre versiones:
- Miku reporta "inverter ELIMINADA, inv_v2 AGREGADA" — pierde la conexión.

**Caso C: reestructuración (inline ↔ outline)**

Si v1 tiene 100 polygons inline en `top` y v2 los mueve a una nueva celda
`group_100` referenciada desde `top`:
- `top` vs `top`: XOR dice "100 polygons ELIMINADOS" (los inline)
- `group_100`: solo existe en v2 → "AGREGADA"

**Resultado:** ruido, aunque el layout físico es idéntico.

### 7.3 Modelo alternativo: flat diff (disponible pero sin usar)

La maquinaria para aplanar está implementada (Fase 8.8 —
`cell.get_polygons().build()`) pero `diff_gds.rs` no la usa todavía.

Con flat diff:
- Expande todas las references recursivamente con transforms aplicados
- Compara los polygons aplanados entre A y B
- Caso A se detecta (polygons en posiciones distintas)
- Caso C se detecta como "sin cambios" geométricos
- Pierde precisión en atribución (dice "cambió en (x,y) global" sin decir qué celda)

### 7.4 Tradeoffs de los dos modelos

| | Cell-by-cell (actual) | Flat (`get_polygons(depth=-1)`) |
|---|---|---|
| Velocidad | Rápido | Más lento (expande jerarquía) |
| Detecta refs movidas | ✗ | ✓ |
| Detecta inline/outline | ✗ (genera ruido) | ✓ |
| Atribución a celda | Precisa | Ambigua (coords globales) |
| Detecta renombres | ✗ (ambos modelos) | ✗ |
| Memoria | Baja | Alta (polygons duplicados) |
| Caso de uso | Diff de autor iterando | Audit de tapeout completo |

### 7.5 Plan futuro (no decidido, documentado por si se retoma)

Cuando se construya `miku diff` como CLI final:
- **Default:** cell-by-cell (rápido, atribución precisa).
- **Flag `--deep`:** flat diff para casos de reestructuración.
- `--deep` requeriría agregar `cell.get_labels(depth)` y `cell.get_paths(depth)`
  para completitud (ver §5.1).

Alternativa: detectar heurísticamente si vale la pena hacer flat (por
ejemplo, si el count total de polygons directos difiere mucho vs reference
counts, sugerir `--deep`).

### 7.6 Detección de renombres (fuera de ambos modelos)

Los renombres de celdas no los detecta ningún modelo actual. Alternativas
futuras para investigar:
- **Hash de contenido por celda** (bbox + area + polygon count por layer) →
  si el hash coincide pero el nombre no, proponer match.
- **Fuzzy matching de nombres** (Levenshtein) → solo heurístico, frágil.
- **Historial de git** (parsear rename hints de `git log --follow`).

Por ahora Miku reportará rename como "DELETE + ADD". Documentar esta
limitación en el CLI.

---

## 8. Convenciones de estilo del código Rust

- **Nomenclatura:** `Polygon<'a>`, `Cell<'a>` (structs con lifetime).
  `PolygonHandle` (opaque FFI type sin lifetime).
- **Getters:** `polygon.layer()` no `polygon.get_layer()` — idiomatic Rust.
- **Returns:** `Point2D` shared struct para coords, `BoundingBox` para bboxes.
- **Iteradores:** `.polygons()`, `.cells()`, `.labels()`, etc. devuelven
  `impl Iterator<Item = T<'_>> + use<'a>`.
- **Docstrings:** en español donde sea claro, en inglés donde el término
  técnico es internacional (FFI, PIMPL, etc).

---

## 9. Archivos clave del proyecto

| Archivo | Rol |
|---|---|
| `gdstk/rust/Cargo.toml` | Dependencias cxx + criterion |
| `gdstk/rust/build.rs` | Compila 18 .cpp de gdstk + Clipper + shims + linkea vcpkg |
| `gdstk/rust/src/shims.h` | Declaraciones FFI (`extern "C++"`) |
| `gdstk/rust/src/shims.cpp` | Implementación de los wrappers shim |
| `gdstk/rust/src/lib.rs` | Bridge cxx + wrappers Rust ergonómicos |
| `gdstk/rust/tests/integration.rs` | 31 tests de integración |
| `gdstk/rust/benches/gds_bench.rs` | Criterion benchmarks |
| `gdstk/rust/run_tests.sh` | CI local |
| `gdstk/rust/README.md` | Estado actual del binding |
| `research/arquitectura/gdstk_rust_bindings_migracion.md` | Plan original (7 fases) |
| `research/arquitectura/gdstk_rust_decisiones.md` | **Este documento** |

---

## 10. Cómo actualizar este documento

Cuando se agregue una fase o se tome una decisión importante:

1. Agregar fila a la tabla en **§2 Fases ejecutadas** con número, entregable, tests.
2. Si es scope expandido, nota en **§3**.
3. Si se descarta algo nuevo, agregar a **§4** con razón.
4. Si se pospone algo, agregar a **§5** con "se agrega cuando…".
5. Si aparece un gap conocido, documentar en **§6**.

No borrar decisiones anteriores aunque se reviertan — tachar y anotar el
cambio para preservar el rastro histórico.
