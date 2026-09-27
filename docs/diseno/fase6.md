# Fase 6: rendimiento con layouts grandes y multinúcleo

Diseño basado en mediciones reales, no en suposiciones. Primero el algoritmo (lo que más rinde, en un solo núcleo), después los núcleos. Estado (2026-09-26): **6.1, 6.2, 6.3 y 6.5.a hechos**; faltan 6.4, 6.5.b y 6.6 (resumen en [`../roadmap.md`](../roadmap.md)).

---

## 1. Mediciones

Layout de prueba: `user_project_wrapper` de IHP SG13G2, 42 MB, 1 top, 28 celdas, 35 capas, 12,4 millones de polígonos aplanados. Dos versiones (`/tmp/bigrepo`, commits `a` y `b`) que difieren en 6 polígonos de la capa 8/0. Contenedor con 12 núcleos y 11 GB de RAM. Herramientas: `riku-mod-layout/examples/profile_diff.rs` (cronometra cada etapa del diff con tope de tiempo) y `profile_view.rs` (arma la escena del visor sin ventana).

### Diff

| Etapa | Tiempo | Memoria |
|---|---|---|
| Leer A y B | 0,5 s | 274 MB |
| Huella de las 28 celdas (aplanar su jerarquía) | 4,2 s | 2,5 GB, **no baja al liberar** |
| Aplanar por capa, top, 10 de 35 capas | 2,7 s | |
| **XOR, top, 10 de 35 capas** | **373 s** | |
| · capa 19/0: 124 mil rectángulos que no se superponen | **358 s → 0 diferencias** | |
| · capa 6/0: 9,6 millones de polígonos | 13,7 s → 0 diferencias | 3 GB |
| · capa 8/0 (la que cambió) | 0,84 s → 6 polígonos | |
| **Total estimado** | **~22 min** | |

- El 99 % del tiempo es XOR, y casi todo sobre capas **idénticas** en A y B.
- La capa 19/0 es el peor caso de Clipper (el motor booleano de gdstk): barre el layout por franjas horizontales y, cuando miles de figuras comparten la misma franja, cada paso revisa miles de bordes activos (costo casi cuadrático). KLayout fusiona esa capa en segundos.
- Repartir las 35 capas en 12 núcleos no alcanza: el piso es la capa más lenta (358 s).

### Visor

| Medida | Valor |
|---|---|
| Armar la escena (`GdsBackend::load`) | 4,4 s · 6,2 millones de elementos · 2,1 GB |
| Preparar un cuadro con el chip completo (`paint_scene`) | **560–660 ms** (menos de 2 cuadros/s) |
| RAM del proceso con la ventana abierta | **11 GB** (toda la del contenedor) |

Causas, en el código:
- `Scene::visit` (`viewer-core/src/scene.rs`) recorre los 6,2 millones de elementos en cada cuadro y recalcula el bbox de cada uno: no hay índice espacial.
- Con el chip entero en pantalla un píxel cubre 3,8 µm, y aun así se dibuja cada polígono: egui arma un `Shape` y su malla por elemento en cada cuadro.
- Los polígonos cóncavos se triangulan con earcut en cada cuadro (`riku/src/gui/polygon_fill.rs`).

### `log`

`riku log -n 200` sobre el repo de Riku: 3,5 s. Lo domina git (qué archivos cambió cada commit, con detección de renombres); parsear `.sch` no pesa.

---

## 2. Objetivos (criterios de "listo")

| # | Objetivo | Medido con |
|---|---|---|
| O1 | Primer diff del layout de 42 MB: **< 30 s en un núcleo**, **< 10 s con 12** | `profile_diff`, `riku diff` |
| O2 | Mismo resultado que hoy: `riku diff` texto/JSON idéntico y áreas iguales a KLayout | `riku_phase1_regress.sh`, `tools/verify/compare.sh --xor` |
| O3 | Visor con el chip completo: **< 50 ms por cuadro**, **< 3 GB** de RAM | `RIKU_PROFILE`, `ps` |
| O4 | Una capa que sí cambió y es patológica (19/0) hace XOR en **< 15 s** con 12 núcleos | `profile_diff` forzando el XOR |
| O5 | Sin carreras de datos al aplanar en paralelo | test concurrente con ThreadSanitizer en gdstk-rs |

