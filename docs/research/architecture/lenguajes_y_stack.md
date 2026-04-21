# Selección de Lenguaje y Stack para Riku

## 1. Requisitos del sistema

Riku necesita:
- Interfaz con KLayout (API Python/Ruby, core C++)
- Interfaz con Xschem (Tcl, exporta SPICE)
- Interfaz con NGSpice (CLI batch)
- Interfaz con Magic VLSI (Tcl, CLI)
- Leer historial de Git y extraer archivos
- Parsear GDS binario (puede ser multi-GB)
- Parsear texto: netlists SPICE, `.mag`, `.sch`
- Correr en CI (GitHub Actions, etc.)
- GUI opcional (embebida o externa)

---

## 2. Análisis por lenguaje

### Python

**Ecosistema EDA disponible hoy:**
| Librería | Qué cubre | Estado |
|---|---|---|
| `klayout` (PyPI) | KLayout API completa headless | Activo, ~21k descargas/semana |
| `gdstk` | Parseo y manipulación GDS (core C++) | Activo, sucesor de gdspy |
| `spicelib` | Netlists SPICE + NGSpice batch + parseo `.raw` | Activo |
| `pygit2` | Git via libgit2 (C backend) | Activo |
| `gitpython` | Git puro Python | **Mantenimiento solamente** — no recomendado |

**Limitaciones:**
- GIL limita paralelismo real en lógica Python (no afecta gdstk que corre en C++)
- No hay parsers de `.mag` ni `.sch` en PyPI — habría que construirlos
- `gitpython` en modo mantenimiento; usar `pygit2` en su lugar

**Veredicto:** Cubre el 80% de los requisitos con librerías existentes. Ideal para MVP.

---

### Rust

**Ecosistema EDA disponible hoy:**
| Librería | Qué cubre | Estado |
|---|---|---|
| `gds21` | Parseo GDSII (in-memory, serde) | Activo, UC Berkeley |
| `lef21` | Parseo LEF | Activo, mismo workspace |
| `git2-rs` | Git via libgit2 | Producción, rust-lang org |
| `gitoxide` | Git puro Rust | Madurado, ~25% más rápido que libgit2 |
| `dan-fritchman/Netlist` | Parser de netlists multi-dialecto | Pequeño, activo |

**GUI:**
| Toolkit | Características | Relevancia |
|---|---|---|
| `egui` | Immediate mode, WASM/desktop | Prototipos y tooling |
| `iced` | MVU/Elm, stateful | GUIs complejas |
| `tauri` | Frontend web + backend Rust | Apps de escritorio pulidas |
| `cxx-qt` | Rust + Qt/QML (KDAB) | Si se quiere espejo de KLayout |

**Proyectos EDA en Rust (confirmados):**
- **Layout21 / gds21 / lef21** — UC Berkeley, Dan Fritchman (github.com/dan-fritchman/Layout21)
- **substrate2** — UC Berkeley, Sky130 PDK, pre-producción (github.com/ucb-substrate/substrate2)
- **LibrEDA** — Europeo, fondos NLnet/UE, P&R tools (libreda.org)
- **Copper** — PCB EDA editor en Rust

**Limitaciones:**
- `gds21` carga todo en memoria (no streaming) — no escala a GDS multi-GB sin trabajo adicional
- No hay parsers de `.spice`, `.mag`, `.sch` — construir desde cero
- `gitoxide` todavía no tiene `push` completo ni `merge`
- FFI para llamar a KLayout C++ desde Rust requiere binding manual
- Curva de aprendizaje más alta

**Veredicto:** Mejor opción a largo plazo para rendimiento y corrección. La comunidad EDA open source se está moviendo en esta dirección.

---

### Go

**Ecosistema EDA:** Prácticamente nulo. El namespace "EDA" en Go está dominado por Event-Driven Architecture.

**Git:** `go-git` es excelente (producción, usado por Gitea). Pero sin EDA libraries, habría que construir todo desde cero.

**Veredicto:** No aplica para Riku.

---

### C++

Máximo rendimiento. KLayout ya es C++ con Qt — embeber su API directamente es posible. Pero:
- Sin garantías de memory safety al parsear archivos de git history potencialmente corruptos
- Velocidad de desarrollo baja para un proyecto nuevo
- Fricción alta en CI (build system, linking, plataformas)

**Veredicto:** Solo si se decide embeber KLayout directamente. No para el stack completo.

---

### Julia

Nicho de simulación analógica (CedarSim.jl, con fondos DARPA). Sin parsers GDS, sin tooling de layout, sin GUI viable. Requeriría construir ~100% del stack desde cero.

**Veredicto:** No aplica.

---

## 3. Matriz de cobertura

| Requisito | Python | Rust | Go | C++ |
|---|---|---|---|---|
| KLayout API | ✅ nativo (PyPI) | ⚠️ FFI manual | ❌ | ✅ nativo |
| SPICE / NGSpice | ✅ spicelib | ⚠️ parcial | ❌ | ❌ |
| Magic (.mag) | ⚠️ construir | ⚠️ construir | ❌ | ❌ |
| Xschem (.sch) | ⚠️ construir | ⚠️ construir | ❌ | ❌ |
| Git | ✅ pygit2 | ✅ git2-rs | ✅ go-git | ✅ libgit2 |
| GDS binario | ✅ gdstk (C++ core) | ⚠️ gds21 (no streaming) | ❌ | ✅ gdstk directo |
| GUI | ✅ PyQt, Dear PyGui | ✅ egui, iced, tauri | ⚠️ Fyne | ✅ Qt |
| CI pipelines | ✅ excelente | ✅ bueno | ✅ bueno | ⚠️ difícil |
| Comunidad EDA | ✅ grande | 🔼 pequeña y creciendo | ❌ | ✅ establecida |

