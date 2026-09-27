# Plan: Migración gdstk Python → Rust (cxx)

> Reemplazar los bindings CPython de gdstk (~15,757 líneas) con bindings Rust usando el crate `cxx`, manteniendo el core C++ intacto.

---

## 1. Objetivo

Exponer el motor C++ de gdstk a Rust para que Miku pueda consumirlo directamente, sin pasar por Python.

**Contexto:** Miku es un VCS especializado para diseño de chips IC (Dante + Carlos Cueva). Necesita boolean XOR, `area()`, y parseo de GDS — todo lo que gdstk ya resuelve en C++, pero actualmente solo expuesto vía Python.

---

## 2. Beneficios esperados

| Beneficio | Impacto concreto |
|---|---|
| Sin GIL | Diff paralelo de múltiples commits con `rayon` |
| Binario único | `miku` se distribuye sin runtime Python |
| Tipos en compilación | Errores detectados antes de ejecución |
| Stack unificado | Todo Miku en Rust, un solo lenguaje |
| Memoria segura | Ownership garantizado por Rust |

**Lo que NO cambia:** performance del XOR y parseo (el C++ core es el mismo).

---

## 3. Arquitectura objetivo

```
gdstk/
├── src/          # C++ core — sin cambios
├── include/      # headers C++ — sin cambios
├── external/     # Clipper — sin cambios
├── python/       # CPython bindings — referencia, no tocar
└── rust/         # NUEVO
    ├── Cargo.toml
    ├── build.rs
    └── src/
        ├── lib.rs
        ├── parsing.rs
        ├── repetition.rs
        ├── polygon.rs
        ├── label.rs
        ├── rawcell.rs
        ├── raithdata.rs
        ├── reference.rs
        ├── cell.rs
        ├── curve.rs
        ├── flexpath.rs
        ├── robustpath.rs
        ├── library.rs
        └── gdswriter.rs
```

---

## 4. Herramientas

| Herramienta | Rol |
|---|---|
| `cxx` crate | Bridge C++ ↔ Rust declarativo |
| `cc` crate | Compila C++ desde `build.rs` |
| `bindgen` (fallback) | Si `cxx` no cubre algún caso |
| zlib, qhull | Mismas dependencias C++ que gdstk |

---

## 5. Fases de implementación

### Fase 1 — Setup del build (1-2 días)
- Crear `Cargo.toml` con dependencias `cxx`, `cc`
- Escribir `build.rs` que compila:
  - Todos los `.cpp` de `src/`
  - `external/clipper/clipper.cpp`
  - Linkea zlib y qhull
- Verificar que linkea correctamente con un bridge mínimo de prueba

**Entregable:** `cargo build` exitoso, aunque `lib.rs` esté vacío.

---

### Fase 2 — Tipos base (2-3 días)
- `parsing.rs` — conversiones básicas (Tag, Vec2, ErrorCode)
- `repetition.rs` — clase `Repetition` (sin dependencias)
- `polygon.rs` — clase `Polygon` con `area()`, `layer`, `datatype`

**Entregable:** poder crear un Polygon desde Rust y calcular su área.

---

### Fase 3 — Clases standalone simples (2-3 días)
- `label.rs` — etiquetas de texto
- `rawcell.rs` — celdas crudas (sin parsear)
- `raithdata.rs` — datos RAITH e-beam (especializado)

**Entregable:** todas las clases simples expuestas con tests unitarios.

---

### Fase 4 — Clases compuestas (3-5 días)
- `reference.rs` — referencias a celdas (usa polygon, repetition)
- `cell.rs` — la clase más importante, contiene arrays de todo

**Entregable:** poder crear una Cell con polígonos y referencias.

---

### Fase 5 — Curvas y paths (3-5 días)
- `curve.rs` — curvas geométricas
- `flexpath.rs` — paths flexibles
- `robustpath.rs` — paths robustos

**Entregable:** paths completos, opcionales para Miku MVP.

---

### Fase 6 — Top-level (2-3 días)
- `library.rs` — clase Library, contiene cells
- `gdswriter.rs` — escritor streaming
- `lib.rs` — entry point, funciones globales (`read_gds`, `boolean`)

**Entregable:** `gdstk::read_gds("chip.gds")` funciona desde Rust.

---

