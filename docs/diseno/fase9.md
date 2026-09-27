# Fase 9: revisión del proyecto — bugs, rendimiento, estructura y librerías

Estado (2026-09-27): **plan** (revisión hecha, nada implementado). Resumen en [`../roadmap.md`](../roadmap.md).

Revisión de todo el código de `riku_chip` (~28 000 líneas) y de la parte Rust de `gdstk_rust`, en cuatro zonas: núcleo y contratos (`riku-kernel`, `viewer-core`, registro), layouts (`riku-mod-layout` + gdstk-rs), núcleo de análisis y módulos (`riku/src/core`, `riku/src/modules`) y CLI + visor (`riku/src/cli`, `riku/src/gui`). Criterio: bugs reales, SOLID solo donde duele (sin sobreingeniería), respetar el microkernel, y rendimiento (CPU, RAM, velocidad) con árboles/grafos/índices solo donde la ganancia es concreta.

**Conclusión general:** el proyecto está sano. El microkernel se respeta (el núcleo no mira extensiones; los módulos no dependen entre sí). Lo de las fases 6–7 (huellas Merkle, gemelas, XOR por cuadrantes, índice espacial del visor, grafo del historial) está bien hecho y no hay que tocarlo. Hay bugs reales (varios chicos) y costos que conviene bajar.

Leyenda: **[v]** verificado leyendo el código durante la revisión; **[r]** reportado por la revisión, a confirmar al implementar. Esfuerzo: S (horas), M (1–2 días), L (más).

## 9.1 Bugs (primero; cada uno con su test)

| # | Qué pasa | Dónde | Arreglo | Esf. |
|---|---|---|---|---|
| B1 [v] | Si un módulo falla (GDS roto, lado ilegible), el error va como *aviso*: `FileChange` queda vacío, se imprime "Sin cambios semánticos" y `riku diff --ci` sale con **0**. Un chequeo de CI pasa en silencio | `riku-kernel/src/change.rs:12-25`, `riku/src/modules/layout.rs:150`, `riku/src/cli/commands.rs:58` | `FileChange.error: Option<String>` (serde default/skip); los módulos lo llenan en el camino de error; `is_empty()` es falso con error; la CLI sale con 2 | S–M |
| B2 [v] | **Renombre + modificación:** `log`, `status` y `diff` de todo el repo leen el lado "antes" con la ruta **nueva** (`cf.path`): el blob no está y sale "todo añadido". `show` lo hace bien (`old_path`) | `core/analysis/log/walk.rs:158,185`, `status/analyze.rs:112`, `diff_set.rs:111,135` | leer "antes" con `old_path` (también en el costo de las tandas) | S |
| B3 [v] | Un lado omitido por tamaño (>50 MB, `LargeBlob`) o ilegible se vuelve **vacío** (el diff dice "todas las celdas añadidas/eliminadas"); los avisos se pierden en `FileSummary::from_report_with`, así que un archivo corrupto queda `Unchanged` y `log` lo oculta. `status` lee el disco sin límite | `blob_io.rs:42-47`, `show.rs:106`, `diff_set.rs:117`, `summary/build.rs:27-41`, `status/analyze.rs:139` | lado faltante → `FileSummary::error` sin llamar al módulo; avisos sin cambios → categoría `Error` (o campo `warnings` en el resumen) | S |
| B4 [v] | Un `.raw` ASCII con más valores que variables en una fila hace **pánico** (`columns[c]` fuera de rango) y tumba `riku log` entero (cruza rayon) | `modules/spice/raw.rs:292-294` | error si `row.len() > n` | S |
| B5 [v] | Los **avisos** de gdstk (`MissingReference`, `UnsupportedRecord`, `InvalidRepetition`…, códigos 1–8) se tratan como error fatal: el lector devuelve `nullptr`. Un GDS con una referencia a una celda externa o con registros de stream-out comercial (`PLEX`, `FORMAT`) no se puede comparar ni ver | gdstk_rust `rust/src/shims.cpp:305-308` (`read_gds_with_error`, `read_oas_with_error`), `rust/src/lib.rs:456` | código < `ChecksumError` → devolver la `Library` + el aviso; mostrarlo en `GdsError`/avisos. Probar con un fixture con una SREF sin resolver | S |
| B6 [v] | `riku diff A top.mag -f visual` contra el **disco**: el visor arma `commit_files(&svc, ":worktree")` y no encuentra las sub-celdas (la CLI sí, por `diff_set::sources`) | `riku/src/gui/app.rs:476-477` | si el commit es `WORKTREE`, `files::workdir_files(svc.root())` | S |
| B7 [r] | Visor: (a) una vista de ondas abierta tapa el diff pedido desde History (`load_backend_diff` no hace `self.wave = None`); (b) `open_raw`, el diff de `.raw` y `open_path` con error no cancelan `pending_load`: la carga vieja llega y pisa la actual; (c) `open_path` no limpia `diff_ctx` (breadcrumb viejo); (d) `handle_history_request` fija `diff_ctx` antes de saber si la carga funciona | `gui/app.rs:368-411, 444-481, 583, 661, 1063` | `cancel_pending()` al empezar toda carga; `diff_ctx = None` en `open_path` y fijarlo solo si la carga sale bien. Se resuelve de raíz con el `enum Content` de 9.3 | S |
| B8 [r] | Un GDS corrupto con **ciclo** de celdas: `tree_prints` lo detecta, pero el diff cae a `get_polygons(depth=-1)`, que recursa sin fin → desborde de pila (aborta el proceso, no es un panic). Igual en el visor con `entry` explícito | `riku-mod-layout/src/prints.rs:224`, gdstk | orden topológico de celdas una vez (grafo); con ciclo: error/aviso y no aplanar | S |
| B9 [r] | Errores de Clipper ignorados: un fallo del booleano sale como "sin cambios" | gdstk_rust `shims.cpp:1243` (`(void)err`) | llevar el código a `XorSplit` y convertirlo en aviso | S |
| B10 [r] | `riku log -n 20 ARCHIVO` corta en los 20 commits más recientes **antes** de filtrar por archivo (y los merges se conservan siempre) | `log/walk.rs:37-43,99`, `cli/commands.rs:221-224` | filtrar en el revwalk hasta juntar `limit` visibles (pathspec, o comparar `tree_entry_id` padre/hijo: O(profundidad), no un diff de árbol completo como `commit_touches`) | M |

