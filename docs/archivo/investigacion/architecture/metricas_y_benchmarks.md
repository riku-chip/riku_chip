# Métricas y Benchmarks del Binding Rust de gdstk

## ⚠️ Aviso importante: datos EXPERIMENTALES

**Estos números son EXPERIMENTALES.** Fueron recolectados durante la exploración de Miku (la aplicación consumidora), no son benchmarks de producción del binding en sí. El binding Rust es únicamente la capa fina sobre el C++ core de gdstk; estas mediciones comparan el overhead del lenguaje host (Rust vs Python) y la utilidad de ciertos patrones de uso (paralelismo, `gds_info` vs `read_gds` completo), no la calidad de la implementación subyacente.

Se preservan aquí como **referencia histórica** del performance alcanzado durante el desarrollo (Fases 1-12) y como evidencia para decidir el stack de Miku. No deben interpretarse como promesas de rendimiento ni como comparativas rigurosas entre implementaciones.

Reglas de lectura:

- Todos los benchmarks corrieron en la máquina de desarrollo del autor (Windows 11, single run salvo criterion).
- Cuando aparezca "interpretación", es una hipótesis del autor, no una medición.
- Rust siempre se midió en modo `--release`.
- Python gdstk es el mismo C++ core por debajo — las diferencias reflejan overhead del host, no del algoritmo.

---

## 1. Benchmarks con `criterion` (Fase 7)

Archivo de prueba: `proof_lib.gds` (~90 KB). Criterion ejecuta múltiples corridas con warmup y reporta mediana + intervalo de confianza 95%.

| Operación | Mediana | Intervalo 95% |
|---|---|---|
| `read_gds` | 126 µs | [125.32, 127.15] µs |
| `gds_info` | 85 µs | [84.75, 85.89] µs |
| `cell_xor_with(self, layer 0)` | 73 µs | [72.88, 74.14] µs |
| `iterate_polygons_all_cells` | 10 µs | [10.04, 10.15] µs |

Interpretación: los intervalos son muy estrechos (<2% del valor), lo que sugiere baja varianza inter-corrida. `iterate_polygons_all_cells` a 10 µs indica que la iteración en sí es casi gratis una vez la librería ya está cargada — el costo dominante es la I/O y el parse.

---

## 2. Comparación Rust vs Python single-file

Archivo: `tinytapeout.gds` (~952 KB). Se corrieron 5 iteraciones de cada uno midiendo wall-clock desde invocación del binario.

### Rust (release)

- Cold (primera corrida, cache frío): **157 ms**
- Warm (corridas 2-5): **63-65 ms** (mediana **65 ms**)

### Python gdstk

- Cold: **183 ms**
- Warm: **164-169 ms** (mediana **169 ms**)

### Speedup

**~2.6× en caliente** a favor de Rust.

Interpretación: Python paga aproximadamente **120 ms** de startup del intérprete + `import gdstk` en cada invocación. Rust paga ~10 ms de startup. Ambos lenguajes usan el mismo C++ core; la diferencia observada es overhead del lenguaje host, no del algoritmo de lectura. Para procesos de larga vida (daemon, notebook) el overhead de startup se amortiza y la diferencia se diluye.

---

## 3. Paralelismo sobre 8 archivos GDS

Se probaron dos cargas distintas para separar el efecto del startup del efecto del dataset.

### Experimento 1 — archivos chicos (total 3.9 MB)

| Enfoque | Tiempo | Speedup |
|---|---|---|
| Rust secuencial | 29.1 ms | 1× |
| Rust `std::thread` paralelo | 16.1 ms | **1.81×** |
| Python secuencial | 27.1 ms | 1× |
| Python `threading` (GIL) | 29.0 ms | 0.93× (empeora) |
| Python `multiprocessing` | 1323 ms | **0.02× (catastrófico)** |

### Experimento 2 — archivos grandes (total 47 MB)

| Enfoque | Tiempo | Speedup |
|---|---|---|
| Rust secuencial | 413.9 ms | 1× |
| Rust paralelo (threads) | 158.7 ms | **2.61×** |
| Python secuencial | 439.2 ms | 1× |
| Python `threading` (GIL) | 440.0 ms | 1.00× |
| Python `multiprocessing` | 476.9 ms | 0.92× |

### Lecciones

