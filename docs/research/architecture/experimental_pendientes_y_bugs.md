# Experimental, pendientes, descartado y bugs del binding Rust de gdstk

> Documento de referencia para devs futuros que retomen el proyecto. Consolida **todo lo que NO está cerrado**: features experimentales que viven en el repo pero conceptualmente no son parte del binding, trabajo pendiente con triggers conocidos, decisiones descartadas permanentemente, bugs históricos con sus correcciones, ambigüedades abiertas y lecciones aprendidas a lo largo de las 12 fases de implementación.

## Introducción

El binding Rust de gdstk (`gdstk/rust/`) nació con un objetivo acotado: **exponer la lectura de archivos GDSII** desde Rust con cobertura suficiente para alimentar a **Miku**, el VCS/diff tool para layouts de chips. A lo largo de 12 fases de implementación, el scope creció de forma orgánica y se tomaron decisiones que vale la pena documentar explícitamente para que quien herede el código sepa:

- Qué piezas del repo **no son binding puro** sino prototipos de Miku que se quedaron ahí por conveniencia.
- Qué features **faltan deliberadamente** y bajo qué condición conviene implementarlos.
- Qué se **descartó de raíz** y por qué no vale la pena reabrir la discusión sin un cambio de contexto fuerte.
- Qué **bugs reales** nos tropezaron, cómo los encontramos y cómo los arreglamos (para no repetirlos).
- Qué decisiones quedaron **sin cerrar** porque no había suficiente información para decidir.
- Qué **patrones de proceso** aprendimos y que conviene aplicar a futuras extensiones.

Si vas a tocar este binding, **lee este documento antes** de modificar `shim.cpp`, `lib.rs` o los ejemplos. Te va a ahorrar horas.

---

## 1. Experimental — hecho pero no pertenece al binding puro

Los siguientes artefactos viven físicamente dentro de `gdstk/rust/` pero **conceptualmente pertenecen a Miku** (la aplicación futura), no al binding. Se preservan como referencia, benchmarks históricos y proto-fixtures, no como features soportadas del crate.

### 1.1 Ejemplos que son prototipos de aplicación, no demos de binding

| Archivo | Qué es | Debería vivir en |
|---------|--------|------------------|
| `examples/diff_gds.rs` | CLI de diff con modelo cell-by-cell. Prototipo de `miku diff`. | Repo de Miku |
| `examples/roundtrip.rs` | Valida `open → write → open → compare`. Útil como test de integridad, no como feature del binding. | `tests/` de Miku o `gdstk/rust/tests/` |
| `examples/count_many.rs` | Benchmark ad-hoc de paralelismo (single-thread). | Archivo histórico / benchmarks de Miku |
| `examples/count_many_fair.rs` | Variante "fair" del benchmark (mismo workload por thread). | Igual |
| `examples/count_many_parallel.rs` | Versión con Rayon para comparar speed-up. | Igual |
| `examples/gds_info_bench.rs` | Compara `gds_info` vs `read_gds` completo. Demo de la optimización de parsing lazy. | Notas de performance |
| `examples/make_modified.py` | Helper Python para crear fixtures de diff (mueve un polygon). | Repo de Miku / fixtures |
| `examples/make_path_modified.py` | Helper Python para crear fixtures con paths modificados. | Repo de Miku / fixtures |
| `examples/bench_python.py` | Comparativa end-to-end Python gdstk vs Rust. | Archivo histórico |
| `examples/measure_mem.py` | Mide RSS de Python gdstk para justificar la migración. | Archivo histórico |
| `examples/count_many_py_threads.py` | Demo de por qué el GIL mata el paralelismo Python. | Archivo histórico |
| `examples/count_many_py_mp.py` | Lo mismo con `multiprocessing`. | Archivo histórico |

**Recomendación concreta:** cuando se cree el repo de Miku aparte, mover `diff_gds.rs` y todos los helpers Python de fixtures ahí. Dejar en `gdstk/rust/examples/` **solo demos mínimos de lectura**:

- `count_cells.rs`
- `list_polygons.rs`
- `list_labels.rs`
- `list_references.rs`
- `list_paths.rs`
- `lib_info.rs`

Estos seis son lo que un nuevo usuario del crate va a buscar como "cómo leo un GDS con esto". Todo lo demás es ruido que confunde el boundary binding/aplicación.