---

## 3. Modelo de concurrencia

```
                     ┌──────────────────────────────────────────┐
 CLI (diff/log/show) │ pool de rayon: 1 hilo del SO por núcleo   │  RIKU_JOBS=N / --jobs N
 visor (carga)       │ (work stealing; anidar es seguro)         │
                     └──────────────────────────────────────────┘
 visor (UI)          hilo de egui: solo dibuja; nunca calcula
                     tokio: espera de E/S y spawn_blocking → dentro, rayon::install
```

- **Hilos del SO en núcleos reales.** `rayon` crea un hilo por núcleo y reparte tareas (como Go con `GOMAXPROCS` = núcleos). No se usan tareas `tokio` para cálculo.
- **Un solo pool** en todo el proceso. Paralelismo anidado (commits de `log` que a su vez hacen diffs de layouts por capa) sin sobresuscripción: rayon lo reparte.
- **Límite:** `RIKU_JOBS` (o `--jobs`) fija el tamaño del pool. Por defecto, los núcleos disponibles.
- **Memoria antes que velocidad:** aplanar la capa 6/0 ocupa 3 GB. Aplanar varias capas grandes a la vez puede agotar la RAM, así que las tareas pesadas pasan por un presupuesto de memoria (sección 4.4).

---

## 4. Diseño por componente

### 4.1 Huella por capa antes del XOR (`riku-mod-layout`, un núcleo) — lo que más rinde

**Hoy** (`gds_diff.rs::diff_one_cell`): para cada capa, aplanar A y B filtrado y hacer XOR siempre.

**Propuesto:** aplanar A y B de la capa, calcular una huella de cada lado y saltar el XOR si son iguales.

```text
para cada capa:
    fa, fb = aplanar(A, capa), aplanar(B, capa)
    si huella(fa) == huella(fb): continuar          ← nuevo
    xor(fa, fb)
```

- **Huella:** la misma de `geometry_fingerprint`: hash de cada polígono (capa, datatype, vértices cuantizados a 1e-6 unidades), vector ordenado. Comparar vectores completos, no un solo hash: una colisión exigiría que dos multiconjuntos de polígonos distintos den los mismos 64 bits en cada elemento.
- **Correctitud:** huellas iguales ⇒ mismos polígonos ⇒ XOR vacío. Al revés no: la misma forma partida distinto da huellas distintas y se hace el XOR, que da el resultado exacto. Nunca se pierde un cambio.
- **Dónde actúa:** `diff_one_cell` lo usan `diff_gds` (CLI, `log`, `status`, `show`) y `diff_cell_as` (diff del visor), así que el beneficio llega a todos.
- **Mejor todavía (revisión contra el código):** `geometry_fingerprint` ya aplana la celda **completa** (todas las capas) para la huella de celda. En esa misma pasada se puede agrupar el hash de cada polígono por capa y devolver `BTreeMap<LayerKey, Vec<u64>>`. Así las capas que no cambiaron se conocen sin volver a aplanar 35 veces; solo las que cambiaron se aplanan filtradas para el XOR. Estimado: 0,5 s leer + 4,2 s huellas (ya se pagan hoy) + 0,8 s XOR de la 8/0 ≈ **6 s en un núcleo** (hoy ~22 min). Se guardan solo los hashes, no los polígonos, para no duplicar los 2,5 GB del aplanado.
- **El diff del visor** (`diff_cell_as`) no calcula huella de celda: llama a `diff_one_cell` directo. Recibe el mismo mapa por capa (calculado ahí, una vez por celda), así el visor también se beneficia.
- **Memoria:** igual que hoy.
- **Contrato:** ninguno cambia. Es interno a `gds_diff.rs`.

### 4.2 Visor: índice espacial y nivel de detalle — **hecho**

