# Fase 6: rendimiento con layouts grandes y multinúcleo

Diseño basado en mediciones reales, no en suposiciones. Primero el algoritmo (lo que más rinde, en un solo núcleo), después los núcleos. Estado: **propuesto** (2026-09-26).

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
- **Estimado:** ~0,5 s leer + 4,2 s huellas de celdas + ~6 s aplanar y hashear las 35 capas + 0,8 s XOR de la 8/0 ≈ **12 s en un núcleo** (hoy ~22 min).
- **Memoria:** igual que hoy (una capa por vez).
- **Contrato:** ninguno cambia. Es interno a `gds_diff.rs`.

### 4.2 Visor: índice espacial, nivel de detalle y triangulación en cache (`viewer-core` + visor)

Un índice que se arma **una vez** al cargar la escena y que el dibujo consulta en cada cuadro.

```rust
// viewer-core/src/index.rs (nuevo)
pub struct SceneIndex {
    grid: Grid,                 // celdas del mundo → índices de elementos que las tocan
    bboxes: Vec<[f32; 4]>,      // bbox precalculado de cada elemento
    coverage: Vec<Coverage>,    // pirámide: por nivel y capa, qué celdas tienen geometría
    triangles: Vec<Option<Box<[u32]>>>, // earcut de cada polígono cóncavo, calculado al cargar
}

pub enum Visible<'a> {
    Element(usize, &'a DrawElement),   // se dibuja tal cual
    Block { layer: Layer, rect: BoundingBox }, // geometría menor a un píxel, resumida
}
```

- **Nivel de detalle:** en cada cuadro se pide `visit_lod(viewport, tamaño_de_píxel)`. Los elementos más grandes que un píxel se entregan como hoy. Los menores no se dibujan uno por uno: la pirámide de cobertura entrega, por capa, las celdas ocupadas del nivel cuyo tamaño ≈ 1 píxel, con los tramos contiguos de una fila fusionados en un solo rectángulo. La cantidad de formas por cuadro queda acotada por los píxeles de la pantalla, no por el tamaño del chip.
- **Triangulación:** `polygon_fill` usa `triangles[i]` en vez de llamar a earcut en cada cuadro.
- **Armado en paralelo:** bboxes, grilla, pirámide y triangulación son independientes por elemento → `rayon` al cargar (dentro del `spawn_blocking` del backend).
- **Contrato sin romper a nadie:**
  - `Scene` gana `index: Option<Arc<SceneIndex>>` y `Scene::build_index()`, con valor por defecto, igual que en la fase 4 (`text_style`, `ghost`…).
  - `RenderableScene` gana `fn visit_lod(...)` con implementación por defecto que llama a `visit`. El crate de Carlos (que implementa `RenderableScene` con la feature `viewer-core-compat`) sigue compilando sin cambios; la CI lo verifica.
  - `DrawElement` **no cambia** (agregarle campos rompería a quien lo construye): lo precalculado vive en tablas paralelas indexadas por posición.
- **Quién lo usa:** `GdsBackend` y `XschemViewer` llaman a `build_index()` al terminar la escena. Un `.sch` también gana culling rápido.
- **Estimado:** cuadro con el chip completo de ~600 ms a < 50 ms. RAM de 11 GB a ~2,5 GB (escena 2,1 GB + índice); egui deja de fabricar millones de formas por cuadro.
- **Aparte, opcional:** la escena ocupa 2,1 GB por guardar los vértices en `f64`. Pasar la geometría a `f32` la reduciría a la mitad. Queda anotado; no entra en esta fase.

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
3. **Test concurrente:** 12 hilos que aplanan y hacen XOR sobre la misma librería (con paths), comparando contra el resultado secuencial. En la CI de gdstk-rs, un job con ThreadSanitizer (`-Zsanitizer=thread`, nightly) que falla ante cualquier carrera.
4. **Memoria que no baja** (2,5 GB tras liberar las huellas): medir si es una fuga (valgrind/heaptrack sobre `profile_diff`) o el allocator de glibc que retiene las arenas. Si es el allocator, `malloc_trim(0)` al terminar cada celda grande; si es una fuga, arreglarla en el shim.