### 1.2 Tests "snapshot" contra ejemplos compilados

Los snapshots en `tests/snapshots/` (por ejemplo `list_polygons_proof_lib.txt`) son **experimentales**: dependen del binario del ejemplo ya compilado y del formato exacto de su output (separadores, precisión de floats, orden). Si alguien cambia el formato de `println!` en `list_polygons.rs`, los tests se rompen sin que el binding haya cambiado.

**Estado:** útil para detectar regresiones durante el desarrollo, frágil como CI a largo plazo.

**Recomendación:** cuando exista Miku, mover estos snapshots a sus tests de integración y reemplazarlos aquí por tests Rust-nativos (`assert!` sobre structs retornados por el binding, sin pasar por el CLI del ejemplo).

---

## 2. Pendientes — no hechos, con trigger conocido

Lo que **a propósito no está implementado** y bajo qué condición conviene retomarlo. Ordenado por probabilidad decreciente de ser necesario.

### 2.1 `Cell::get_labels(depth)` y `Cell::get_paths(depth)` aplanados

- **Estado:** no expuesto. Existe `Cell::get_polygons(depth)` pero no los equivalentes para labels/paths.
- **Por qué no:** el modelo cell-by-cell de Miku detecta cambios de labels/paths celda por celda; aplanar con transforms recursivas es redundante en ese modelo.
- **Trigger para implementar:** cuando Miku agregue modo `--deep` (flat diff) o detección de cambios inline/outline que requieran ver todas las geometrías en coordenadas top-level.
- **Costo estimado:** ~1 h cada uno. Mismo patrón que `Cell::get_polygons` (iterar `cell.get_labels()`/`cell.get_paths()` con la profundidad y transformar).
- **Archivos a tocar:** `shim.cpp` (dos funciones), `lib.rs` (dos wrappers), un test por cada uno.

### 2.2 Soporte OASIS

- **Estado:** no expuesto. Descartadas a propósito: `read_oas`, `write_oas`, `oas_precision`, `oas_validate`.
- **Por qué no:** formato alterno raramente usado; GDSII cubre el 95%+ de la industria de chips en la práctica que nos interesa.
- **Trigger:** usuarios de foundries modernas (7 nm, 5 nm) que trabajen con OASIS por compresión. Si Miku apunta a esos nodos, se vuelve bloqueante.
- **Costo estimado:** 4–6 h. Estructura similar a GDSII pero con parser distinto; gdstk ya lo tiene implementado en C++, solo hay que exponerlo por cxx con el mismo patrón que GDS.

### 2.3 GitHub Actions / CI remoto

- **Estado:** solo `run_tests.sh` local.
- **Por qué no:** requiere un fork público de gdstk en GitHub y el repo actual es privado.
- **Trigger:** cuando se publique Miku o el binding como proyecto abierto.
- **Costo:** ~2 h. Setup YAML estándar más cacheo de vcpkg (crítico para no compilar desde cero cada run).

### 2.4 Auto-copia de DLLs en `build.rs`

- **Estado:** el usuario copia `zlib1.dll` y `qhull_r.dll` manualmente desde el directorio de vcpkg, o vía `run_tests.sh` que lo hace por él.
- **Alternativas:**
  1. Auto-copy en `build.rs` — requiere gymnastics con `cargo:rerun-if-changed` para no recopiar en cada build.
  2. Migrar al triplet vcpkg `x64-windows-static` para linkeo estático y eliminar las DLLs del runtime.
- **Trigger:** cuando la fricción moleste a un nuevo dev (reporte directo).
- **Costo:** ~1 h auto-copy; ~2 h migración a static (hay que verificar que qhull static compile limpio con MSVC).

### 2.5 `Option<T>` en getters variant-específicos de `Repetition`

- **Estado:** `rep.spacing()` devuelve `Point2D { x: 0.0, y: 0.0 }` si la repetición no es `Rectangular`. Igual para otros getters.
- **Problema:** no es idiomatic Rust; invita a bugs donde el caller trata `(0, 0)` como "spacing real de cero" en lugar de "no aplica".
- **Fix idiomatic:** devolver `Option<Point2D>` con `Some` sólo cuando la variante coincide.
- **Trigger:** si `gdstk-rs` se publica en crates.io (ahí el estándar idiomatic pesa).
- **Costo:** ~2 h. Es breaking change y toca todos los call sites.

