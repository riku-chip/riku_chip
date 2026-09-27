# Métricas y Benchmarks — Riku

Mediciones de rendimiento del sistema Riku. Actualizado: 2026-04-19.

---

## Resumen ejecutivo

| Componente | Métrica | Resultado | Umbral crítico |
|---|---|---|---|
| Parser `.sch` | Tiempo por componente | ~7µs/comp | >100ms → considerar Rust |
| Diff semántico | Tiempo por diff | <1ms (<100 comps) | — |
| Git blob (pygit2) | Latencia por blob | 1.39ms (mismo commit) | — |
| Git sweep 20 commits | Latencia total | 46.5ms (2.27ms/blob) | — |
| SVG cache miss | Render + cache write | ~800ms | — |
| SVG cache hit | Path.exists() | ~0.2ms | — |
| `riku log --semantic` | Por diff (20 commits) | ~38ms/diff | >10s/100 commits → lazy+threads |
| SVG annotator | _fit_transform + annotate | <5ms (100 comps) | — |

---

## 1. Parser `.sch` — bench_parser.py (2026-04-19)

Archivos reales de `caravel_user_project_analog`:

| Archivo | Tamaño | Comps | Tiempo |
|---|---|---|---|
| `inv.sch` | 1.3 KB | 4 | 0.16 ms |
| `multiplicador_gilbert.sch` | 7.5 KB | 17 | 1.08 ms |
| `user_analog_project_wrapper.sch` | 7.7 KB | 66 | 0.89 ms |

Archivos sintéticos:

| Comps | Tamaño | Tiempo |
|---|---|---|
| 1000 | 72 KB | 6.9 ms |
| 2000 | 145 KB | 14 ms |

**Conclusión:** lineal a ~7µs/comp. Para alcanzar el umbral de 100ms se necesitarían ~14,000 componentes (~2MB de .sch). No existe en la práctica. **Este módulo no se migrará a Rust.**

---

## 2. Git service — bench_git_service.py (2026-04-19)

Operación `get_blob()` repetida 50 veces:

- Mismo commit: **1.39ms/blob** (std 0.31ms)
- Sweep de 20 commits distintos: **2.27ms/blob** (std 0.68ms)

Operación `get_commits()`:
- 20 commits, filtro por archivo: **~8ms total**

**Comparativa:** subprocess git ≈ 50-200ms/llamada. pygit2 es 35-140x más rápido.

**Conclusión:** pygit2 no es el cuello de botella en ningún escenario realista. **No requiere migración.**

---

## 3. Diff semántico — bench_semantic_diff.py (2026-04-19)

| Escenario | N comps | Tiempo |
|---|---|---|
| Sin cambios | 50 | <0.1 ms |
| Move All (todas coords) | 50 | <0.1 ms |
| 10% modificados | 100 | ~0.5 ms |
| 50% reemplazados | 100 | ~1 ms |
| Sintético grande | 1000 | ~8 ms |

**Desglose:** parseo Python (~15ms × 2 por diff) domina sobre la comparación de dicts (~0.1ms).

---

## 4. Cache SVG — bench_svg_cache.py (2026-04-19, Docker)

| Operación | Tiempo |
|---|---|
| Cache miss (render xschem) | ~800 ms |
| Cache hit (Path.exists + return) | ~0.2 ms |
| Speedup | ~3500x |

El render de xschem incluye: fork de proceso, carga del PDK, `zoom_full`, export SVG, escritura de `origins.txt`, cierre. Es la operación más lenta del sistema pero se paga solo una vez por versión de archivo.

---

## 5. riku log --semantic — bench_log_semantic.py (2026-04-19)

Repo sintético: 20 commits sobre un `.sch` con 50 componentes.

| Métrica | Valor |
|---|---|
| Tiempo total | 717 ms |
| Por diff | ~38 ms |
| Proyección 100 commits | ~3.8 s |

**Cuello de botella:** parseo Python (~15ms × 2 por diff) + overhead GitService (~2ms/blob). No es un problema algorítmico.

**Solución antes de Rust:** lazy evaluation (imprimir cada línea al calcularla) + `ThreadPoolExecutor(4)`. Estimación: llevan el caso de 100 commits a <1s. No se ha llegado a necesitar esto aún.

---

## 6. SVG annotator — bench_svg_annotator.py (2026-04-19)

| Función | N comps en SVG | Tiempo |
|---|---|---|
| `_extract_name_positions` | 100 | <1 ms |
| `_fit_transform` | 100 | <1 ms |
| `annotate()` completo | 100 | ~3 ms |
| `annotate()` completo | 1000 | ~25 ms |

**Conclusión:** el annotator no es cuello de botella en ningún escenario práctico.

---

## 7. Calibración de coordenadas SVG — medición directa (2026-04-19)

Comparación de métodos de estimación de `mooz` en `example_por.sch` (12 comps, 93 wires):

| Método | mooz estimado | Error típico en wires |
|---|---|---|
| Fit libre sin origins | 0.4443 | ~15-20px |
| Origins fijos + eje X de textos | 0.4541 | ~8-11px |
| Origins fijos + wire endpoints | **0.4517** | **<0.01px** |

La calibración desde wire endpoints (`_PATH_RE` sobre paths `M x yL x y`) es el método correcto. Los textos `#cccccc` tienen un offset tipográfico variable por símbolo que sesga el mooz en ~0.5%, causando 5-10px de desfase en wires distantes del origen.

Con 135 pares validados: `mooz = 0.4517 ± 0.0007` (2σ outlier rejection).

---

## Notas metodológicas

- Todos los benchmarks de Python corrieron en Windows 11 con Python 3.11.
- Los benchmarks de SVG cache y render corrieron en Docker (iic-osic-tools, Ubuntu).
- Los tiempos son medianas de múltiples ejecuciones salvo donde se indica.
- Los archivos reales son de `caravel_user_project_analog` (sky130 PDK).
- Ver `tests/bench_*.py` para el código fuente de cada benchmark.