### 4.4 Paralelismo del diff (`riku-mod-layout`, con `rayon`)

Sobre 4.1 y 4.3:

| Nivel | Qué se reparte | Nota |
|---|---|---|
| Lectura | A y B a la vez (`rayon::join`) | 0,5 → 0,25 s |
| Huella de celdas | cada celda común | la top domina (3,8 s de 4,2 s); el resto es chico |
| Capas | aplanar + huella (+ XOR si cambió) de cada capa de cada celda que cambió | la unidad de trabajo fina: (celda, capa) |
| XOR grande | cuadrantes (4.5) | solo si la capa cambió y es pesada |

- **Presupuesto de memoria:** cada tarea (celda, capa) estima su tamaño (polígonos propios × referencias, o el conteo aplanado de la huella de celda, que ya se calculó) y pide ese cupo a un semáforo de bytes antes de aplanar. Por defecto el cupo total es el 50 % de la RAM libre (`/proc/meminfo`). Las tareas grandes esperan su turno; las chicas llenan los huecos.
- **Orden:** las tareas más grandes primero (la cola larga queda con tareas chicas y los núcleos no se quedan ociosos al final).
- **Resultado determinista:** se recolecta por (celda, capa) y se ordena como hoy antes de agrupar instancias, así la salida no depende del orden en que terminen los hilos.
- **Cache (`diff_cache.rs`):** el archivo temporal se llama `tmp<pid>`, así que dos hilos que guardan la misma clave lo pisarían. Pasa a `tmp<pid>-<contador atómico>`.
- **Estimado:** de ~12 s (4.1) a **~4–6 s**, con el piso en la huella de la celda top y en aplanar la capa 6/0.

### 4.5 XOR por cuadrantes (`riku-mod-layout` + una función nueva en gdstk-rs)

Para una capa que **sí cambió** y tiene muchos polígonos (umbral inicial: 100 mil).

- **Identidad:** `XOR(A, B) ∩ T = XOR(A ∩ T, B ∩ T)` para cualquier rectángulo `T`. Partiendo el bbox de la capa en cuadrantes `T₁…Tₙ`, la unión de los resultados es el XOR completo.
- **Por cuadrante:** tomar los polígonos cuyo bbox toca el cuadrante, recortarlos al cuadrante, XOR, y recortar el resultado al cuadrante (un polígono que cruza el borde aparece en los dos lados, cada uno con su parte).
- **Por qué arregla el peor caso de Clipper:** cada franja horizontal ve solo los bordes de su cuadrante. Con una grilla de k×k, los bordes activos por franja bajan del orden de k veces y la cantidad de trabajo de cada llamada, del orden de k².
- **Y reparte núcleos:** los cuadrantes son independientes.
- **Tamaño:** grilla adaptativa, hasta ~20 mil polígonos por cuadrante (se parte en 4 el que se pase).
- **gdstk-rs:** función nueva `xor_split_flat_in(a, b, rect) -> XorSplit` que filtra por bbox, recorta y hace el XOR en C++ (Clipper ya soporta la intersección con un rectángulo).
- **Efecto visible:** un polígono de diferencia que cruza un borde de cuadrante sale partido en dos. Las áreas, los bbox y las instancias no cambian. El conteo `+N polys` sí puede cambiar, solo en capas donde se usan cuadrantes. Se documenta; la comparación contra KLayout es por área y sigue igual.

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
| 6.2 | Índice espacial, LOD y triangulación en cache | `viewer-core`, backends, visor | L | O3; CI del crate de Carlos en verde |
| 6.3 | gdstk-rs seguro entre hilos + memoria | `external/gdstk` | M | O5; la RAM baja al liberar |
| 6.4 | `rayon` en el diff + presupuesto de memoria + cache | `riku-mod-layout` | M | O1 con 12 núcleos (< 10 s) y O2 |
| 6.5 | XOR por cuadrantes | `riku-mod-layout`, `external/gdstk` | M | O4 y áreas iguales a KLayout |
| 6.6 | `log`/`show`/`status` en paralelo | `riku/src/core/analysis` | S | salida idéntica; `log -n 200` < 1 s |

- 6.1 va primero porque es chico, no toca otros repos y resuelve el pendiente #2 casi por completo.
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