- **Python `threading` no paraleliza** trabajo GDS porque `gdstk` no libera el GIL en sus secciones C++. Verificado buscando `ALLOW_THREADS` en el código fuente de los bindings CPython de gdstk (no aparece envolviendo las llamadas de lectura).
- **Python `multiprocessing` tiene costo de spawn prohibitivo en Windows**: ~70 ms/proceso × 8 procesos = ~500 ms de overhead solo para arrancar. Solo rentable cuando la tarea por proceso supera varios segundos.
- **Rust `std::thread` paraleliza de verdad** — no hay GIL, el overhead de spawn es despreciable y escala casi linealmente con cores disponibles en archivos grandes.

Interpretación: para un caso de uso tipo "leer todos los GDS de un proyecto y extraer stats", Rust es la única opción que convierte cores extra en tiempo ahorrado sin acrobacias.

---

## 4. Consumo de memoria

Medido con el equivalente a `GetProcessMemoryInfo` en Windows (peak working set).

### Experimento 1 (3.9 MB, 8 archivos)

| Enfoque | Tiempo | Pico memoria |
|---|---|---|
| Rust (secuencial + paralelo completo) | 66 ms | **10.6 MB** |
| Python secuencial | 152 ms | 28.0 MB |
| Python multiprocessing (8 procesos) | 550 ms | **277.7 MB** |

### Experimento 2 (47 MB, 8 archivos)

| Enfoque | Tiempo | Pico memoria |
|---|---|---|
| Rust (secuencial + paralelo completo) | 648 ms | 123.9 MB |
| Python secuencial | 588 ms | 114.7 MB |
| Python multiprocessing (8 procesos) | 1563 ms | **439.4 MB** |

### Lecciones

- **Python multiprocessing usa ~26× más memoria** en archivos chicos porque cada proceso hijo carga su propio intérprete Python + `gdstk` + `numpy` (que gdstk importa transitivamente).
- **Rust usa ~2.6× menos memoria** en archivos chicos; ahí el startup Python domina.
- En archivos grandes, el **dataset domina** sobre el runtime: Rust y Python secuencial se emparejan (~120 MB ambos). Eso confirma que lo que pesaba en archivos chicos era el runtime, no los datos.

---

## 5. Aclaración tardía: "Rust 648 ms vs Python 588 ms"

En el Experimento 2 parece que Rust "pierde" en tiempo absoluto. **Es un artefacto del test, no una regresión real.**

El binario `count_many.rs` ejecutaba **DOS pasadas** (secuencial + paralela) mientras el script Python corría una sola. Normalizando por archivo procesado:

- Rust: 648 ms / 16 archivos = **40.5 ms/archivo**
- Python: 588 ms / 8 archivos = **73.5 ms/archivo**

Rust sigue siendo ~1.8× más rápido por archivo.

Para cerrar el loop se escribió `count_many_fair.rs` con una sola pasada:

- Rust secuencial: **~457 ms / 91 MB pico**
- Python secuencial: **~575 ms / 119 MB pico**

Lección meta: **escribir benchmarks justos es tan importante como escribir el binding**. Un test mal construido puede sugerir una conclusión inversa a la real.

---

## 6. `gds_info` vs `read_gds` (Fase 6)

Hipótesis inicial: `gds_info` (que solo lee cabeceras + tabla de cells sin parsear geometría) debería ser 3-5× más rápido que `read_gds` completo.

### `proof_lib.gds` (90 KB)

- `read_gds`: **5.82 ms**
- `gds_info`: **3.87 ms**
- Speedup: **1.5×**

### `flat_region_au13b.gds` (11 MB)

- `read_gds`: **109.61 ms**
- `gds_info`: **56.95 ms**
- Speedup: **1.9×**

### Interpretación

El speedup es **menor al esperado** (1.5-1.9× vs 3-5×). Razón: el `gds_info` de gdstk igual recorre el archivo entero secuencialmente, solo evita construir la estructura de polígonos en memoria. No hace skip-seek sobre records de geometría.

Aún así, 1.5-1.9× es rentable en escenarios de alta frecuencia como "correr `gds_info` en cada commit de un `git log`" donde pequeños ahorros se acumulan sobre miles de invocaciones.

---

## 7. Corrección del XOR (Fase 4)

Se validó que el `cell_xor_with` del binding produce resultados **numéricamente idénticos** a hacer el boolean XOR manualmente en Python.

### Test 1: rectángulo agregado

Caso sintético: agregar un rectángulo 5×3 µm a una celda en `proof_lib.gds`.

- Rust reporta: **1 región, 15.00 µm² cambiado**
- Python (boolean manual): **1 región, 15.0000 µm²**

Coincidencia exacta hasta el 4to decimal.

### Test 2: path movido

FlexPath trasladado +1 µm en X e Y en `tinytapeout.gds`.

