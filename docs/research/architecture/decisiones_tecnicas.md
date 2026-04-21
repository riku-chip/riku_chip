# Decisiones técnicas de gdstk-rs

> Binding Rust de la librería C++ [gdstk](https://github.com/heitzmann/gdstk), pensado como base para **Miku**, un sistema de control de versiones (VCS) especializado en archivos GDSII.

Este documento consolida las decisiones arquitectónicas que dieron forma al proyecto `gdstk-rs` (directorio `gdstk/rust/` del repo). Está dirigido a desarrolladores futuros que retomen el trabajo y necesiten entender **por qué** se optó por cada camino —no solo qué se hizo—.

Documentos relacionados:
- [`research/arquitectura/gdstk_rust_bindings_migracion.md`](./gdstk_rust_bindings_migracion.md) — roadmap original (7 fases).
- [`research/arquitectura/gdstk_rust_decisiones.md`](./gdstk_rust_decisiones.md) — primer borrador parcial de decisiones.
- [`research/arquitectura/lenguajes_y_stack.md`](./lenguajes_y_stack.md) — análisis comparativo de lenguajes.
- Código: [`gdstk/rust/src/lib.rs`](../../gdstk/rust/src/lib.rs), [`gdstk/rust/src/shims.cpp`](../../gdstk/rust/src/shims.cpp).

---

## 1. Contexto y propósito

Miku necesita leer, comparar y atribuir cambios en archivos GDSII con precisión geométrica y buen desempeño. Un VCS de layouts tiene necesidades muy distintas a un editor de layouts: **no construye geometría, solo la inspecciona**. Eso permeó toda la arquitectura.

El binding `gdstk-rs` es la pieza que expone gdstk (C++) al ecosistema Rust de forma segura, idiomática y con el mínimo de *runtime cost*. Miku será otro crate aparte que lo consumirá como dependencia Cargo.

---

## 2. Decisión principal: Rust + cxx

### 2.1 Por qué Rust y no alternativas

Se evaluaron cinco caminos y se eligió **Rust + binding a gdstk C++ vía `cxx`**. Razones comparativas:

| Alternativa | Estado | Problema bloqueante |
|---|---|---|
| Python + gdstk (pybind11) | Maduro, usado por diseñadores | GIL impide paralelismo real; dependencia de runtime Python en un VCS; arranque lento |
| Go + cgo + gdstk | Posible | Overhead de **~100–200 ns por llamada** cgo (Rust+cxx: ~2–5 ns). Un diff de millones de polígonos amplifica ese costo linealmente |
| `gds21` / `Layout21` nativo Rust | Experimental (pre-release 2023) | Sin operaciones booleanas (Clipper), sin tesselación, parser incompleto. Apostar a esto = construir lo que gdstk ya resolvió |
| `gdsdk` (Rust) | Abandonado | Sin mantenimiento, cobertura muy parcial de GDSII |
| Reimplementar gdstk en Rust puro | Técnicamente viable | Meses/años de trabajo replicando Clipper, qhull y parser binario GDSII. No aporta valor diferencial a Miku |

**Criterio decisivo:** gdstk liberó `v1.0.0` en febrero 2026 con core C++ maduro, Clipper integrado para booleans, y cobertura completa del estándar GDSII/OASIS. Reinventarlo es antieconómico. Rust aporta el *frontend* seguro; gdstk aporta el motor probado.

### 2.2 Por qué `cxx` y no bindgen / autocxx

| Opción | Por qué se descartó |
|---|---|
| `bindgen` puro | Genera FFI crudo → todo el wrapper queda marcado `unsafe`, el esfuerzo de envolverlo es el mismo que con cxx pero sin garantías |
| `autocxx` | Menos *safety guarantees* estáticas, más frágil ante cambios de headers, depende de heurísticas de Clang que no siempre coinciden con gdstk |
| **`cxx` 1.0.194** (enero 2025) | Estable, permite definir el contrato FFI en un `mod` Rust, fuerza *shared structs* POD y *opaque types* explícitos. La superficie `unsafe` queda acotada al shim C++ |

`cxx` obliga a pensar el binding como un **contrato bidireccional**, lo cual es valioso cuando la librería C++ tiene ownership complejo (gdstk usa arenas, `Array<T>` propio, `reinterpret_cast` entre jerarquías).

---

## 3. Patrón de binding

El shim vive en dos archivos principales: el bridge `cxx` centralizado en [`src/lib.rs`](../../gdstk/rust/src/lib.rs) y las implementaciones C++ en [`src/shims.cpp`](../../gdstk/rust/src/shims.cpp).

### 3.1 Handles opacos vía struct-marker

Para tipos C++ que gdstk maneja por puntero (`gdstk::Library*`, `gdstk::Cell*`, `gdstk::Polygon*`, etc.) se definen **structs marker vacíos** en el bridge cxx y el shim hace `reinterpret_cast` entre ellos y los tipos gdstk reales.

Ventajas:
- El bridge Rust no necesita conocer el layout C++ de gdstk.
- Cambios internos en gdstk no rompen el ABI del binding, siempre que los métodos públicos mantengan firma.
- Permite lifetimes Rust (`Cell<'a>`, `Polygon<'a>`) atados a la vida del `Library` dueño.

### 3.2 PIMPL para tipos con ownership complejo

Los siguientes tipos tienen estado adicional que no cabe en un puntero crudo a gdstk y se implementaron como **PIMPL (Pointer to IMPLementation)**:

| Tipo | Qué encapsula |
|---|---|
| `LibraryHandle` | Propietario del `gdstk::Library`, gestiona ciclo de vida |
| `TopLevelView` | Cache de top cells derivadas, evita recomputar |
| `GdsInfoHandle` | Metadatos extraídos sin parsear geometría completa (fast path) |
| `FlattenedPolygonsHandle` | Resultado materializado de flatten con sus referencias |

### 3.3 Shared POD structs

Estructuras pequeñas, sin invariantes ocultos, se declaran como `struct` compartido en el bridge cxx —layout idéntico en ambos lados, cruzan la FFI por valor sin alocación—:

- `Point2D { x: f64, y: f64 }`
- `BoundingBox { min: Point2D, max: Point2D }`
- `GdsTag { layer: u32, datatype: u32 }`
- `XorMetrics { area_diff: f64, polygon_count: u64, ... }`

### 3.4 Enums con valores numéricos gdstk-compatibles

Los enums Rust se declaran con discriminantes explícitos que coinciden 1:1 con los de gdstk. Eso permite `static_cast` directo en el shim sin tablas de mapeo.

Enums cubiertos: `Anchor` (sparse, no contiguo), `ErrorCode`, `EndType`, `JoinType`, `BendType`, `RepetitionType`.

La decisión de espejar los valores se tomó tras el primer intento con mapeo manual, que introducía bugs silenciosos cuando gdstk añadía un variant.

---

## 4. Decisión axiomática: "Miku es read-only"

Un VCS de layouts **lee, compara y atribuye**. No construye geometría. Esta observación simple fue la más influyente del proyecto: recortó la superficie de API a una fracción del total de gdstk.

### 4.1 Qué se excluyó deliberadamente

- Constructores de geometría: `segment`, `arc`, `fillet`, `offset`, `inflate`, `round_corners`.
- Transformaciones mutables: `translate`, `rotate`, `scale`, `mirror`, `apply_repetition`.
- Modificación de árbol: `add`, `remove`, `insert`, `rename_cell`, `set_*`.
- Serialización de escritura: `write_gds`, `write_oasis`, `write_svg`.

### 4.2 Qué sí se expone

- Lectura (`read_gds`, `read_rawcells`).
- Inspección (`get_polygons`, `get_paths`, `get_labels`, `get_references`, `bounding_box`, `area`).
- Operaciones booleanas y XOR (diff geométrico).
- Flatten (para comparación "flat" cuando aplica).
- Metadatos (unit, precision, layers presentes, top cells).

### 4.3 Implicación en la API

La ausencia de métodos mutables simplifica lifetimes: todo `&Cell<'a>` / `&Polygon<'a>` puede prestarse sin preocupación por invalidación interna. No hay `&mut` que proteger.

---

## 5. Separación binding ↔ aplicación (Miku)

**Este punto se identificó tarde en la conversación de diseño** y vale la pena documentarlo porque no era obvio.

### 5.1 Regla

- `gdstk/rust/` contiene **exclusivamente el binding**. No sabe qué es Miku.
- **Miku es un programa aparte** (aún no implementado al momento de este doc) que declarará `gdstk-rs` como dependencia Cargo.
- El binding no debe acumular lógica específica de VCS (attribution, hashing de celdas, algoritmos de matching).

### 5.2 Zona gris: `examples/diff_gds.rs`

Existe [`examples/diff_gds.rs`](../../gdstk/rust/examples/diff_gds.rs) que **demuestra** cómo construir un diff cell-by-cell sobre el binding. Conceptualmente ese código pertenece a Miku, no al binding. Se mantiene como ejemplo porque:

1. Sirve de *smoke test* realista de la API.
2. Documenta el patrón esperado de uso.
3. Acelera el arranque de Miku (copy-paste-ajustar).

**Cuando Miku nazca como crate, `diff_gds.rs` debe migrar allá** y el ejemplo del binding reducirse a algo más trivial (imprimir metadatos, por ejemplo).

---

## 6. Convenciones de API Rust

Se adoptaron convenciones idiomáticas de Rust en lugar de espejar literalmente la API C++:

| Convención | Ejemplo | Rationale |
|---|---|---|
| Getters sin prefijo `get_` | `poly.area()`, no `poly.get_area()` | Idiomático Rust; Clippy lo exige |
| Iteradores lazy sobre `Vec<T>` eager | `cell.polygons()` devuelve `impl Iterator<Item = Polygon<'a>>` | Evita materializar toda la colección; compone con `filter`/`map` sin alocar |
| Lifetimes atados a `Library` | `Cell<'a>`, `Polygon<'a>` comparten `'a` con `Library` | Garantiza que el handle padre vive lo suficiente; previene use-after-free |
| Bounds-check retorna zeros | Acceso fuera de rango → `Point2D { 0.0, 0.0 }` en vez de `panic!` | Un VCS procesando archivos sucios no debe abortar; mejor valor neutro + métrica de error |
| Bridge cxx centralizado | Todo el `#[cxx::bridge]` en [`lib.rs`](../../gdstk/rust/src/lib.rs) | Un único archivo, fácil de auditar. Se intentó dividir y generó conflictos de símbolos |

---

## 7. Normalizaciones y convenciones de datos

### 7.1 `repetition_count = 0` → `1`

`gdstk::Repetition::get_count()` devuelve **0** cuando el tipo de repetición es `RepetitionType::None`. Nuestro shim **normaliza ese 0 a 1** (la instancia origen).

Razón: permite al código Rust escribir `for i in 0..rep.count()` sin casos especiales. Con el 0 nativo habría que ramificar manualmente cada iteración.

Tradeoff: se pierde la distinción "no-repetition" vs "repetition-de-uno" si alguna vez importara. No importa para Miku.

### 7.2 Bounds-check no-panicking

Ya mencionado en §6: todos los accesos indexados devuelven valores neutros (`Point2D::zero()`, `BoundingBox::empty()`) en vez de romper. Decisión consistente con la naturaleza de un VCS que procesa inputs poco confiables.

### 7.3 `const_cast` localizado en el shim

gdstk tiene métodos conceptualmente read-only que **no están marcados `const`**: `get_offsets`, `bounding_box`, `get_polygons`, entre otros. Desde el shim se llaman sobre handles que Rust garantiza inmutables (`&Library`, `&Cell`).

Solución: `const_cast<gdstk::Library*>(...)` en el shim, **siempre con comentario justificatorio** indicando por qué es seguro. Es seguro porque:

1. El lifetime Rust asegura que no hay concurrencia de escritores.
2. Los métodos aludidos son de lectura pura (inspección visual del código gdstk lo confirma para la v1.0.0).
3. La alternativa —proponer patches `const` upstream— es válida pero lenta; se deja como mejora futura.

---

## 8. Plataforma y build

### 8.1 Target soportado

**Windows + vcpkg + MSVC (VS2019 BuildTools o superior).** Se decidió no dispersar el esfuerzo:

| Plataforma / Toolchain | Estado | Razón |
|---|---|---|
| Windows + MSVC + vcpkg (dynamic) | **Soportado** | Entorno del equipo |
| Windows + MinGW | No soportado | ABI C++ incompatible con las DLL de vcpkg MSVC |
| Windows + vcpkg triplet estático (`x64-windows-static`) | No soportado *aún* | Complica la integración de Clipper/qhull; evaluar en fase futura |
| Cross-compile (Linux → Windows, etc.) | No soportado | Sin caso de uso presente |
| Linux/macOS nativo | No evaluado | Posible en el futuro; gdstk es portable |

### 8.2 DLLs copiadas manualmente

Los ejemplos y binarios requieren `qhull_r.dll` y `zlib1.dll` presentes junto al `.exe`. **El usuario debe copiarlas manualmente** desde `vcpkg/installed/x64-windows/bin/` a `target/release/examples/` (o equivalente).

Alternativas consideradas y descartadas por ahora:

| Alternativa | Por qué se postergó |
|---|---|
| Triplet estático vcpkg | Rompe build de Clipper/qhull; requiere investigación |
| Auto-copy en `build.rs` | Requiere detectar ruta de vcpkg robustamente; es trabajo no trivial; decidido para fase futura |
| Pedir al usuario que agregue al PATH | Frágil; contamina entorno global |

---

## 9. Scope evolucionado: de 7 a 12 fases

El [roadmap original](./gdstk_rust_bindings_migracion.md) planteaba 7 fases. El scope real creció a **12** por inserción de fases intermedias descubiertas al ejecutar:

| Fase | Origen | Gap descubierto |
|---|---|---|
| 5 | Planificada | Booleans |
| **5.5** | Insertada | Flatten correcto con repetitions anidadas no cubierto por fase 5 |
| 8 | Planificada | Diff geométrico |
| **8.5** | Insertada | Normalización de orientación/winding antes de XOR |
| **8.6** | Insertada | Métricas agregadas (`XorMetrics`) que Miku necesita y el shim no exponía |
| **8.7** | Insertada | Cell-by-cell vs flat tradeoffs documentados y expuestos |
| **8.8** | Insertada | Manejo de references con transformaciones no ortogonales |

### 9.1 Lección extraíble

**Los roadmaps subestiman sistemáticamente el scope de bindings production-ready.** Cada fase que creíamos "cerrada" revelaba un gap honesto al intentar usarla desde el código cliente (`diff_gds.rs`). No es falla de planificación; es la naturaleza del trabajo FFI: el contrato entre dos lenguajes se valida recién al consumirlo.

Recomendación para quien retome: reservar **~50% de buffer** sobre cualquier estimación inicial de fase en bindings, y validar cada fase con un ejemplo consumidor realista antes de declararla cerrada.

---

## 10. Modelo de diff: cell-by-cell vs flat

Ambos enfoques están disponibles en el binding; Miku deberá elegir cuándo usar cada uno.

| Criterio | Cell-by-cell | Flat diff |
|---|---|---|
| Método | Recorrer celdas por nombre, diff geometría local + references | `cell.get_polygons(depth = -1)` antes de diff |
| Velocidad | Rápido (reutiliza estructura del árbol) | Costoso (materializa toda la jerarquía aplanada) |
| Atribución | Precisa (cada diff ligado a una celda) | Pobre (todo vive en una celda virtual) |
| Detección de refactors | Detecta cambios por celda aunque la jerarquía se reordene si nombres coinciden | Invisible a refactors jerárquicos puros |
| Falsos negativos | Si dos librerías tienen celdas equivalentes con nombres distintos, no las empareja | Ninguno geométrico |
| Cuándo usar | **Default** para Miku | Validación cruzada; comparación con librerías de terceros cuyo árbol no coincide |

[`examples/diff_gds.rs`](../../gdstk/rust/examples/diff_gds.rs) usa cell-by-cell como referencia. Flat diff queda expuesto por si Miku lo necesita; **no es el default**.

---

## 11. Decisiones descartadas explícitamente

Registro de caminos que se consideraron y se cerraron, para evitar que alguien los reabra sin contexto:

| Decisión descartada | Razón |
|---|---|
| Reimplementar gdstk en Rust puro | Meses de trabajo, no aporta valor diferencial a Miku |
| Usar `gds21` / `Layout21` | Experimental, sin booleans, abandonaría cobertura GDSII madura |
| Binding Go + cgo | Overhead de FFI 50–100× mayor que Rust+cxx |
| Mantener Python como runtime | GIL bloquea paralelismo; dependencia de runtime en VCS |
| `bindgen` puro sin cxx | Superficie `unsafe` enorme, mismo esfuerzo sin garantías |
| `autocxx` | Menos *safety guarantees*, frágil ante cambios de headers |
| Bridge cxx dividido en múltiples archivos | Conflictos de símbolos; auditoría fragmentada |
| Exponer API mutable de gdstk | Contradice axioma "Miku es read-only" |
| Getters `get_*` estilo C++ | No idiomático Rust |
| `Vec<T>` eager en accesors | Alocación innecesaria; impide composición lazy |
| Panic on out-of-bounds | Inaceptable en VCS que procesa archivos sucios |
| Soporte MinGW | ABI C++ incompatible con vcpkg MSVC |
| Triplet vcpkg estático (por ahora) | Complica build de deps transitivas (Clipper/qhull) |
| Auto-copy de DLLs en `build.rs` (por ahora) | Trabajo no trivial; postergado a fase futura |
| Mantener `repetition_count=0` nativo | Obliga a ramificar cada iteración en el consumidor |
| Patch a gdstk para marcar métodos `const` | Válido pero lento; `const_cast` localizado resuelve hoy |
| `diff_gds.rs` como parte del binding en vez de Miku | Confunde separación de responsabilidades; migrará cuando Miku nazca |

---

## Apéndice A — Referencias cruzadas rápidas

- Roadmap original y fases: [`gdstk_rust_bindings_migracion.md`](./gdstk_rust_bindings_migracion.md)
- Borrador parcial previo: [`gdstk_rust_decisiones.md`](./gdstk_rust_decisiones.md)
- Stack y lenguaje (análisis comparativo): [`lenguajes_y_stack.md`](./lenguajes_y_stack.md)
- Bridge cxx central: [`gdstk/rust/src/lib.rs`](../../gdstk/rust/src/lib.rs)
- Shim C++ (implementación): [`gdstk/rust/src/shims.cpp`](../../gdstk/rust/src/shims.cpp)
- Ejemplo consumidor (diff cell-by-cell): [`gdstk/rust/examples/diff_gds.rs`](../../gdstk/rust/examples/diff_gds.rs)

## Apéndice B — Checklist al retomar el proyecto

1. Verificar que VS2019 BuildTools (o superior) + vcpkg están instalados.
2. `vcpkg install gdstk` (o equivalente según cómo esté enganchado).
3. `cargo build --release` desde `gdstk/rust/`.
4. Copiar `qhull_r.dll` y `zlib1.dll` a `target/release/examples/`.
5. Correr `cargo run --release --example diff_gds -- <a.gds> <b.gds>` para validar el stack completo.
6. Revisar el estado del roadmap en `gdstk_rust_bindings_migracion.md` antes de abrir nuevas fases.