Implementado en `viewer-core/src/index.rs` (`SceneIndex`) y el painter del visor; cómo funciona, en [`../gui.md`](../gui.md) y [`../arquitectura.md`](../arquitectura.md). Resultado con el layout de 42 MB y el chip completo: **11 GB → 2,5 GB** de RAM y **~600 ms → ~2 ms** para preparar el cuadro. Esquemáticos y celdas: capturas idénticas a antes en tres zooms.

Lo que cambió respecto al plan, por lo que salió al medir:
- La pirámide se pinta como **una textura por nivel** (capas compuestas en su orden de pintado, cache de 3), no como rectángulos: con rectángulos seguían siendo 150–350 mil formas por cuadro.
- **Se resume solo si hace falta:** si lo visible cabe en 60 000 elementos, se dibuja todo como siempre. Si no, hay dos pirámides (lo menor a 4 o a 16 celdas); se prueba la de 16 antes de agrandar el texel.
- **Cables largos y finos:** el ancho se estima con `2·área/perímetro` (sirve con curvas); esos se marcan siguiendo sus bordes, los polígonos chicos por las celdas cuyo centro cae dentro, y las capas de solo contorno por sus bordes.
- Las etiquetas ilegibles u ocultas no salen de la consulta (eran 260 mil por cuadro en un layout de 8 MB).
- Límite: zoom cercano sobre una zona muy densa (~45 ms por cuadro); una pirámide más fina necesitaría bitsets dispersos.

### 4.3 gdstk-rs seguro entre hilos (submódulo `external/gdstk`)

Hoy `Library` guarda un `cxx::UniquePtr` a un tipo C++ opaco: Rust lo trata como no `Send` ni `Sync` y no deja compartirla entre hilos.

Revisión del C++ (lo que usa Riku):

| Operación | ¿Escribe estado compartido? |
|---|---|
| Leer GDS/OASIS | No: todo es local a la librería nueva |
| `Cell::get_polygons` (aplanar) | Es `const`, **pero** con paths llama a `FlexPath::to_polygons`, que ejecuta `remove_overlapping_points()` y **borra puntos del path** |
| `RobustPath::to_polygons` | No (`const`) |
| Booleanas (Clipper) | No: objetos locales a cada llamada |
| Globales | Solo `static` de SVG en `utils.cpp` (Riku no los usa) y uno comentado en `clipper_tools.cpp` |

`remove_overlapping_points` solo escribe si encuentra puntos repetidos. Después de una primera pasada, las siguientes solo leen.

**Cambios:**
1. **Normalizar al cargar.** En `read_gds_with_error`/`read_oas_with_error` (shim C++), recorrer los `FlexPath` de todas las celdas y llamar a `remove_overlapping_points()` una vez, en el hilo que lee. Desde ahí aplanar no escribe nada.
2. **`unsafe impl Send for Library` y `unsafe impl Sync for Library`**, con un comentario `SAFETY:` que cite la tabla anterior. `Cell<'a>`, `Reference<'a>` y los demás toman prestada la librería (`&'a Library`) y heredan la seguridad sin tocarlos.
3. **Test concurrente:** 12 hilos que aplanan y hacen XOR sobre la misma librería (con paths), muchas rondas, comparando contra el resultado secuencial. Corre en la CI de gdstk-rs con el toolchain estable. ThreadSanitizer queda como job aparte y opcional: necesita nightly (`-Zsanitizer=thread`) **y** compilar el C++ de gdstk con `-fsanitize=thread`; el contenedor solo tiene el toolchain estable y no tiene valgrind, así que se hace en el runner de GitHub.
4. **Memoria que no baja** (2,5 GB tras liberar las huellas): verificado en `shims.cpp` que `FlattenedPolygonsHandle` libera cada polígono al soltarse, así que no es una fuga evidente. Lo más probable es el allocator de glibc reteniendo arenas fragmentadas por millones de asignaciones chicas. Plan: `malloc_trim(0)` desde el shim al terminar un aplanado grande y medir con `profile_diff`; si no baja, probar `heaptrack` en el runner.