### 2.6 Publicación como crate público en crates.io

- **Estado:** uso interno a Miku, sin versionado público.
- **Implicaciones si se hace:**
  - Revisión idiomatic (Option en getters, `std::error::Error` para `LibraryError`, doc-tests en todas las APIs públicas).
  - Semver con regla clara sobre qué cambios son breaking.
  - License file, README público, CHANGELOG.
  - CI en GitHub Actions (ver 2.3).
- **Trigger:** decisión de producto, no técnica.

### 2.7 Caché de flatten (GeometryInfo map)

- **Estado:** no expuesto. Cada llamada a `cell.get_polygons(depth)` recomputa todo el flatten desde cero.
- **Por qué no:** Miku llama una vez por celda y descarta; el caché sería overhead sin beneficio.
- **Trigger:** si aparece UI interactiva (diff visual con zoom/pan) o diff paramétrico donde el mismo cell se flattea muchas veces.
- **Costo:** ~2 h. Exponer el `PolygonCache` interno de gdstk como handle opaco en el builder pattern.

### 2.8 Linaje de transforms en polygons aplanados

- **Estado:** los polygons devueltos por `get_polygons(depth)` pierden la info de "de qué `Reference` vinieron y qué transform se les aplicó".
- **Por qué no:** gdstk upstream no trackea esto; hay que construirlo por nuestra cuenta.
- **Trigger:** si Miku quiere reportar cosas como "este polygon que se movió es una instancia de `amp` rotada 90°" en vez de "polygon X se movió".
- **Costo:** alto, ~8 h. Requiere sistema de traza propio (cada polygon aplanado guarda `Vec<(cell_name, transform)>` con el stack de refs).

### 2.9 Detección de renombres de celdas

- **Estado:** ni el modelo cell-by-cell ni el flat detectan renombres. Una celda `amp` renombrada a `amplifier` produce un `delete amp` + `add amplifier` en el diff.
- **Alternativas a investigar:**
  - Hash de contenido por celda (bbox + area + polygon count por layer) para matchear pares delete/add con fingerprint similar.
  - Fuzzy matching de nombres (Levenshtein con umbral).
  - Parse de `git log --follow` para detectar renombres externos.
- **Trigger:** reportes de usuarios con doble ruido delete+add en diffs.
- **Costo:** 8–16 h dependiendo del enfoque. El hash de contenido es el más robusto pero el más caro.

### 2.10 Métodos de conveniencia

Posponibles hasta que un caso de uso real los pida tres veces:

- `rep.bbox()` computado desde los extrema.
- `reference.bbox_with_repetition()` (bbox del ref ya considerando la repetición).
- `cell.layer_stats()` con count de polygons por layer (útil para reportes rápidos).

---

## 3. Descartado permanentemente

Decisiones cerradas. No reabrir sin un cambio de contexto fuerte (ej. el scope de Miku cambia a read-write).

### 3.1 Métodos de construcción y modificación (Miku = read-only)

Desde la API Python de gdstk, **omitidos deliberadamente todos** los siguientes, en todas las clases donde aparecen:

- **Transforms:** `translate`, `rotate`, `scale`, `mirror`.
- **Geometría derivada:** `fillet`, `fracture` en `Polygon`.
- **Construcción de paths:** `segment`, `arc`, `turn`, `cubic`, `bezier`, `interpolation`, `commands` en `FlexPath` y `RobustPath`.
- **Edición de biblioteca:** `rename_cell`, `replace_cell`, `remap_tags`, `add`, `remove` en `Library` y `Cell`.
- **Edición de paths:** `set_layers`, `set_datatypes`, `set_ends`, `set_joins`.
- **Repeticiones:** `apply_repetition` (que materializa la repetición en polygons concretos).
- **Constructores:** todos los `init()`.
- **Copia:** `copy_from()`, `copy()`.

**Razón permanente:** Miku es un **VCS read-only** sobre GDS. Exponer APIs de escritura sería código muerto mantenido indefinidamente. Si algún día aparece la necesidad de escritura sintética (p. ej. generar un GDS de diff visual), conviene rediseñar desde cero con el nuevo scope.

### 3.2 Cinco archivos de bindings Python no portados