---

## 4. Recomendación: híbrido Python + Rust

El patrón más sólido para Riku es el mismo que usa gdstk:  
**Python en la superficie, Rust en el núcleo pesado.**

```
┌─────────────────────────────────────────┐
│              CLI / Orquestación          │  Python
│     KLayout API · spicelib · pygit2     │
├─────────────────────────────────────────┤
│         Parser GDS streaming            │  Rust (expuesto via PyO3)
│         Motor de diff GDS               │
└─────────────────────────────────────────┘
```

**Python cubre:**
- CLI y orquestación del flujo completo
- `klayout.db` para diff/XOR de GDS
- `spicelib` para NGSpice (netlists + `.raw`)
- Subprocess + parser de texto para Magic y Xschem
- `pygit2` para operaciones Git

**Rust cubre (fases posteriores):**
- Streaming de GDS binario multi-GB sin cargar todo en memoria
- Motor de diff de alto rendimiento
- Expuesto a Python via PyO3

**MVP:** Solo Python. Rust se agrega cuando el rendimiento sea un cuello de botella real.

---

## ¿Cuándo refutar estas decisiones?

**"Python para el MVP"** deja de ser válido si:
- Un diff de GDS de tamaño realista (>200 MB) tarda más de 30 segundos con `klayout.db` en Python — ese tiempo rompe la UX de uso interactivo y justifica mover antes a Rust.
- `klayout.db` via PyPI tiene un bug crítico que no se resuelve en la versión pip (ha pasado antes: algunas operaciones de `LayoutDiff` solo funcionan en la versión del sistema).

**"Rust solo para streaming GDS"** deja de ser válido si:
- Se necesita parsear `.raw` de NGSpice de varios GB con baja latencia — `spicelib` carga el archivo completo en memoria.
- El diff de netlist SPICE necesita ser más rápido de lo que Python permite para casos con miles de subcircuitos.

**"gdstk sobre gdspy"** deja de ser válido si:
- gdstk no mantiene compatibilidad con alguna variante de GDS que usen los PDKs objetivo (GF180, IHP). gdspy tiene más años de campo.

**"Rust se mueve hacia EDA"** no es una decisión — es contexto. No actuar sobre esto hasta tener una razón de rendimiento concreta.

---

## 5. Contexto de la comunidad EDA

La comunidad EDA open source se está moviendo hacia Rust para infraestructura nueva:
- Paper en ASP-DAC 2025 usando Rust como HDL embebido
- LibrEDA con fondos de la Unión Europea (NLnet)
- UC Berkeley invirtiendo en Layout21 / substrate2
- Artículo "EDA Needs to be Using Rust" (Jason McCampbell) argumentando que Rust resuelve los tres problemas históricos del C++ en EDA: rendimiento, uso de memoria y memory safety

No es mainstream todavía, pero la dirección es clara para los próximos 5 años.

---

## Referencias

### Librerías Python para EDA
- **klayout** (Python API): https://github.com/KLayout/klayout — `pip install klayout` da acceso a `klayout.db` sin display
- **gdstk**: https://github.com/heitzmann/gdstk — alternativa a gdspy, más rápida, escrita en C++
- **gdspy**: https://github.com/heitzmann/gdspy — precursora de gdstk, aún usada en muchos repos
- **spicelib**: https://github.com/nunobrum/spicelib — parse de netlists SPICE y archivos `.raw` de NGSpice
- **PySpice**: https://github.com/FabriceSalvaire/PySpice — generación programática de netlists
- **pygit2**: https://github.com/libgit2/pygit2 — bindings Python para libgit2, acceso a objetos git

### Librerías Rust para EDA
- **gds21**: https://github.com/dan-fritchman/Layout21/tree/main/gds21 — parser/writer GDS en Rust puro
- **Layout21**: https://github.com/dan-fritchman/Layout21 — framework de layout en Rust (incluye gds21, lef21)
- **git2-rs**: https://github.com/rust-lang/git2-rs — bindings Rust para libgit2
- **substrate2**: https://github.com/substrate-labs/substrate2 — framework EDA moderno en Rust (OSRE)

### Contexto de adopción Rust en EDA
- **OSRE (Open Source Rust EDA)**: https://github.com/osre-eda — organización de herramientas EDA en Rust
- Presentación "Rust in EDA" en ORConf 2023: buscar en https://orconf.org/

### Ver también
- [arquitectura_cli_y_orquestacion.md](arquitectura_cli_y_orquestacion.md) — cómo el stack se traduce en componentes
- [../operaciones/cache_y_rendimiento.md](../operaciones/cache_y_rendimiento.md) — justificación de Rust para streaming GDS
- [../herramientas/gds_klayout_magic_diff.md](../herramientas/gds_klayout_magic_diff.md) — disponibilidad de klayout.lay según paquete (PyPI vs sistema)
- [../herramientas/headless_y_compatibilidad_herramientas.md](../herramientas/headless_y_compatibilidad_herramientas.md) — dependencias Docker por herramienta y tamaños de imagen