### Fase 7 — Verificación (3-5 días)
- Portar tests de `tests/*.py` a Rust
- Benchmarks comparativos Python vs Rust
- CI con `cargo test`

**Entregable:** paridad funcional con Python confirmada.

---

## 6. Estimación total

| Fase | Días |
|---|---|
| 1. Setup | 1-2 |
| 2. Tipos base | 2-3 |
| 3. Standalone | 2-3 |
| 4. Compuestas | 3-5 |
| 5. Curvas/paths | 3-5 |
| 6. Top-level | 2-3 |
| 7. Verificación | 3-5 |
| **Total** | **3-4 semanas** (1 dev) |

---

## 7. Riesgos técnicos

### 7.1 Complejidad C++ no cubierta por cxx
gdstk usa contenedores custom (`Array<T>`, `Set<T>`, `Map<T>`) y allocators personalizados. `cxx` puede no mapear todo directamente.

**Mitigación:** escribir wrappers C++ simples en `rust/cpp_shims/` que expongan APIs cxx-friendly.

### 7.2 Gestión de memoria
gdstk usa `malloc`/`free` manual en algunos paths. Rust `UniquePtr` debe coincidir exactamente con cómo gdstk libera memoria.

**Mitigación:** leer cuidadosamente los destructores C++ antes de exponer cada clase.

### 7.3 Clipper como dependencia embebida
Clipper está vendored en `external/`. El `build.rs` debe compilarlo y linkearlo correctamente.

**Mitigación:** copiar la configuración de `CMakeLists.txt` existente.

### 7.4 Thread safety
gdstk no documenta claramente qué es thread-safe. El beneficio principal de Rust (paralelismo) requiere garantías explícitas.

**Mitigación:** marcar tipos como `!Send`/`!Sync` por defecto, relajar solo tras análisis.

---

## 8. Criterios de éxito

1. Los 15 archivos binding de Python tienen equivalente Rust funcional
2. `read_gds` + `boolean("xor")` funcionan desde Rust
3. Los tests portados pasan
4. Benchmarks: Rust ≥ Python en performance
5. Miku puede hacer `miku diff chip.gds` usando los bindings Rust

---

## 9. Mapeo Python → Rust

| Python (`python/`) | Rust (`rust/src/`) | Prioridad |
|---|---|---|
| `gdstk_module.cpp` | `lib.rs` | Alta |
| `library_object.cpp` | `library.rs` | Alta |
| `cell_object.cpp` | `cell.rs` | Alta |
| `polygon_object.cpp` | `polygon.rs` | Alta |
| `parsing.cpp` | `parsing.rs` | Alta |
| `reference_object.cpp` | `reference.rs` | Alta |
| `rawcell_object.cpp` | `rawcell.rs` | Media |
| `label_object.cpp` | `label.rs` | Media |
| `flexpath_object.cpp` | `flexpath.rs` | Baja |
| `robustpath_object.cpp` | `robustpath.rs` | Baja |
| `curve_object.cpp` | `curve.rs` | Baja |
| `gdswriter_object.cpp` | `gdswriter.rs` | Baja |
| `repetition_object.cpp` | `repetition.rs` | Media |
| `raithdata_object.cpp` | `raithdata.rs` | Baja |
| `docstrings.cpp` | — | No aplica |

---

## 10. Decisiones pendientes

- **¿Fork de gdstk o subárbol?** ¿Mantenemos sync con upstream o divergimos?
- **¿Publicar como crate propio?** `gdstk-rs` en crates.io vs interno a Miku
- **¿Mantener Python?** ¿Eliminamos `python/` o coexisten ambos bindings?
- **¿Quién mantiene?** ¿Contribuir upstream al proyecto de Heitzmann?

---

## 11. Próximos pasos inmediatos

1. Fork/copy de gdstk en ubicación de trabajo
2. Crear `rust/Cargo.toml` con dependencias
3. Escribir `build.rs` mínimo que compile `src/`
4. Bridge `cxx` de prueba exponiendo solo `read_gds`
5. Test manual: leer un GDS desde Rust y contar celdas

Si la Fase 1 funciona en 2 días, el resto del plan es ejecutable. Si la Fase 1 tarda más de una semana, pausar y reevaluar (posiblemente usar Python directamente para el MVP).