Los siguientes archivos de gdstk Python (`python/*.cpp`) quedaron fuera del binding Rust con justificación explícita:

1. **`curve_object.cpp`** — `Curve` en gdstk es un objeto utility transient que casi nadie usa directamente; vive como `FlexPath::spine`, que ya exponemos vía `spine_point(i)`.
2. **`rawcell_object.cpp`** (parcial) — en Fase 8 expusimos la iteración básica (`name`, `size`, dependencias). Quedan fuera los métodos de **construcción** de RawCell (no aplica a read-only).
3. **`raithdata_object.cpp`** — específico de máquinas Raith e-beam; menos del 1 % de los GDSs reales lo usan.
4. **`gdswriter_object.cpp`** — streaming writer para GDSs > 100 MB. `Library::write_gds` cubre los casos normales; streaming solo se necesita para outputs gigantes que Miku nunca genera.
5. **`docstrings.cpp`** — no aplica, es solo documentación de la API Python.

### 3.3 Paridad byte-a-byte con numpy

Python expone `polygon.points` como `numpy.ndarray`. Rust devuelve iterador lazy sobre `Point2D`. **Funcionalmente equivalente**, semántica igual. No se busca paridad binaria ni compartir buffers — la interop C con Rust complica más de lo que ahorra.

### 3.4 Function pointers y callbacks C

`bend_function`, `end_function`, `join_function` en paths de gdstk son punteros a función C. **No son bindable por cxx** (cxx no soporta function pointers con captura ni closures C arbitrarios).

**Solución tomada:** cuando un path tiene alguna de estas funciones custom, devolvemos la variante `EndType::Function` / `BendType::Function` / `JoinType::Function` como "custom no introspectable". El usuario del binding puede detectar que hay una función, pero no puede inspeccionarla ni ejecutarla.

---

## 4. Bugs históricos y correcciones

Lecciones reales del desarrollo. Documentados para no repetirlos.

### 4.1 XOR ignoraba paths (Fase 4)

- **Síntoma:** tests passing con `proof_lib.gds` porque ese fixture tiene 0 paths; falla silenciosa con cualquier archivo que tuviera paths (todos los reales).
- **Causa:** `cell_xor_with` iteraba únicamente `polygon_array` y no `flexpath_array` / `robustpath_array`.
- **Detección:** al planear Fase 5, verificamos que `tinytapeout.gds` tiene 46 paths en 21 celdas y el diff los estaba ignorando completamente.
- **Fix:** Fase 5 agregó `collect_polygons_for_layer()` que convierte los paths a polygons vía `to_polygons()` **antes** del XOR.
- **Lección:** tests con un solo fixture son insuficientes. Diversificar geometría (agregar fixtures con paths, con references, con repetitions) antes de declarar "listo".

### 4.2 Commit inicial como submódulo (gitlink)

- **Síntoma:** el primer `git commit` creó `mode 160000 gdstk` (un gitlink/submódulo) en lugar de incluir el contenido del directorio.
- **Causa:** `gdstk/` tiene su propio `.git` adentro (es un clone del upstream); git detecta y trata como submódulo por default.
- **Fix:** `git rm --cached gdstk` + agregar `gdstk/` al `.gitignore` del repo padre. El trabajo Rust se commitea **dentro** del clone de gdstk (que tiene su propio historial independiente).
- **Lección:** cuando un directorio tiene `.git` adentro, git lo trata como submódulo. Si no es la intención, hay que decidir explícito: o submódulo bien configurado, o `.gitignore` + commits en el clone.

### 4.3 Error C2338 en Fase 6 (cxx con `TopLevelView`)

- **Síntoma:** error de compilación MSVC `C2338: definition of ::gdstk_shim::TopLevelView is required`.
- **Causa:** devolvíamos `UniquePtr<TopLevelView>` sin tener la definición completa del struct en el header que cxx genera.
- **Fix:** aplicar PIMPL completo — struct con `Impl` forward-declared + `unique_ptr<Impl>` + constructores y destructor explícitos en el `.cpp`.
- **Lección:** cxx exige definición completa de cualquier tipo que cruce como `UniquePtr`. Si el tipo es forward-declarado en el header, hay que usar PIMPL.

### 4.4 Error de lifetime en `TopLevel::cell()`