**Revisión para implementar (2026-09-26, después de 6.1):**
- **Qué se marca `Send`/`Sync`:** en Rust, `Cell<'a>`, `Polygon<'a>` y los demás guardan `&'a ffi::XxxHandle`, y los tipos opacos de `cxx` no son `Sync`. No alcanza con `Library`: se declara `unsafe impl Send + Sync` en el bridge para los handles de solo lectura (`LibraryHandle`, `CellHandle`, `ReferenceHandle`, `PolygonHandle`, `LabelHandle`, `FlexPathHandle`, `RobustPathHandle`, `RepetitionHandle`, `FlattenedPolygonsHandle`, `TopLevelView`) y `Send` para `XorSplitHandle`. Con eso, los wrappers heredan todo solos.
- **Otros caminos que llegan a `FlexPath::to_polygons`:** `Cell::bounding_box` y `Reference::bounding_box` (usan `const_cast` en el shim) y `write_gds`/`write_oas`. La normalización al cargar los cubre a todos.
- **`as_flexpath_mut`** (`shims.cpp`) no lo usa ninguna función expuesta: se borra, para que no quede una puerta a escribir desde `&self`.
- **Test de la normalización:** un GDS con `PATH` que tenga puntos repetidos. El aplanado debe dar lo mismo antes y después de normalizar, y lo mismo en 12 hilos que en uno.

### 4.4 Paralelismo del diff (`riku-mod-layout`, con `rayon`)

**Medición después de 6.1** (layout de 42 MB, sin cambios reales, 1 núcleo): `riku diff` tarda **5,9 s** y ocupa **1,7 GB**.

| Etapa | Tiempo | Nota |
|---|---|---|
| Leer A y B | 0,47 s | |
| Aplanar las 28 celdas, A y B | 2,6 s | la top sola: 2,17 s (12,4 millones de polígonos) |
| Huellas (`canonical_points` + hash + orden) | ~2,7 s | un `Vec` nuevo por polígono, 13,8 millones de veces |
| XOR | 0 | ninguna capa difiere |

Con un cambio real en las capas 6/0 y 19/0 son **16 s**. Lo extra es volver a aplanar esas capas, volver a hashearlas en `xor_layer` y hacer el XOR local.

**Hallazgo:** después de 6.1 el piso ya no es el XOR sino **aplanar y hashear la top**. Además, `layer_prints` aplana toda la celda y la guarda entera (≈ 180 B por polígono, ~2,2 GB) para quedarse con 8 B por polígono.

**Diseño:**

| # | Qué | Cómo | Ganancia estimada |
|---|---|---|---|
| a | Leer A y B a la vez | `rayon::join` | 0,47 → 0,25 s |
| b | **Huella por pedazos** | `Cell::get_polygons` es, en el C++, lo propio de la celda más `Reference::get_polygons` de cada referencia. `layer_prints` hace lo mismo en pedazos: lo propio (`depth(0)`) y cada referencia, en paralelo. Cada pedazo se aplana, se hashea y se suelta. Al final se juntan los vectores por capa y se ordenan (`par_sort_unstable`) | aplanar + huellas: 5,3 → ~0,6 s; pico de memoria de ~2,2 GB a lo que haya en vuelo |
| c | Sin asignación por polígono | `canonical_points` escribe en un buffer reutilizado por hilo | ~2× en las huellas, aun en un núcleo |
| d | Celdas y lados A/B en paralelo | `par_iter` sobre las celdas comunes; A y B de cada una con `join` | rayon anida sin sobresuscribir |
| e | Capas que difieren, en paralelo | `par_iter` sobre esas capas: aplanar filtrado + `xor_layer` | caso con cambios: 16 → ~4–5 s |