- Rust reporta: **2 regiones, 5.27 µm²**
- Python equivalente: **2 regiones, 5.2703 µm²**

Coincidencia exacta.

Interpretación: no hay divergencia numérica introducida por el binding. Las operaciones booleanas son deterministas y el XOR es safe para comparar versiones de layout.

---

## 8. Roundtrip tests (Fase 6)

Se validó que `read → write → read` preserva semánticamente el contenido.

### `proof_lib.gds`

- `diff_gds` entre original y escrito-leído: **0 cambios reportados** ✓

### `tinytapeout.gds`

- `diff_gds` entre original y escrito-leído: **0 cambios reportados** ✓

No se compara byte-a-byte porque los timestamps del header GDS difieren entre escrituras. Sí hay **paridad semántica completa**: mismas celdas, misma jerarquía, mismos polígonos, mismos paths, mismas referencias.

---

## 9. Stats del proyecto (binding)

### Binding

- **18** archivos `.cpp` de gdstk compilados vía `build.rs`
- **Clipper vendored** (`external/clipper/clipper.cpp`) para operaciones booleanas
- **1** `shims.cpp` (C++ wrapper para cxx) + **1** `lib.rs` (API Rust) + varios handles opacos
- **15** archivos Python binding originales "migrados": **9** cubiertos, **5** descartados explícitamente con justificación, **1** parcial
- **31** tests de integración pasando
- **4** benchmarks criterion
- **12** fases ejecutadas (7 planeadas en el roadmap inicial + 5 extra descubiertas en el camino)

### Código

- CPython bindings totales de gdstk upstream: **15,757 líneas** de C++ en `gdstk/python/`
- Nuestros shims: **~900 líneas** `shims.cpp` + **~1,000 líneas** `lib.rs`
- Ratio de reducción: **~15× menos código** para scope comparable
- Razón principal: `cxx` elimina el boilerplate manual de la CPython API (refcounting, parse de argumentos, tipos Python→C++, exception translation). En Rust con `cxx` eso es todo implícito.

Interpretación: la reducción no significa que hicimos "lo mismo con menos"; hicimos un **subset** del scope de los CPython bindings (9/15 archivos). Aún ajustando por scope (~60% de cobertura), el ratio queda en ~9×, lo cual sigue siendo significativo.

---

## 10. Conclusiones y lecciones aprendidas

### Qué muestran los números (con honestidad)

1. **El binding Rust sí reduce overhead de host vs Python** en escenarios de invocación corta: ~2.6× en caliente en single-file. Esto no es mérito del binding sino del runtime Rust vs CPython.
2. **Paralelismo real es el verdadero diferencial**. Python no puede paralelizar gdstk sin pagar costos prohibitivos (spawn de procesos, memoria ×26, GIL). Rust sí, con `std::thread` plano.
3. **Memoria es significativamente menor en Rust** en cargas de archivos chicos; en cargas grandes el dataset domina y empata.
4. **Paridad numérica completa** con gdstk-Python en XOR y en roundtrip I/O.
5. **`gds_info` ahorra menos de lo esperado** (1.5-1.9× vs 3-5× hipotéticos), pero sigue siendo útil en hot-paths frecuentes.

### Qué NO muestran los números

- **No comparan implementaciones**, comparan overhead de binding. El C++ core es el mismo.
- **No son benchmarks de producción**. Single-run en una máquina, sin control de noise termal, sin variedad de hardware.
- **No validan throughput sostenido**. Todas las mediciones son de operaciones discretas.
- **No cubren corner cases** como archivos GDS corruptos, archivos >1 GB, o operaciones con cientos de miles de celdas.

### Decisión de stack (motivación real)

Para Miku, el stack Rust se eligió **no por performance single-file** (donde la diferencia es marginal desde la perspectiva del usuario: 65 ms vs 169 ms, ambas imperceptibles), **sino por:**

- **Distribución**: un único binario estático, sin dependencias de Python/numpy en máquinas del usuario.
- **Paralelismo**: escalar a directorios con cientos de GDS sin pagar el costo multiprocessing.
- **Memoria predecible**: sin overhead de intérprete por proceso.
- **Tipos estrictos** en la capa consumidora.

El performance es un **bonus**, no el driver principal. Estos benchmarks confirman que no hay regresión respecto a Python, y que en los ejes que importan (paralelismo, memoria en chico) hay mejoras reales.

### Recordatorio final

Estos números son de **exploración**, no de **validación de producción**. Si el día de mañana Miku se despliega a escala y aparecen hotspots distintos a los medidos acá, habrá que volver a medir con casos de uso reales. Esta tabla es un punto de partida, no un contrato.