- **Síntoma:** error de compilación Rust "method returns lifetime `'1` but should be `'a`".
- **Causa:** `ffi::top_level_at(&self.view, idx)` devolvía `&CellHandle` con el lifetime del `view`, no de la `Library`. Pero los cells realmente viven en la `Library`, no en el view.
- **Fix:** `unsafe { std::mem::transmute }` justificado con comentario explicando que los `CellHandle` son propiedad de `Library` y el `view` es solo una vista.
- **Lección:** cuando el lifetime inferido por cxx es más corto que el real (y se puede demostrar), el `transmute` con comentario es aceptable. Pero cada uno debe estar documentado con la invariante que lo justifica.

### 4.5 Test Rust rompe con `signed_area` asumido

- **Síntoma:** un test esperaba `|signed_area| * count == area`, falla para polygons con `Polygon.fillet`.
- **Causa:** polygons filleteados pueden tener `signed_area = 0` (orientaciones que se cancelan) manteniendo `area > 0`.
- **Fix:** relajar el test a "`signed_area` es finito y `area > 0`".
- **Lección:** no asumir relaciones geométricas obvias. El área signed vs unsigned depende de la orientación acumulada de los vértices.

### 4.6 Paridad Python/Rust `True` vs `true`

- **Síntoma:** diff entre output de script Python y binario Rust en el test de `list_references` fallaba.
- **Causa:** Python `str(bool)` produce `True`/`False`; Rust `{}` produce `true`/`false`.
- **Fix cosmético:** `.lower()` en el script Python para normalizar.
- **Lección:** paridad de output entre lenguajes es frágil. Preferir estructura (JSON, tuples) sobre strings.

### 4.7 Cobertura de fixtures: `proof_lib.gds` vs `tinytapeout.gds`

- **Issue:** `proof_lib.gds` no tiene paths (0); `tinytapeout.gds` tiene 46 paths y 1099 references.
- **Consecuencia:** bugs relacionados con paths solo se detectan contra `tinytapeout.gds`.
- **Mitigación aplicada:** algunos tests opcionales requieren `tinytapeout.gds` (que está en repo externo); los tests críticos usan solo `proof_lib.gds` para no bloquear CI.
- **Lección:** mantener **al menos dos fixtures con geometría diversa** como requisito de testing desde el día uno.

### 4.8 `get_extrema` no manejaba `RepetitionType::None`

- **Síntoma:** `count = 0` cuando la repetición es `None`, rompiendo la simetría con `count()` (que normalizamos a 1 para None).
- **Fix:** chequeo explícito en el shim: si `type == None`, retornar 1 extremum en el origen.
- **Lección:** cuando se expone una API con múltiples getters relacionados, mantener las invariantes consistentes entre todos.

### 4.9 Asimetría `explicit_offset_count` vs `count`

- **Síntoma:** `count()` incluía el origen (elemento implícito en origin), `explicit_offset_count()` no lo incluía. Footgun para cualquiera que mezclara los dos.
- **Fix:** eliminar los métodos `explicit_*` entirely. El usuario que quiera los raw offsets usa `offsets().skip(1)` o el `count()` normalizado.
- **Lección:** si dos APIs difieren por una convención sutil, borrar una. No confiar en que el caller lea la doc.

### 4.10 `bootstrap-vcpkg.bat` bloqueado por sandbox

- **Síntoma:** auto-ejecución fallida con "Executing bootstrap-vcpkg.bat from a freshly cloned external repo" (el sandbox rechaza scripts autoejecutados de repos recién clonados).
- **Fix:** el usuario ejecutó manualmente. Queda documentado en README como paso manual de setup.
- **Lección:** no asumir que los scripts de bootstrap de terceros se auto-ejecutan; documentar siempre el paso manual.

---

## 5. Ambigüedades y decisiones abiertas

Decisiones que **no se cerraron** por falta de información o porque el boundary quedó difuso. Si tocas estas zonas, revisa antes.

### 5.1 Fallback a `O` (centro) en `Anchor` inválido

Inventamos el fallback: si el código de anchor no coincide con ninguno conocido, devolvemos `Anchor::O` (centro). Python gdstk tiene un bug latente que retorna `NULL` en ese caso. **No hay "behavior canónico"** si aparece un anchor con código `3`, `7`, `11` (valores no asignados).

**Decisión abierta:** ¿devolver `Result<Anchor, InvalidAnchor>` o mantener el fallback silencioso?