- **Correctitud de (b):** el multiconjunto de polígonos es el mismo que el de `get_polygons()` completo, porque el C++ hace exactamente eso. Un test compara la huella por pedazos con la huella entera en los fixtures y en los tres PDKs.
- **Referencia gigante:** si una sola referencia concentra casi todo (un macro que es el 90 % del chip), ese pedazo queda en un hilo. Primero se mide cómo se reparten las referencias de la top. Si hace falta, se parte también dentro del macro, lo que exige componer transformaciones y queda fuera de este paso.
- **Presupuesto de memoria (solo para e):** la huella ya dice cuántos polígonos tiene cada capa de cada lado. Cada tarea pide `(nA + nB) × 180 B` a un semáforo de bytes antes de aplanar; el cupo total es el 50 % de `MemAvailable` (`/proc/meminfo`). Las tareas grandes van primero; una tarea más grande que todo el cupo corre sola, para no trabarse.
- **Salida determinista:** `par_iter` sobre las capas en orden, y `collect` conserva ese orden. Después se agrupan las instancias como hoy.
- **Cache (`diff_cache.rs`):** el archivo temporal pasa de `tmp<pid>` a `tmp<pid>-<contador atómico>`.
- **Pool:** uno global. `riku` lo configura al arrancar con `--jobs N` o `RIKU_JOBS`; `riku-mod-layout` solo usa `rayon` y no fija tamaños. `RIKU_JOBS=1` sirve de salida de emergencia.
- **Visor:** `diff_cell_as` pasa por `diff_one_cell` y se beneficia solo. La carga de la escena (`scene.rs`, `get_polygons` de toda la celda) puede usar los mismos pedazos más adelante.

**Objetivos revisados:** O1 ya se cumple en un núcleo. Nuevos: sin cambios, **< 2 s** y **< 1 GB**; con el cambio en 6/0 y 19/0, **< 6 s**; salida idéntica (`riku_phase1_regress.sh`, `compare.sh --xor`).

**Orden de implementación, midiendo cada paso con `profile_diff` y `/usr/bin/time`:**
1. 6.3 en gdstk_rust: normalización, `Send`/`Sync`, test concurrente. Commit ahí y actualización del submódulo.
2. (b) y (c) todavía en un núcleo, con el test de igualdad. Ahí se ve la baja de memoria.
3. `rayon`: (a), (b) y (d) en paralelo.
4. (e) con el presupuesto de memoria, `--jobs`/`RIKU_JOBS` y el arreglo de la cache.

### 4.5 XOR de una capa que cambió: primero por huellas, cuadrantes como respaldo

**Dato del código:** `xor_split_flat` no hace un XOR: hace **dos** booleanas `NOT` de Clipper (`B \ A` para lo añadido y `A \ B` para lo eliminado, `shims.cpp::polygons_xor_split`). Cada una barre toda la capa, así que la 19/0 paga 2 × ~180 s.

#### 4.5.a Diferencia por huellas con recorte local (`riku-mod-layout`, exacto y sin cambios en gdstk-rs)

Con los hashes por polígono de 4.1, cada lado se parte en los polígonos **idénticos en ambos** (`C`) y los **propios** de cada lado (`A'`, `B'`). Casi siempre `A'` y `B'` son diminutos (en el layout de prueba: 6 polígonos de 18 mil en la 8/0).

- Identidad exacta: `XOR(A, B) = XOR(A', B') \ C`. Los polígonos comunes cancelan cualquier diferencia que caiga dentro de ellos.
- `C` es enorme, pero solo importan los polígonos de `C` cuyo bbox toca el resultado de `XOR(A', B')`: `C_local`. Se filtran con los bboxes (ya calculados para el hash) y la resta se hace contra `C_local`, normalmente decenas de polígonos.
- Costo: dos `NOT` sobre conjuntos chicos. La 8/0 pasa de 0,84 s a milisegundos; una 19/0 con un cambio chico pasaría de 358 s a milisegundos.
- Correctitud: la geometría del resultado es idéntica a la del XOR completo. La **partición en polígonos** puede diferir (Clipper une lo que toca de otra forma), así que el conteo `+N polys` puede cambiar; las áreas, bbox e instancias no. Se documenta y se verifica contra KLayout por área.

#### 4.5.b Cuadrantes, para cuando `A'` y `B'` son grandes (una capa regenerada entera)

Umbral inicial: más de 100 mil polígonos propios en un lado.