Menores (S): `shell_complete.rs:33` corta un string con `+1` después de un espacio multibyte (NBSP) → pánico; `path_matcher.rs:10` descarta en silencio globs inválidos (si todos lo son, pasa todo); `app.rs:845` detecta la cancelación por el texto "cancelled"; `app.rs:433` convierte un error de lectura en "archivo borrado" (`unwrap_or_default`); el botón Reload no recarga History y `history.repo` queda fijo al arrancar; unidades distintas entre lados (nm vs µm) no se normalizan en el XOR (`gds_diff.rs:347,443`: leer con `unit=1e-6` o avisar); `RIKU_MAG_PATH.split(':')` (usar `std::env::split_paths`); `array` de Magic con rango enorme sin tope (OOM al aplanar); zoom sin límite con bbox degenerada (`viewport.rs:34-55`, escala ~1e11, egui pierde precisión en f32).

## 9.2 Rendimiento

### Lo que más rinde

| # | Qué | Hoy | Arreglo | Ganancia | Esf. |
|---|---|---|---|---|---|
| P1 [r] | History lanza **un hilo por commit seleccionado** (sin espera ni tope) y **redibuja a 60 fps** mientras calcula resúmenes | `gui/history/mod.rs:169-183, 208`; `app.rs:895` | debounce ~150 ms; un solo trabajo en vuelo (gana el último); no pedir detalles con el panel cerrado; el hilo llama `ctx.request_repaint()` al terminar y `request_repaint_after(250ms)` para el estado | de ~1 núcleo al 100 % a ~0 en espera; sin picos de RAM al recorrer con ↓ | S–M |
| P2 [v] | Offsets de repetición **O(n²)**: `repetition_offset_at` genera todos los offsets en cada `idx`. Un AREF de 1000×1000 **cuelga** el visor (labels sin tope); `hier_walk` hasta 4096² | gdstk_rust `shims.cpp:204-218`; `labels.rs:88,104`, `hier_walk.rs:96`, `prints.rs:355` | una llamada FFI `repetition_offsets(rep) -> Vec<Point2D>` | O(n²) → O(n) | S |
| P3 [v] | `Origins::of` escanea **todas** las instancias por cada polígono del XOR: O(polígonos × instancias). Cambiar una celda estándar usada 50 000 veces = 50k×50k chequeos por capa y por ancestro | `riku-mod-layout/src/hier_walk.rs:119-133` | índice espacial de las instancias (reusar `BoxGrid` de `prints.rs` con consulta de candidatos) | segundos/minutos → ms | S–M |
| P4 [r] | `find_cell` **lineal**, llamado en bucles (una vez por instancia en labels) | gdstk_rust `lib.rs:478`; `labels.rs:102`, `gds_diff.rs:385,671,687,730` | índice nombre→celda en el motor (en `finish_load`, o `OnceLock<HashMap>`) | O(n·m) → O(n) | S |
| P5 [r] | El visor **relee todo en cada clic de celda**: las dos librerías (con tempfile), re-colecta Magic, rehashea entradas, recalcula `tree_prints` | `viewer_core_compat.rs:354, 426-431`; `gds_diff.rs:454` | cachear en `GdsBackend` el último par (digest → `Arc<lados leídos + TreePrints>`); `Library` ya es `Send + Sync` | ~1–3 s por clic en el chip de 42 MB | M |
| P6 [r] | RAM del índice del visor (**2,5 GB** con 42 MB): tres copias vivas (`FlattenedPolygons` C++ → `Vec<DrawCommand>` → `DrawElement`) | `scene.rs:39-46`, `viewer_core_compat.rs:221-261` | `LayerKeys` primero (con `lib.layers()`), aplanar por piezas (`Pieces`), convertir directo a `DrawElement` por capa, quitar `DrawCommand` | pico ~40–60 % menor | M |
| P7 [v] | `MemBudget` bloquea un worker de **rayon** en un `Condvar` dentro de `xor_layer`: si el hilo que tiene el cupo roba (en el `par_iter` de cuadrantes) otra tarea que pide cupo, se cuelga. Raro en Linux (cupo = mitad de la RAM libre) | `prints.rs:451, 690-707` | como en `log` (fase 6.6): planificar desde afuera con las huellas (chicas en `par_iter`, grandes en secuencia), sin `Condvar` | elimina un cuelgue difícil de diagnosticar | M |
| P8 [r] | **Xschem** relee `.xschemrc`, recorre el PDK y **reparsea todos los `.sym`** en cada parseo (2 por diff); el caché de símbolos vive un solo parseo | `modules/xschem.rs:15-26`; `external/xschem-viewer-rust/src/scene.rs:59,307` | memoizar opciones de render; caché de símbolos del proceso (`Arc<RwLock<HashMap<PathBuf, Arc<..>>>>`) pasado por `RenderOptions` (toca el crate de Carlos: coordinar) | 3–10× en `.sch` dentro de `log`/`show` (estimado; medir) | M |
| P9 [r] | Una llamada FFI **por vértice** (~70 M llamadas con 13,8 M polígonos) | gdstk_rust `lib.rs:730, 1709-1716` | `polygon_points() -> &[Point2D]` (mismo layout que `Vec2`; `static_assert`) y copia en bloque en `XorSplit` | ~10–25 % en huellas y conversión | S |
| P10 [r] | Lecturas de Git sin OIDs: 4 revparse + recorrido de árbol por archivo; `get_blob` infla el blob entero antes de mirar el tamaño y `to_vec` duplica el pico | `log/walk.rs:158-190`, `git/blob.rs:8-32` | `old_oid`/`new_oid` en `ChangedFile` (del delta), `get_blob_by_oid`, `read_header` antes de inflar; saltar `old_oid == new_oid` | poca CPU; no inflar blobs >50 MB; pico de RAM a la mitad | S–M |
| P11 [r] | Armar el índice del visor no se puede cancelar (earcut, grillas y pirámides sobre millones de elementos) | `viewer-core/src/index.rs:260-371` | revisar el token antes de `build_index`; opcional: `&dyn Fn() -> bool` entre fases | cambiar de celda rápido no deja trabajo colgado | S |

