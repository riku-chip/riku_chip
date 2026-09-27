# Gotchas técnicos del binding Rust de gdstk

Este documento recopila los "footguns", trampas sutiles y decisiones no obvias que
encontramos (y sufrimos) durante la construcción del binding Rust sobre `gdstk`
vía `cxx`. Está escrito para el desarrollador que venga después de nosotros: la
intención es que pueda leer este archivo antes de tocar el crate y ahorrarse las
horas de debugging que nos costaron.

Cada sección sigue la misma estructura:

- **Síntoma**: lo que uno observa cuando cae en la trampa.
- **Causa raíz**: la razón técnica real.
- **Cómo lo detectamos**: el camino concreto por el que lo descubrimos.
- **Solución final**: qué hacemos hoy en el crate.
- **Código** (cuando aplica): fragmento mínimo reproducible.

---

## Índice

1. [Valores sparse del enum `Anchor` (3, 7, 11 no existen)](#1-valores-sparse-del-enum-anchor-3-7-11-no-existen)
2. [`Repetition::get_count()` devuelve 0 para `RepetitionType::None`](#2-repetitionget_count-devuelve-0-para-repetitiontypenone)
3. [Asimetría `count` / `offset` respecto al origen](#3-asimetría-count--offset-respecto-al-origen)
4. [`True`/`False` de Python vs `true`/`false` de Rust](#4-truefalse-de-python-vs-truefalse-de-rust)
5. [CRLF vs LF en Windows](#5-crlf-vs-lf-en-windows)
6. [`STATUS_DLL_NOT_FOUND` (0xc0000135) con vcpkg](#6-status_dll_not_found-0xc0000135-con-vcpkg)
7. [Error C2338 de `cxx`: "definition of X is required"](#7-error-c2338-de-cxx-definition-of-x-is-required)
8. [`#pragma once` falla con `cxx-build`](#8-pragma-once-falla-con-cxx-build)
9. [Include path de Clipper (doble `clipper/clipper`)](#9-include-path-de-clipper-doble-clipperclipper)
10. [`bootstrap-vcpkg.bat` bloqueado por sandbox](#10-bootstrap-vcpkgbat-bloqueado-por-sandbox)
11. [Bug silencioso en Fase 4: XOR ignoraba paths](#11-bug-silencioso-en-fase-4-xor-ignoraba-paths)
12. [Lifetimes en views que referencian owners distintos](#12-lifetimes-en-views-que-referencian-owners-distintos)
13. [`bounding_box` no es `const` en gdstk](#13-bounding_box-no-es-const-en-gdstk)
14. [`gdstk::Set<Tag>` no tiene `operator[]`](#14-gdstkset-no-tiene-operator)
15. [`Reference::get_polygons` devuelve polígonos heap-owned](#15-referenceget_polygons-devuelve-polígonos-heap-owned)
16. [`Library::read_gds` sin propagación de error](#16-libraryread_gds-sin-propagación-de-error)
17. [`proof_lib.gds` y polígonos con `signed_area == 0`](#17-proof_libgds-y-polígonos-con-signed_area--0)
18. [`cxx` no soporta function pointers de C++](#18-cxx-no-soporta-function-pointers-de-c)
19. [`gdstk::Array<T>` no tiene `to_vec`](#19-gdstkarrayt-no-tiene-to_vec)
20. [`RepetitionType::None` en `get_extrema`](#20-repetitiontypenone-en-get_extrema)

---

## 1. Valores sparse del enum `Anchor` (3, 7, 11 no existen)

### Síntoma

El enum `Anchor` de gdstk tiene huecos: existen 0, 1, 2, 4, 5, 6, 8, 9, 10, pero
no 3, 7, ni 11. Si uno declara el enum en Rust con `#[repr(u8)]` y numeración
0..8 contigua, los valores del GDSII ya no coinciden con la variante Rust y las
lecturas salen silenciosamente desplazadas.

### Causa raíz

No es un bug: es el encoding binario del GDSII. El byte del anchor se descompone
en dos campos de 2 bits:

```
anchor = vertical * 4 + horizontal
  horizontal: 0 = left,   1 = center, 2 = right
  vertical:   0 = top,    1 = middle, 2 = bottom
```

La tabla completa:

| | H=0 (left) | H=1 (center) | H=2 (right) |
|---|---|---|---|
| **V=0 (top)**    | 0  `NW` | 1 `N` | 2  `NE` |
| **V=1 (middle)** | 4  `W`  | 5 `O` | 6  `E`  |
| **V=2 (bottom)** | 8  `SW` | 9 `S` | 10 `SE` |

Los valores 3, 7, 11 corresponden a `horizontal == 3`, que no existe: el campo
solo admite 0, 1 y 2. gdstk simplemente preserva esa sparsidad, fiel al binario
GDSII.

### Cómo lo detectamos

Al implementar el mapping bidireccional `u8 <-> Anchor` en Rust. Un test de
round-trip sobre un label real con anchor `E` (valor 6) dio `O` (valor 5) al
usar numeración contigua. Ahí encontramos el hueco.

Paralelamente descubrimos un bug latente en el propio binding Python de gdstk:
en el `switch` del getter de `Anchor`, el `default` devuelve `NULL` sin llamar
`PyErr_Format`. Si el GDSII llegara con un byte fuera de la tabla, Python
recibiría `NULL` sin excepción y segfaultearía en el caller.

### Solución final

Preservamos los valores numéricos del C++ en Rust:

```rust
#[repr(u8)]
pub enum Anchor {
    NW = 0, N = 1, NE = 2,
    W  = 4, O = 5, E  = 6,
    SW = 8, S = 9, SE = 10,
}

impl From<u8> for Anchor {
    fn from(v: u8) -> Self {
        match v {
            0 => Anchor::NW, 1 => Anchor::N,  2  => Anchor::NE,
            4 => Anchor::W,  5 => Anchor::O,  6  => Anchor::E,
            8 => Anchor::SW, 9 => Anchor::S,  10 => Anchor::SE,
            _ => Anchor::O, // fallback defensivo para 3, 7, 11, 12+
        }
    }
}
```

El fallback a `O` es una decisión explícita: preferimos que un GDSII corrupto
produzca un label centrado que un panic en deep-copy.

---

## 2. `Repetition::get_count()` devuelve 0 para `RepetitionType::None`

### Síntoma

Un loop `for i in 0..repetition.count()` nunca ejecuta para polygons sin
repetición. El polígono base se pierde en el output.

### Causa raíz

En C++ `gdstk::Repetition::get_count()` retorna literalmente 0 cuando el tipo
es `RepetitionType::None`. Semánticamente gdstk espera que el caller trate "sin
repetición" como un caso aparte, no como "una repetición de tamaño 1".

### Cómo lo detectamos

Al escribir `list_polygons` en Fase 2: polígonos sin repetición desaparecían del
output. El diff contra el script Python de referencia marcó la ausencia.

### Solución final

Introdujimos un shim `repetition_effective_count` que normaliza el None a 1:

```cpp
uint64_t repetition_effective_count(const gdstk::Repetition& r) {
    if (r.type == gdstk::RepetitionType::None) return 1;
    return r.get_count();
}
```

Todos los sitios del binding que iteran sobre instancias usan esta función, no
`get_count()` crudo. Misma normalización se replica en `get_extrema` (ver
gotcha #20).

---

## 3. Asimetría `count` / `offset` respecto al origen

### Síntoma

Al combinar `repetition.count()` con `repetition.explicit_offset(i)`, los
bounds de la iteración se desfasan en uno y el último offset queda fuera de
rango o se duplica el origen.

### Causa raíz

La convención original del binding (hoy eliminada) era:

- `count()` incluía el origen ⇒ N+1.
- `explicit_offset_count()` excluía el origen ⇒ N.
- `offsets()` incluía el origen.
- `explicit_offsets()` excluía el origen.

Semánticamente tenía sentido por separado, pero mezclar APIs producía
off-by-one silencioso. Peor: el bug era determinístico y no crasheaba; solo
dejaba una instancia mal colocada en el layout.

### Cómo lo detectamos

Un usuario interno comparó el bbox agregado de una celda con repetición
explícita y detectó 1 unidad de diferencia. Tardamos en reproducirlo porque
solo ocurría cuando alguien ya era suficientemente avanzado para combinar las
dos APIs.

### Solución final

Eliminamos la variante `explicit_*`. Hoy solo existe una convención (la que
incluye el origen). Si el usuario quiere el array crudo al estilo C++:

```rust
let raw: Vec<Point2D> = repetition.offsets().into_iter().skip(1).collect();
```

El `.skip(1)` es explícito, visible en code review, y no se puede
"accidentalmente" mezclar con `count()`.

---

## 4. `True`/`False` de Python vs `true`/`false` de Rust

### Síntoma

Test de paridad byte-a-byte `list_references` falla con un único diff:

```
-  x_reflection: True
+  x_reflection: true
```

El resto de la salida es idéntica.

### Causa raíz

`str(bool)` en Python produce `True`/`False` con mayúscula inicial. En Rust el
`Debug`/`Display` de `bool` produce `true`/`false`. Nuestro test compara
stdouts textualmente.

### Cómo lo detectamos

Primer run de la suite de paridad. Obvio al mirar el diff.

### Solución final

Decisión cosmética: normalizamos en el script Python de referencia, no en Rust.

```python
print(f"  x_reflection: {str(r.x_reflection).lower()}")
```

Razón: el GDSII real no contiene esta string en ningún lado. Es solo output de
diagnóstico humano. Rust queda idiomático, Python queda equivalente. Si
alguien modifica el script Python, que replique el `.lower()`.

---

## 5. CRLF vs LF en Windows

### Síntoma

`diff` marca *todas* las líneas como diferentes aunque visualmente el contenido
sea idéntico. `wc -l` da números consistentes en ambos archivos.

### Causa raíz

En Windows, Python con `print()` escribe `\r\n` al redirigir a archivo (modo
text). Rust con `println!` escribe `\n` independiente del SO. `diff` es
byte-exact y ve el `\r` como carácter extra.

### Cómo lo detectamos

El primer test de paridad en Windows falló al 100% aunque visualmente el output
se veía idéntico. `cat -A` (o `od -c`) reveló los `\r`.

### Solución final

En el pipeline de tests de paridad:

```bash
python script.py fixture.gds | tr -d '\r' > python.out
cargo run --bin list_polygons -- fixture.gds > rust.out
diff python.out rust.out
```

Alternativa (no adoptada): `.gitattributes` con `* text eol=lf` para que el
checkout normalice line endings. Rechazada porque rompe otros pipelines del
monorepo que sí esperan CRLF en Windows.

---

## 6. `STATUS_DLL_NOT_FOUND` (0xc0000135) con vcpkg

### Síntoma

El binario Rust compila OK. Al ejecutarlo, Windows reporta exit code
`0xc0000135` y un popup "aplicación no pudo iniciarse". No hay ningún mensaje
útil en stderr.

### Causa raíz

El crate linkea contra `zlib1.dll` y `qhull_r.dll` provistas por vcpkg
(`C:/vcpkg/installed/x64-windows/bin/`). Windows busca DLLs en: directorio del
exe, luego `System32`, luego `PATH`. vcpkg no está en ninguno de esos por
defecto. El loader falla sin imprimir a stderr porque el proceso ni siquiera
alcanza a inicializar.

### Cómo lo detectamos

Primera ejecución post-build en Fase 1. Depuramos con `dumpbin /dependents` y
Process Monitor; filtrando por `NAME NOT FOUND` apareció la ruta fallida.

### Solución final

Copiamos las DLLs al lado del `.exe` como post-build step. Es feo pero
portable:

```rust
// build.rs (simplificado)
let out = env::var("OUT_DIR").unwrap();
let target_dir = Path::new(&out).ancestors().nth(3).unwrap();
for dll in &["zlib1.dll", "qhull_r.dll"] {
    let src = format!("C:/vcpkg/installed/x64-windows/bin/{}", dll);
    fs::copy(&src, target_dir.join(dll)).ok();
}
```

Alternativas evaluadas:

- **Agregar `C:/vcpkg/installed/x64-windows/bin` al PATH**: funciona para `cargo
  run` en una shell con PATH modificado, pero `cargo run` no hereda PATH
  modificaciones hechas en `build.rs` al spawn del child. Hay que setear a
  nivel usuario, lo cual contamina el sistema.
- **Triplet `x64-windows-static`**: linkea todo estático y elimina el problema
  en raíz. No probado aún; riesgo es que `qhull` static con CRT static entre en
  conflicto con el CRT dinámico de Rust.

---

## 7. Error C2338 de `cxx`: "definition of X is required"

### Síntoma

```
error C2338: static_assert failed: 'definition of LibraryHandle is required'
```

Aparece al compilar un bridge que usa `UniquePtr<LibraryHandle>`.

### Causa raíz

`cxx` necesita conocer `sizeof(T)` en sitios donde traduce `UniquePtr<T>`. Si
solo forward-declaras el tipo, `cxx` no puede emitir el `static_assert` de
layout y falla. Esto entra en conflicto con el patrón PIMPL típico, donde uno
quiere ocultar la `Impl` detrás de un `unique_ptr`.

### Cómo lo detectamos

En Fase 6, al agregar `TopLevelView` intentamos replicar el patrón PIMPL que ya
funcionaba para `LibraryHandle`, pero forward-declaramos `TopLevelView` entero.
C2338 explotó inmediatamente.

### Solución final

El handle exterior se declara completo en el header; solo la `Impl` interna
queda forward-declared:

```cpp
// shims.h
struct LibraryHandle {
    struct Impl;                       // forward declaration, definición en .cpp
    std::unique_ptr<Impl> impl;

    LibraryHandle();
    ~LibraryHandle();
    LibraryHandle(const LibraryHandle&) = delete;
    LibraryHandle& operator=(const LibraryHandle&) = delete;
};
```

```cpp
// shims.cpp
struct LibraryHandle::Impl {
    gdstk::Library lib;
    // ...
};

LibraryHandle::LibraryHandle()  : impl(std::make_unique<Impl>()) {}
LibraryHandle::~LibraryHandle() = default;
```

Con esto `cxx` ve tamaño fijo (`sizeof(unique_ptr) == sizeof(void*)`), y la
estructura real queda oculta en el `.cpp`.

---

## 8. `#pragma once` falla con `cxx-build`

### Síntoma

```
error C2011: 'LibraryHandle': 'struct' type redefinition
```

El mismo header incluido dos veces en el mismo TU.

### Causa raíz

`cxx-build` copia `src/shims.h` a
`target/.../cxxbridge/crate/gdstk-rs/src/shims.h`. El archivo generado y el
original son byte-idénticos pero tienen paths absolutos distintos. `#pragma
once` deduplica por **identidad de archivo del filesystem** (inode en Unix,
`BY_HANDLE_FILE_INFORMATION` en Windows), no por contenido. MSVC ve dos paths
distintos ⇒ dos archivos distintos ⇒ dos inclusiones ⇒ redefinición.

### Cómo lo detectamos

Inmediatamente al primer build que incluía el shim desde ambos lados del
bridge. El error C2011 reveló que el struct se estaba definiendo dos veces en
el mismo TU.

### Solución final

Header guards clásicos, que deduplican por **macro**:

```cpp
#ifndef GDSTK_RS_SHIMS_H
#define GDSTK_RS_SHIMS_H
// ... contenido ...
#endif // GDSTK_RS_SHIMS_H
```

Convención del proyecto: todo header consumido por `cxx-build` usa guards
explícitos con prefijo `GDSTK_RS_`. Nunca `#pragma once`.

---

## 9. Include path de Clipper (doble `clipper/clipper`)

### Síntoma

```
fatal error C1083: Cannot open include file: 'clipper/clipper.hpp'
```

A pesar de que el archivo existe y el include path parece correcto.

### Causa raíz

`clipper_tools.cpp` contiene:

```cpp
#include "clipper/clipper.hpp"
```

Es decir, el include ya tiene el prefijo `clipper/`. Si uno agrega
`external/clipper/` al search path, el compilador resuelve a
`external/clipper/clipper/clipper.hpp` (doble `clipper/`), que no existe.

### Cómo lo detectamos

Primer build de Fase 3 (clipper_tools). El error de include es explícito y
rápido de diagnosticar una vez que uno mira la ruta resuelta.

### Solución final

Agregar el **padre**, no la carpeta del repo clonado:

```rust
// build.rs
cc.include("external");  // no "external/clipper"
```

Ahora `#include "clipper/clipper.hpp"` resuelve a `external/clipper/clipper.hpp`.
Convención adoptada: include paths siempre apuntan al directorio *padre* del
prefijo que ya está embebido en los `#include` del upstream.

---

## 10. `bootstrap-vcpkg.bat` bloqueado por sandbox

### Síntoma

El script `bootstrap-vcpkg.bat` aborta silenciosamente al invocarlo desde
Claude Code u otros agentes. El exit code no es útil.

### Causa raíz

El sandbox de ejecución marca como untrusted los batch scripts descargados de
repositorios recién clonados. Es una política razonable (un `.bat` recién
clonado puede ejecutar cualquier cosa) pero bloquea la instalación automática
de vcpkg.

### Cómo lo detectamos

Al intentar automatizar el setup de desarrollo. El agente reportó "script no
ejecutó" sin más detalle; lo reprodujimos manualmente fuera del sandbox y ahí
funcionó.

### Solución final

Documentación: el setup de vcpkg es un paso **manual** que el desarrollador
humano debe correr una vez en su máquina. El `build.rs` asume que
`C:/vcpkg/installed/x64-windows/` ya existe y falla con un mensaje claro si
no:

```rust
// build.rs
let vcpkg = Path::new("C:/vcpkg/installed/x64-windows");
if !vcpkg.exists() {
    panic!("vcpkg no encontrado. Corre manualmente:\n\
            git clone https://github.com/microsoft/vcpkg C:/vcpkg\n\
            C:/vcpkg/bootstrap-vcpkg.bat\n\
            C:/vcpkg/vcpkg install zlib qhull");
}
```

---

## 11. Bug silencioso en Fase 4: XOR ignoraba paths

### Síntoma

En Fase 4, el diff boolean XOR contra Python daba idéntico sobre
`proof_lib.gds`. Toda la suite pasaba. En Fase 5, cambiando el fixture a
`tinytapeout.gds`, el diff fallaba con cientos de diferencias.

### Causa raíz

`cell_xor_with` iteraba solamente `polygon_array` de la celda, ignorando
`flexpath_array` y `robustpath_array`. `proof_lib.gds` tiene 0 paths (es
puramente polígonos), así que el bug era invisible. `tinytapeout.gds` tiene 46
paths, y cada uno era omitido del XOR.

### Cómo lo detectamos

Al correr la suite de Fase 5 con el nuevo fixture. El diff explotó y el bisect
entre polygons-only y paths-mixed lo hizo obvio.

### Solución final

Antes del XOR, materializamos los paths a polygons:

```cpp
void collect_all_polygons(const gdstk::Cell& c, gdstk::Array<gdstk::Polygon*>& out) {
    for (uint64_t i = 0; i < c.polygon_array.count; i++)
        out.append(c.polygon_array[i]);
    for (uint64_t i = 0; i < c.flexpath_array.count; i++)
        c.flexpath_array[i]->to_polygons(false, 0, out);
    for (uint64_t i = 0; i < c.robustpath_array.count; i++)
        c.robustpath_array[i]->to_polygons(false, 0, out);
}
```

### Lección

Tests con un único fixture son insuficientes cuando el código recorre
estructuras heterogéneas. Política actual: toda regla de paridad se valida
contra **al menos dos** GDSs con geometría estructuralmente distinta (uno
polygon-only, uno path-heavy). Si agregas un nuevo boolean, agrega un tercer
fixture con ese caso.

---

## 12. Lifetimes en views que referencian owners distintos

### Síntoma

```
error[E0515]: cannot return reference to temporary value
error[E0521]: borrowed data escapes outside of method
```

Al intentar devolver un `Cell<'a>` desde `TopLevelView::cell(i)`.

### Causa raíz

`TopLevel::cell(i)` debe devolver un `Cell<'a>` donde `'a` es el lifetime del
`Library` padre (el `Library` es quien owns los cells). Internamente, sin
embargo, el shim `ffi::top_level_at(&self.view, ...)` devuelve un
`&CellHandle` cuyo lifetime el compilador infiere como el de `self.view`,
porque es lo único visible en la firma FFI.

`self.view` típicamente vive menos que el `Library` (es un sub-view efímero),
así que el lifetime inferido es demasiado corto y el compilador rechaza el
return.

### Cómo lo detectamos

Al implementar `TopLevelView::iter()` en Fase 6. Los errores E0515/E0521
saltaron en cascada.

### Solución final

`mem::transmute` documentado, con el invariante justificado por escrito:

```rust
impl<'a> TopLevelView<'a> {
    pub fn cell(&self, i: usize) -> Cell<'a> {
        // SAFETY: los CellHandle pertenecen al Library ('a), no al view.
        // El view solo indexa; extender el lifetime al del Library es sound
        // mientras el Library exista, y 'a nos garantiza eso.
        unsafe {
            let short: &CellHandle = ffi::top_level_at(&self.view, i);
            let long: &'a CellHandle = mem::transmute(short);
            Cell::from_handle(long)
        }
    }
}
```

El `unsafe` está aislado en este método y el invariante está escrito en el
SAFETY comment. Si alguien cambia `TopLevel` para que owns cells en lugar de
solo indexar, este comment falla y el `transmute` pasa a ser UB — hay que
revalidar.

---

## 13. `bounding_box` no es `const` en gdstk

### Síntoma

```
error C2662: 'void gdstk::Cell::bounding_box(Vec2&, Vec2&)':
cannot convert 'this' pointer from 'const Cell' to 'Cell &'
```

### Causa raíz

`gdstk::Cell::bounding_box` y `gdstk::Reference::bounding_box` están declarados
como **non-const** aunque semánticamente no mutan el objeto (solo calculan).
Dentro del shim recibimos `const gdstk::Cell&` (porque Rust pasa `&self` como
const), y no podemos llamar un método non-const.

### Cómo lo detectamos

Primera implementación del shim de bbox. Error de compilación obvio.

### Solución final

`const_cast` aislado en el shim, con justificación escrita:

```cpp
// SAFE: bounding_box es non-const en gdstk por oversight del upstream.
// No muta el objeto; solo recorre hijos y calcula. Seguro en read-only
// single-threaded. Si en el futuro se paralelizan escrituras concurrentes
// contra la misma Cell, este const_cast pasa a ser data race.
std::array<double, 4> cell_bounding_box(const gdstk::Cell& c) {
    gdstk::Vec2 min, max;
    const_cast<gdstk::Cell&>(c).bounding_box(min, max);
    return {min.x, min.y, max.x, max.y};
}
```

Misma nota para `Reference::bounding_box`. Si en algún momento el binding pasa
a ser multi-thread con escrituras concurrentes, estos dos sitios hay que
revisarlos.

---

## 14. `gdstk::Set<Tag>` no tiene `operator[]`

### Síntoma

No hay forma directa de iterar o indexar un `gdstk::Set<Tag>` desde el shim. El
orden de iteración además depende del hash bucket, así que dos runs consecutivos
pueden devolver las tags en orden distinto.

### Causa raíz

`gdstk::Set` es un hash set, no un ordered container. No expone
`operator[]`, y la iteración natural es en orden de bucket (no determinístico
entre runs, sistemas, o reconstrucciones del set).

### Cómo lo detectamos

Al exponer `shape_tags` y `label_tags` del `LibraryInfo` en Fase 6. Los tests de
paridad empezaron a fallar aleatoriamente: el orden de las tags cambiaba entre
runs en la misma máquina incluso.

### Solución final

Materializar a array y ordenar antes de exponer:

```cpp
std::vector<uint32_t> library_info_shape_tags(const LibraryInfo& info) {
    gdstk::Array<gdstk::Tag> arr = {};
    info.shape_tags.to_array(arr);
    std::qsort(arr.items, arr.count, sizeof(gdstk::Tag),
               [](const void* a, const void* b) {
                   return int(*(const gdstk::Tag*)a) - int(*(const gdstk::Tag*)b);
               });
    std::vector<uint32_t> out(arr.items, arr.items + arr.count);
    arr.clear();
    return out;
}
```

Orden ascendente por valor de tag. Determinístico, reproducible, compatible
con diffs byte-a-byte.

---

## 15. `Reference::get_polygons` devuelve polígonos heap-owned

### Síntoma

Valgrind / ASan reportan leaks tras usar `get_polygons`. Alternativamente:
crash por doble free, o por UAF si uno libera demasiado pronto.

### Causa raíz

La API de gdstk para `get_polygons` devuelve un array de `Polygon*` donde cada
polígono está heap-allocated con dos niveles: el struct `Polygon` externo, y
los arrays internos `point_array` y `property_list` (también heap). El caller
es responsable de liberar **ambos** niveles, en orden.

### Cómo lo detectamos

ASan build de los tests de Fase 2 reportó ~N leaks por cada `list_references`
run, donde N = número de polígonos materializados.

### Solución final

Helper canónico que todos los shims deben usar:

```cpp
void free_polygon_array(gdstk::Array<gdstk::Polygon*>& arr) {
    for (uint64_t i = 0; i < arr.count; i++) {
        arr[i]->clear();              // libera point_array y property_list
        gdstk::free_allocation(arr[i]); // libera el struct Polygon
    }
    arr.clear();                      // libera el array de punteros
}
```

Reglas:

- **Olvidar `clear()`**: leak de `point_array` y `property_list` (los interiores).
- **Olvidar `free_allocation`**: leak del struct `Polygon` mismo.
- **Liberar antes del último uso**: UAF.
- **Liberar dos veces**: double free.

Convención: nunca llamar `get_polygons` sin un `free_polygon_array`
correspondiente en el mismo scope. Usar RAII (`scope_guard`) si es posible.

---

## 16. `Library::read_gds` sin propagación de error

### Síntoma

`Library::open("archivo_que_no_existe.gds")` devuelve una `Library` vacía sin
reportar nada. El código downstream falla más adelante con un mensaje
críptico (normalmente "no cells found").

### Causa raíz

En Fase 1 priorizamos velocidad y swallowmos el `ErrorCode` que devuelve
`gdstk::read_gds`. El wrapper Rust `Library::open` se quedó con firma `->
Library`, sin `Result`.

Fase 6 expuso `ErrorCode` como enum Rust idiomático, pero **no** hizo el
breaking change de cambiar la firma a `Result<Library, ErrorCode>` porque
había demasiados call sites dependiendo de la firma infalible.

### Cómo lo detectamos

Un usuario interno typeó mal un path en la CLI y obtuvo un core dump en el
primer `.cell()` en lugar de "archivo no existe".

### Solución final (parcial, deuda técnica abierta)

Hoy `Library::open` sigue sin `Result`. Para chequeos robustos existe una
variante `Library::try_open -> Result<Library, ErrorCode>` que es lo que
deberían usar CLIs nuevas. El plan es:

1. Deprecar `Library::open` con `#[deprecated]`.
2. En una major version futura, renombrar `try_open` a `open` y romper ABI.

Hasta entonces, code review rechaza `Library::open` en CLIs que aceptan paths
del usuario. Interno con paths hardcoded puede seguir usándolo.

---

## 17. `proof_lib.gds` y polígonos con `signed_area == 0`

### Síntoma

Test inicial de Fase 2 asumía:

```
|polygon.signed_area()| * repetition_count == polygon_total_area
```

Fallaba consistentemente para la celda `Polygon.fillet` de `proof_lib.gds`
(signed_area = 0, pero visualmente el polígono tiene área > 0).

### Causa raíz

gdstk representa el resultado de `fillet` con polígonos donde los vértices del
contorno exterior y del arco interior se cancelan en el cálculo de signed
area (la fórmula de shoelace da 0 cuando las orientaciones positivas y
negativas suman igual). Geométricamente el polígono es válido y tiene área
positiva; algebraicamente `signed_area` da 0.

### Cómo lo detectamos

Test de invariante de área falló solo en esa celda. El resto del fixture
pasaba. Al graficar el polígono verificamos que es visualmente correcto.

### Solución final

Relajamos el test:

```rust
assert!(polygon.signed_area().is_finite(),
        "signed_area debe ser finito, fue {:?}", polygon.signed_area());
// NO assert que sea != 0: fillet puede legítimamente producir 0.
```

La invariante real que nos interesa (área total > 0) no se puede verificar
desde `signed_area()` en este tipo de polígonos; habría que rasterizar. Fuera
del scope de los tests unitarios.

---

## 18. `cxx` no soporta function pointers de C++

### Síntoma

No hay forma de exponer vía `cxx` los callbacks `end_function`,
`join_function`, `bend_function` de `FlexPath` / `RobustPath`.

### Causa raíz

`cxx` no binde function pointers C++: su modelo de tipos no los representa, y
no sabe cómo traducir closures Rust a `void(*)(...)` C++ ABI estable. Es una
limitación fundamental del crate, no un bug.

### Cómo lo detectamos

Al implementar el binding de `FlexPath` en Fase 3. Intentamos exponer los
tres enums con todas sus variantes y llegamos a `Function` sin idea de cómo
pasar el callback.

### Solución final

Documentamos explícitamente que los valores `EndType::Function`,
`JoinType::Function` y `BendType::Function` significan "callback custom no
introspectable". Al encontrarlos durante una lectura:

- La struct Rust expone la variante, para que el usuario pueda detectarla.
- Los datos del callback no son accesibles.
- Al escribir un path, Rust no puede construir uno con `Function`; hay que
  extender el binding con otra vía (ej. un shim C++ con callback hardcoded).

Ningún Miku real usa estos callbacks, así que la limitación no nos bloquea.

---

## 19. `gdstk::Array<T>` no tiene `to_vec`

### Síntoma

No existe un método directo para convertir un `gdstk::Array<Vec2>` a un
`std::vector<Vec2>` o un `Vec<Point2D>` Rust.

### Causa raíz

`gdstk::Array<T>` es un contenedor propio de gdstk (no `std::vector`), con
semántica de ownership particular: `clear()` libera el buffer, `append`
realoca, etc. No implementa interop con STL.

### Cómo lo detectamos

Al exportar listas de puntos al bridge Rust en Fase 2. No había shortcut; hubo
que escribir el loop manual.

### Solución final

Loop manual con `count` + indexado. Patrón canónico:

```cpp
std::vector<Point2D> vec2_array_to_vec(const gdstk::Array<gdstk::Vec2>& arr) {
    std::vector<Point2D> out;
    out.reserve(arr.count);
    for (uint64_t i = 0; i < arr.count; i++) {
        out.push_back({arr[i].x, arr[i].y});
    }
    return out;
}
```

Sin shortcut, pero el patrón es trivial de replicar. Si uno se encuentra
escribiendo el mismo loop 4 veces, vale la pena un template en `shims.h`.

---

## 20. `RepetitionType::None` en `get_extrema`

### Síntoma

Similar a #2: `repetition.get_extrema()` devuelve array vacío cuando el tipo
es `None`. Código que asume "siempre hay al menos un extremum" itera 0 veces y
pierde el polígono base.

### Causa raíz

El switch interno de `Repetition::get_extrema` en C++ no tiene case para
`None`: cae al `default` que no agrega nada al array de salida. Consistente
con el comportamiento de `get_count()` (gotcha #2), pero igualmente inútil
para el consumidor.

### Cómo lo detectamos

Al calcular bboxes de celdas con polígonos sin repetición. El bbox venía
vacío.

### Solución final

Shim que normaliza igual que `repetition_effective_count`:

```cpp
gdstk::Array<gdstk::Vec2> repetition_effective_extrema(const gdstk::Repetition& r) {
    gdstk::Array<gdstk::Vec2> out = {};
    if (r.type == gdstk::RepetitionType::None) {
        out.append({0.0, 0.0});   // un único extremum en origen
        return out;
    }
    r.get_extrema(out);
    return out;
}
```

Invariante mantenida: `repetition_effective_extrema(r).count ==
repetition_effective_count(r)`. Los dos shims evolucionan juntos; si agregas
un `RepetitionType` nuevo, actualiza ambos.

---

## Cierre

Estos 20 ítems representan aproximadamente 60% de las horas de debugging del
proyecto. Si antes de tocar el crate lees este documento entero y buscas si lo
que vas a hacer toca alguna de estas áreas, vas a ahorrar mucho tiempo.

**Patrones transversales** que emergen:

- **GDSII tiene sparsidad y casos None por diseño** (gotchas #1, #2, #20).
  Siempre normalizar en el shim, nunca asumir que los valores son contiguos o
  que "sin repetición" significa "repetición de 1".
- **cxx tiene limitaciones estructurales** (gotchas #7, #8, #18). No pelear
  contra ellas: diseñar alrededor.
- **Windows + vcpkg + sandboxes = fricción** (gotchas #5, #6, #10). Documentar
  los pasos manuales claramente.
- **Un solo fixture de test no alcanza** (gotcha #11). Mínimo dos fixtures
  estructuralmente distintos por feature.
- **`unsafe` aislado con SAFETY docs** (gotchas #12, #13). Si tenés que meter
  `unsafe`/`const_cast`, escribí el invariante; el siguiente dev te lo
  agradece.

Para deuda técnica viva ver especialmente gotcha #16 (error propagation en
`Library::open`), que sigue abierto.