- **Identidad:** `XOR(A, B) ∩ T = XOR(A ∩ T, B ∩ T)` para cualquier rectángulo `T`. Partiendo el bbox de la capa en cuadrantes `T₁…Tₙ`, la unión de los resultados es el XOR completo.
- **Por cuadrante:** tomar los polígonos cuyo bbox toca el cuadrante, recortarlos al cuadrante, XOR, y recortar el resultado al cuadrante (un polígono que cruza el borde aparece en los dos lados, cada uno con su parte).
- **Por qué arregla el peor caso de Clipper:** cada franja horizontal ve solo los bordes de su cuadrante. Con una grilla de k×k, los bordes activos por franja bajan del orden de k veces y la cantidad de trabajo de cada llamada, del orden de k².
- **Y reparte núcleos:** los cuadrantes son independientes.
- **Tamaño:** grilla adaptativa, hasta ~20 mil polígonos por cuadrante (se parte en 4 el que se pase).
- **gdstk-rs:** función nueva `xor_split_flat_in(a, b, rect) -> XorSplit` que filtra por bbox, recorta y hace el XOR en C++ (Clipper ya soporta la intersección con un rectángulo).
- **Efecto visible:** un polígono de diferencia que cruza un borde de cuadrante sale partido en dos. Las áreas, los bbox y las instancias no cambian. El conteo `+N polys` sí puede cambiar, solo en capas donde se usan cuadrantes. Se documenta; la comparación contra KLayout es por área y sigue igual.
- **Cache:** la clave de `diff_cache.rs` incluye `CARGO_PKG_VERSION`. Como 4.5.a y 4.5.b pueden cambiar la partición de los polígonos, `riku-mod-layout` sube a `0.2.0` al activarlos, y los resultados viejos se recalculan solos.

### 4.6 `log`, `show` y `status` en paralelo (núcleo del ejecutable: `riku/src/core/analysis`)

- `log/walk.rs`: los commits se analizan con `par_iter().map_init(...)`. `map_init` abre un `GitService` por hilo, porque `git2::Repository` se puede mover entre hilos pero no compartir. El `collect` de rayon conserva el orden de los commits.
- `show.rs` y `status/analyze.rs`: igual, por archivo.
- Los avisos se juntan por commit o archivo y se concatenan en orden.
- `GitRepository` pasa a exigir `Sync`, o se recibe una fábrica que abre el repo. Los mocks de los tests siguen funcionando con la versión secuencial.
- **Estimado:** `log -n 200` de 3,5 s a menos de 1 s. Más importante con layouts en el historial, donde se combina con 4.4.

---

## 5. Qué no cambia

- `riku-kernel` (los contratos `FormatModule` y `Registry` ya exigen `Send + Sync`).
- El motor de Carlos (`xschem-viewer-rust`).
- La salida de la CLI: texto y JSON idénticos. La única diferencia admitida es el conteo de polígonos en capas que usan cuadrantes (4.5).
- Los flags existentes. Se suman `--jobs N` (en `diff`, `show`, `log`, `status`) y la variable `RIKU_JOBS`.

---

## 6. Orden, esfuerzo y criterio de cada paso

| Paso | Qué | Dónde | Esfuerzo | Listo cuando |
|---|---|---|---|---|
| 6.1 | Huella por capa | `riku-mod-layout` | S | O1 en un núcleo (< 30 s) y O2 |
| 6.2 | Índice espacial, LOD y triangulación en cache — **hecho** (4.2: 11 GB → 2,5 GB, 600 ms → 2 ms) | `viewer-core`, backends, visor | L | O3; CI del crate de Carlos en verde |
| 6.3 | gdstk-rs seguro entre hilos + memoria | `external/gdstk` | M | O5; la RAM baja al liberar |
| 6.4 | Huella por pedazos + `rayon` en el diff + presupuesto de memoria + cache | `riku-mod-layout`, `riku` (`--jobs`) | M | sin cambios < 2 s y < 1 GB; con cambios < 6 s; O2 |
| 6.5.a | Diferencia por huellas con recorte local | `riku-mod-layout` | S | capa cambiada con pocos cambios: XOR en ms; áreas iguales a KLayout |
| 6.5.b | XOR por cuadrantes (respaldo) | `riku-mod-layout`, `external/gdstk` | M | O4 y áreas iguales a KLayout |
| 6.6 | `log`/`show`/`status` en paralelo | `riku/src/core/analysis` | S | salida idéntica; `log -n 200` < 1 s |