### Por cuadro de dibujo (todos S)

- `SceneIndex::fill(i)` hace un `HashMap::get` por polígono y por cuadro, también en convexos (`viewer-core/src/index.rs:179, 662`): buscar solo con `Fill::Triangles`; mejor aún, triángulos en formato CSR como `Grid` (sin `Box` por polígono cóncavo).
- `scene_painter.rs:364`: un `Vec<Pos2>` nuevo por polígono (hasta 60 000) y `layer_colors` por polígono (`:360-362`): buffer reutilizable y colores solo al cambiar de capa (~0,5–1,5 ms por cuadro).
- Fantasmas y anotaciones del diff sin recorte (`scene_painter.rs:257, 324`): saltar lo que no cruza la vista o mide <1 px.
- `label_layout.rs:62, 124`: colocación de etiquetas O(n²) → grilla hash de ~64 px (O(n)).
- `wave_view.rs`: `signal_names` O(n²) llamado 4–5 veces por cuadro (`:312`), búsquedas lineales (`:192, 852`), lista de señales sin virtualizar (`:846`): caché de nombres + `HashMap` + `show_rows`. El diezmado es sobre todo el eje (4 000 puntos): con zoom 100× la forma sale mal → diezmar el rango visible (búsqueda binaria + ~2× el ancho en px, caché por rango cuantizado) (M).
- Clones por cuadro: `project_tree` (`app.rs:1080`), recorrer/ordenar el árbol (`:1113`), el `LogCommit` seleccionado (`history/mod.rs:491, 520`).
- La caché de texturas de cobertura usa la dirección del `SceneIndex` como clave (`scene_painter.rs:447`): una escena nueva puede reusar la dirección y mostrar la textura de otra → contador `scene_gen` en `BackendState`.