### 5.2 `Library::open` no devuelve `Result`

Los errores del parser (archivo corrupto, versión GDSII no soportada, etc.) se swallow silenciosamente y retornan una `Library` vacía o parcial. **Deuda técnica consciente desde Fase 1** por simplicidad del binding cxx.

**Decisión abierta:** cuándo migrar a `Result<Library, LibraryError>`. Es breaking change y requiere mapear todos los códigos de error de gdstk upstream.

### 5.3 Ciclos de references infinitos

No verificamos si gdstk upstream protege contra ciclos en el grafo de references. Si se pasa `depth = -1` (infinito) a un GDS con un ciclo (`A` referencia `B`, `B` referencia `A`), **podría loopear para siempre**. No probado ni con fixtures sintéticos ni con archivos reales.

**Decisión abierta:** agregar protección defensiva en el shim (límite de profundidad o set de visitados) o dejarlo como caveat documentado.

### 5.4 Modelo de diff: cell-by-cell vs flat

Ambos modelos están disponibles desde Fase 8.8:

- **Cell-by-cell:** `diff_gds.rs` lo usa. Compara celdas con el mismo nombre.
- **Flat:** vía `get_polygons(depth = -1)`. Aplana todo a top-level y compara polygons absolutos.

**No se decidió el default para Miku CLI.** La decisión queda pospuesta al desarrollo del CLI real, cuando se pueda medir con usuarios qué esperan ver por defecto.

### 5.5 Nombres con chars no-UTF-8

gdstk usa `char*` null-terminated sin especificar encoding. Nuestro wrapper:

- Para `Label.text` devuelve `Vec<u8>` y deja al caller usar `String::from_utf8_lossy`.
- Para `Cell.name` y `Reference.cell_name` asume UTF-8 directo (devuelve `&str`).

**Archivos con nombres exóticos** (chars latinos con encoding Windows-1252, caracteres de control, etc.) podrían romper el binding con panic en la conversión a `&str`.

**Decisión abierta:** migrar todos los nombres a `Vec<u8>` + helper de conversión o mantener el riesgo documentado.

---

## 6. Lecciones aprendidas

Patrones de proceso que se repitieron y conviene recordar.

1. **El roadmap inicial subestima el scope.** Planeamos 7 fases; hicimos 12. Cada fase reveló gaps honestos de las anteriores (paths ignorados, refs sin flatten, labels sin `get_labels(depth)`). No es falla de planeación: es la naturaleza de portar una API madura a otro lenguaje.

2. **Tests con un solo fixture son peligrosos.** `proof_lib.gds` ocultó el bug de paths hasta Fase 5. Dos fixtures con geometría distinta como **requisito de testing** desde el día uno hubieran detectado el bug antes.

3. **"Es obvio que no necesitamos X" es sospechoso.** La revisión honesta post-implementación, en cada fase, reveló consistentemente gaps de lectura que habíamos asumido cubiertos. Cuando te escuches pensar "esto claramente no hace falta", revísalo dos veces.

4. **Binding vs aplicación es una distinción que se confunde.** Lo hablamos tarde. `diff_gds.rs` confundió conceptualmente los boundaries — está en el repo del binding pero es código de Miku. Desde el día uno conviene separar `examples/` (demos del binding) de `apps/` o un repo aparte (código de producto).

5. **Paridad byte-a-byte en tests es frágil.** CRLF/LF, `True`/`true`, formato de floats, orden de campos — todos causan falsos positivos. Preferir asserts estructurales (compare structs, no strings) siempre que se pueda.

6. **cxx es excelente pero tiene rincones.** PIMPL requirement para tipos que cruzan como `UniquePtr`, lifetime inference conservador que a veces requiere `transmute` justificado, nulo soporte de templates C++. Son aprendibles, pero no obvios en el primer contacto.

7. **La performance estimada suele ser optimista.** Para `gds_info` esperábamos un speedup de 3–5× sobre el parse completo; el real fue 1.5–1.9×. El overhead de I/O y el parser de headers domina más de lo que parece en teoría.

---

*Última actualización: 2026-04-18. Mantener este documento vivo: cada vez que se cierre un pendiente, descarte algo nuevo, o aparezca un bug no trivial, actualizar la sección correspondiente.*