- 6.1 y 6.5.a van primero (juntos son un paso): son chicos, no tocan otros repos y resuelven el pendiente #2 casi por completo, en un solo núcleo.
- 6.2 va segundo porque es el riesgo que ve el usuario: abrir un chip real se come la memoria.
- 6.3 es requisito de 6.4 y 6.5.
- Cada paso se mide con `profile_diff`/`profile_view` antes y después, y se anota en la tabla de avance.

---

## 7. Riesgos

| Riesgo | Mitigación |
|---|---|
| Una carrera de datos en gdstk que la revisión no vio | Test concurrente + ThreadSanitizer en la CI de gdstk-rs; `RIKU_JOBS=1` como salida de emergencia |
| Paralelizar agota la RAM (capas de varios GB a la vez) | Presupuesto de memoria (4.4), tareas grandes primero |
| LOD esconde un detalle que el usuario busca | Solo resume lo menor a un píxel; al acercarse aparece todo. Opción en Ajustes para desactivarlo |
| Cambia el conteo de polígonos con cuadrantes | Solo en capas pesadas que cambiaron; documentado; áreas verificadas contra KLayout |
| El layout de prueba es de IHP; otro PDK podría tener otro peor caso | Repetir `profile_diff` con un wrapper de SKY130 (Caravel) antes de cerrar la fase |

---

## 8. Revisión contra el código (2026-09-26)

Verificado leyendo `external/gdstk` (C++ y gdstk-rs), `riku-mod-layout`, `viewer-core`, `riku/src/gui` y `riku/src/core`:

| Supuesto del plan | ¿Se sostiene? | Detalle |
|---|---|---|
| gdstk no tiene estado global en lo que usa Riku | Sí | Booleanas con objetos locales (`clipper_tools.cpp`); los `static` son de SVG o están comentados |
| `Library` no es `Send`/`Sync` | Sí | `cxx::UniquePtr` a tipo opaco, sin `impl Send` en el bridge |
| `FlexPath::to_polygons` escribe | Sí | `remove_overlapping_points()` borra puntos; después de una pasada solo lee (compara distancias) |
| `RobustPath::to_polygons` es `const` | Sí | |
| `Cell<'a>` toma prestada la librería | Sí | `Cell { handle: &'a CellHandle }`: hereda la seguridad de `Library` |
| Los aplanados se liberan | Sí | `FlattenedPolygonsHandle::Impl::~Impl` libera cada polígono → la RAM retenida es del allocator, no una fuga |
| El XOR es una sola operación | **No** | Son dos `NOT` de Clipper (`B\A`, `A\B`): el costo por capa es doble. Motiva 4.5.a |
| `geometry_fingerprint` aplana toda la celda | Sí | Se reaprovecha para los hashes por capa (4.1) |
| `diff_cell_as` (visor) no usa la huella de celda | Sí | Recibe el mapa por capa (4.1) |
| La cache se invalida por versión | Sí | `env!("CARGO_PKG_VERSION")` en la clave → subir a 0.2.0 con 4.5 |
| `Scene::visit` es lineal y recalcula bboxes | Sí | `Polygon::bounding_box` recorre los vértices en cada cuadro |
| earcut en cada cuadro | Sí | `polygon_fill::triangulate` desde `paint_filled_polygon` |
| La carga del visor corre en `spawn_blocking` | Sí | `rayon::install` cabe adentro sin bloquear a tokio |
| `rayon` ya está en el árbol | **No** | Dependencia nueva en `riku-mod-layout`, `viewer-core` (feature) y `riku`; el `Cargo.lock` cambia |
| Hay nightly/valgrind en el contenedor | **No** | Solo `stable`; ThreadSanitizer va en el runner de GitHub (4.3) |
| `log`/`status` abren el repo por ruta | Sí | `analyze_with_options_path` → `GitService::open`: se puede abrir uno por hilo |