### Menores

- Spice: búsqueda de señales O(S²) con `eq_ignore_ascii_case` (`compare.rs:122-131`, `raw.rs:83`) → `HashMap` por nombre en minúsculas (con 10⁴ señales post-layout son ~2·10⁸ comparaciones). `raw.rs:240` pierde la capacidad al clonar `Vec::with_capacity`.
- `log`: el costo de un commit es la **suma** de sus archivos, pero se procesan en secuencia: usar `max()` (`walk.rs:155-162`).
- `handles_path` construye `info()` (Strings, Vec y, en Xschem, detección del PDK) por cada ruta y módulo: `FormatModule::extensions() -> &'static [&'static str]` (ver 9.3).
- Construcción del índice: el mapa `order` se recorre una vez por pirámide (`index.rs:436`); con `parallel`, las dos pirámides con `rayon::join`.
- `RIKU_PROFILE` leído con `env::var_os` en bucles calientes (`gds_diff.rs:538`, `prints.rs:453`) → `OnceLock`.
- Cache de diffs: SipHash con dos semillas sobre todas las entradas en cada lookup → digest (p. ej. xxh3-128) una vez por carga.
- `diff_layer` clona polígonos a los buckets y el camino del reporte calcula `CellDiff.polygons` para descartarlo.
- `M1` visor: `fs::read`, blobs de Git y parseo de `.raw` en el hilo de la UI (100–300 ms congelado con 42 MB) → a la tarea async; `source.as_ref().clone()` copia el archivo entero en cada cambio de celda/pestaña (84 MB en Diff): tolerable frente al parseo, no justifica romper el contrato ahora.

**Árboles y grafos:** no hace falta un R-tree ni un BVH: el índice del visor ya es un quadtree plano ("loose", grillas CSR por tamaño) y una consulta por ventana con presupuesto no ganaría. Donde sí rinde: el índice de instancias (P3), el índice nombre→celda (P4), y **el grafo de celdas en orden topológico calculado una vez** (detecta ciclos, B8; habilita piezas recursivas: hoy las piezas son de un nivel y un TOP→CORE aplana CORE entero, ~1,2 GB — M–L, `prints.rs:30-56`). No vale la pena una caché de parseos por OID entre commits consecutivos de `log` (choca con el presupuesto de memoria y con el paralelismo; medir antes).

## 9.3 Estructura (SOLID sin sobreingeniería)

Solo cortes que se pagan solos:

- **Núcleo: un solo flujo `diff_pair`.** `log`, `status`, `show` y `diff_set` repiten "leer antes/después, armar `DiffFiles`, `diff_with`, juntar avisos", cada uno con su política (silent/lenient/propagar): de ahí B2 y B3. Generalizar `Side`/`read_side`/`sources` de `diff_set` a `Version { Rev, WorkTree, Absent }` + ruta por lado, y `diff_pair(repo, workdir, module, before, after, opts) -> FileChange` con una sola política. Sin traits nuevos (M; −80 líneas y 3 bugs). `commit_diff::analyze_diff*` es código muerto; `diff_set::analyze_all` va en secuencia mientras `show` va en paralelo.
- **Visor (`app.rs`, 1 683 líneas):** `gui/content.rs` (`enum Content { Empty, Scene(SceneView { state, diff: Option<DiffContext> }), Wave(WaveView) }`, reemplaza `backend_state` + `wave` + `diff_ctx`: B7 imposible por construcción); `gui/loader.rs` (~150 líneas: `start/poll/cancel`, E/S dentro de la tarea, `gen` para la caché de cobertura); `gui/canvas.rs` (líneas 1159–1290 + `ViewState`); `gui/details_panel.rs`. En `app.rs` quedan preferencias, barras, atajos, paneles y ruteo de History (~700 líneas). No partir más.
- **Layouts:** `source.rs` compartido por `gds_diff.rs` y el visor (lectura + clave de cache + avisos). Hoy la CLI usa etiquetas `"A"/"B"` y el visor `"antes"/"después"`: **no comparten la cache**, y `RIKU_MAG_LAMBDA` no está en la clave (ponerla en `params`). `diff_scene.rs` con `build_diff_scene`, `change_items`, `port_item`, `cell_presence_items`. `changed_cells` duplica el bucle de `diff_libraries_unnamed` → `modified_cells()` (la GUI podría sacar "changed" del "report" cacheado). Cerrar la API pública a lo que usa `riku` (`diff_layout_sides`, `DiffCache`, `GdsBackend`, `mag::*`); el resto `pub(crate)` (ejemplos detrás de una feature). No hacen falta traits de backend.
- **Microkernel (fugas chicas):**
  - Listas fijas de extensiones en `cli/shell.rs:93`, `shell_complete.rs:25` y la ayuda (`:231/245/250`), sin `raw`; `app.rs:268` agrega `"raw"` a mano → `Registry::extensions()`.
  - `app.rs` usa `modules::spice` fuera de la excepción documentada (`is_raw`, `read_raw`, `raw_files`, `open_raw`, parseo del diff): moverlos a `wave_view` (la excepción queda en un solo archivo).
  - `summary/build.rs:129` filtra `x|y|rotation|mirror` (claves de Xschem en el núcleo), duplicado en `xschem_view.rs`: que el módulo no las emita como parámetros.
  - `friendly_error` (`app.rs:1650`) busca el texto "GDSII": el mensaje debe venir del backend.
  - `FormatModule::extensions() -> &'static [&'static str]` para `handles_path`; `info()` queda para `doctor`.