---

## Avance

| Paso | Estado | Notas |
|---|---|---|
| 6.1 + 6.5.a | Hecho (2026-09-26) | `gds_diff.rs`: la huella de celda pasa a ser **por capa** (`layer_prints`, una sola pasada de aplanado) y se reutiliza para saltar las capas iguales. Las capas que difieren van a `xor_layer`: empareja los polígonos idénticos de A y B (hash + vértices), y si lo común es mayoría hace el XOR solo de lo propio más los comunes que lo tocan (`XOR(A' ∪ Cl, B' ∪ Cl)`, grilla de bboxes); si no, el XOR de la capa entera como antes. Resultado en el layout de 42 MB: **~22 min → 6,4 s** (1,7 GB), igual a KLayout (área de diferencia 0; KLayout tarda 26 s). Con un cambio real en las capas 6/0 (4,8 millones de polígonos) y 19/0: **16 s**, áreas idénticas a KLayout. `riku-mod-layout` sube a 0.2.0 (invalida la cache). 54 tests; regresión y `compare.sh --xor` idénticos |
| 6.3 | Hecho (2026-09-26) | `gdstk_rust` `3ff9daf`. `finish_load` (shims.cpp), al leer GDS/OASIS: borra una vez los puntos repetidos de cada `FlexPath` (copia de `remove_overlapping_points`, que en gdstk es privada) y calcula las capas de `Library::layers()`. `unsafe impl Send + Sync` para los handles del bridge, con el porqué. Se borró `as_flexpath_mut`. `tests/concurrency.rs`: 12 hilos × 40 rondas aplanan, piden bbox, capas y XOR (jerarquía SREF/AREF y paths con puntos repetidos) y dan lo mismo que un hilo. CI de gdstk_rust: job no bloqueante con ThreadSanitizer. Regresión, `compare.sh` (tres PDKs) y `compare.sh --xor` idénticos |

**Diferencias con el plan (6.1 + 6.5.a):**
- **Hizo falta tocar gdstk-rs** (el plan decía que no): no había una booleana sobre listas de polígonos. Se agregó `xor_split_owned` (`gdstk_rust` `3746c63`), con dos tests que la comparan contra `xor_split_flat`.
- **La causa real del caso de 42 MB no era Clipper sino la huella.** B es una reexportación de A: los mismos polígonos escritos con otro vértice de inicio o sentido de giro. Con la huella por vértices literales, ninguna capa coincidía (0 gemelos en 4,8 millones en la 6/0). La huella usa ahora la **forma canónica** de cada polígono (`canonical_points`: cuantizado, sin puntos repetidos ni el de cierre, antihorario, empezando por el vértice menor). Con la regla nonzero de gdstk, esa normalización no cambia la región: dos polígonos con la misma forma canónica cubren exactamente lo mismo. Con ella, el 100 % de los polígonos tiene su gemelo y el diff ni siquiera llega al XOR.
- Los "6 polígonos de diferencia" que el XOR completo reportaba en la capa 8/0 eran restos del redondeo de Clipper entre dos escrituras distintas de la misma geometría: KLayout mide área 0 y la forma canónica empareja los 9.252 polígonos.

**Diferencias con el plan (6.3):**
- **Había otra escritura:** la cache perezosa de `Library::layers()` (`mutable` en `LibraryHandle::Impl`) se llenaba en la primera llamada; dos hilos a la vez la corrompían. Ahora se calcula al cargar.
- **Bug encontrado de paso:** `Library::layers()` solo miraba los polígonos, y el diff recorre esas capas; un cambio en una capa dibujada solo con paths no se reportaba. Ahora incluye las capas de los paths (test `change_in_a_layer_drawn_only_with_paths_is_reported`).
- **Memoria que no baja (`malloc_trim`):** pasa a 6.4. La huella por pedazos ya evita aplanar la celda entera; se mide ahí si todavía hace falta.