- **Contratos:** los métodos de carga crecen por acumulación (`load`, `load_entry`, `load_with`, `load_diff`, `load_diff_with`). No tocarlos (rompería a Carlos); **regla para adelante:** el próximo parámetro va en un `LoadRequest` con un método por defecto, no en un sexto método. `FileFormat` es un enum cerrado en el kernel: aceptable con 3 formatos; aclararlo en `arquitectura.md`. `legacy.rs` (v1) mete convenciones de layout/xschem en el kernel: moverlo a `cli/format` cuando se quite la v1 (`summary/build.rs:62` todavía depende de él).
- Otros: la GUI depende de `cli::format` (`eng`, `format_timestamp`) → módulo de utilidades compartido; `GitRepository` es ancho (`get_commits` legado, defaults vacíos que esconden implementaciones faltantes); `show.rs:84` convierte errores pasando por texto; el shell parte con `split_whitespace` (rompe `--expr "gain = v(out)/v(in)"`, un ejemplo de la ayuda, y rutas con espacios) → `shlex`; rutas relativas: una sola `to_repo_path(cwd, workdir, f)` para CLI y shell (`riku diff amp.sch` desde `repo/sub` busca en la raíz); `diff`/`show` en JSON siempre indentados (sin `--compact`); comentarios "Miku" en gdstk_rust (`lib.rs:3,950,1253,1475`, `shims.cpp:376`); `min_size` no hace lo que dice su doc (`index.rs:161, 667`); `files.rs:94` doc engañosa sobre `..`.

## 9.4 Librerías

| Librería | Hoy → última | Recomendación |
|---|---|---|
| `eframe`/`egui` + `egui_plot` | 0.34 / 0.35 → 0.36 / 0.37 | **Actualizar juntas** (M: cambios de API entre versiones menores de egui). Probablemente saca duplicados del árbol (`smithay-client-toolkit` 0.19/0.20, `calloop`, `rustix` 0.38/1.1, `thiserror` 1/2) |
| `rustyline` | 14 → 18 | **Actualizar** (S–M: API de `Helper`/`Completer`); saca `unicode-width` 0.1 duplicado |
| `git2` | 0.20 → 0.21 | Actualizar (S). `gix` (Rust puro, seguro entre hilos: simplificaría `reopener`) no conviene ahora: migración L para poca ganancia |
| `earcutr` | 0.5 | Probar `earcut` 0.4 (más rápido, mantenido) midiendo con `profile_view` antes de cambiar |
| `shlex` | — | **Agregar** (arreglo del shell) |
| `rust-i18n` | 4.2.2 → 4.2.3 | Parche; trae `itertools` 0.11 duplicado (no se puede evitar) |
| `clap`, `serde`, `rayon`, `tokio`, `thiserror`, `glob`, `dirs`, `cxx`, `pest` (Carlos) | al día | Bien así |
| `poll-promise` 0.3 | sin cambios hace tiempo | Bien así (chico y estable) |
| `async-trait` | — | Sigue haciendo falta (no hay `async fn` en traits usados como `dyn`) |
| `libc` | — | Solo para `SIGPIPE` (`main.rs:6`): correcto |
| R-tree (`rstar`) | — | **No** hace falta (ver 9.2) |

425 paquetes en el lock.

## 9.5 Está bien así (no tocar)

- Microkernel: el núcleo solo usa `Registry::for_path`; `FileSource`/`DiffFiles` en `viewer-core` (el crate más bajo); `Bounds` (JSON) separado de `BoundingBox` (visor); dos detecciones (`detect` y `accepts`) = la duplicación mínima para que el backend de Carlos sea independiente; `DiffOptions` como struct con `Default`; `Registry` como `Vec` lineal con 3 módulos; `Layer = u16` con índices densos.
- Índice espacial (grillas CSR por tamaño, cada elemento en ≤2×2 celdas), pirámide de cobertura con dos resúmenes, bboxes en `f32` redondeadas hacia afuera, earcut una vez al cargar.
- Huellas Merkle con memo y guarda de ciclos, gemelas como multiconjunto, `multiset_diff`, XOR local, forma canónica, SipHash de 64 bits, quadtree con `MAX_DEPTH`, `BoxGrid`, `LibraryBuilder` con un cruce FFI, `finish_load` + `Send/Sync`, destructores del lado C++ sin fugas, `DiffCache` (rename atómico, corruptas → recalcular, errores no cacheados).
- Magic: ciclos descartados, grilla común exacta, parseo paralelo por nivel, índice del PDK cacheado por tecnología, resolver por closure (motor sin Riku).
- `map_in_waves` (tandas planificadas en vez de semáforo), `graph.rs` (lineal por columnas activas), `GitFiles` perezoso, casos de Git (HEAD desacoplado, rama sin commits, clon superficial).
- Spice: `raw.rs` con binarios truncados e índices acotados; `expr.rs` sin pánicos; `compare.rs` (RMS por trapecios, NaN, punto de operación); interpolación O((n+m)·log n) con `partition_point`.
- CLI/visor: `dispatch.rs` único, `wants_json` + `riku-error/v1`, códigos de `--ci`, `resolve_targets`; filas de History y del selector de celdas virtualizadas; `HistoryModel` sin egui y con tests; `LayerBatch`, `OUTLINE_MIN_PX`, caché de cobertura de 3 niveles; carga única con `CancellationToken`; re-encuadre con animaciones interrumpibles; `label_layout` y `toast` puros y testeados; `wave_view` como excepción bien delimitada (salvo lo que se filtra a `app.rs`).

## Orden propuesto

1. **9.1 Bugs** B1–B10 (+ menores), cada uno con su test.
2. **9.2 Rendimiento:** P1, P2, P3, P4 (S, alto impacto); después P5–P11 y lo de por cuadro; medir antes/después (`profile_diff`, `profile_view`, `RIKU_PROFILE`, `mag_bench.sh`).
3. **9.3 Estructura:** `diff_pair` (núcleo), `Content` + `loader` (visor), `source.rs` (layouts), fugas del microkernel.
4. **9.4 Librerías:** egui/eframe/egui_plot juntos, rustyline, git2, `shlex`; probar `earcut`.

Cada paso con la verificación completa: suite con `-D warnings`, combinaciones de features, regresión de la fase 1, `compare.sh` (GDS), `compare_mag.sh` (Magic contra KLayout 0.30.12) y el crate de Carlos con `viewer-core-compat`.

## Avance

| Paso | Estado | Notas |
|---|---|---|
| Revisión | Hecha (2026-09-27) | cuatro revisiones en paralelo + verificación en el código de B1–B6, P2, P3, P7 |
| B1 | Hecho | `FileChange.error`; `diff`/`show`/`status` con `--ci` salen con 2 si un archivo no se pudo comparar; en `status`/`log` es la categoría `error` |
| B2 | Hecho | `before_path()`; log, status, diff de todos y el visor (History, `-f visual`) leen el lado "antes" con la ruta vieja. Además `status` listaba un renombre con la ruta vieja (`git2` da `entry.path()` = la vieja) |
| B3 | Hecho | `blob_io::Blob` (`Bytes`/`Missing`/`Skipped`) y `pipeline::diff_blobs`: un lado omitido (>50 MB, ilegible; también en disco) es error sin llamar al módulo; `FileSummary.warnings`, y `log` no oculta un archivo con avisos |
| B4 | Hecho | `read_ascii`: más valores que variables en un punto es error, no pánico |
| B5 | Hecho | gdstk_rust: un aviso del lector (códigos 1-8) devuelve la `Library` y `read_warning()`; riku lo muestra como aviso (qué celdas faltan) en el diff y en el visor |
| B6 | Hecho | `diff_set::token_files`: el visor contra `:worktree` lee las sub-celdas del disco |
| B7 | Hecho | `cancel_pending()` al empezar toda carga (abrir, diff, `.raw`); el diff saca la vista de ondas; `diff_ctx` se limpia al abrir un archivo y se fija solo si la carga arrancó. Verificado compilando y con la suite (el visor no tiene tests de estado); el arreglo de raíz es el `enum Content` de 9.3 |
| B8 | Hecho | `check_acyclic`: DFS iterativo por el grafo de referencias al leer un GDS/OASIS (diff y visor); con ciclo, error con el camino (`A → B → A`) en vez de desbordar la pila |
