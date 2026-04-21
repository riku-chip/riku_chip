# Investigación — Riku

Riku es un VCS especializado para diseño de chips IC analógico, construido sobre Git. Soporta KLayout, Xschem, NGSpice y Magic VLSI.

**Prioridades de diseño:** rendimiento, tamaño mínimo de artefactos en el repo, experiencia de usuario práctica.

---

## Estructura

```
research/
├── arquitectura/          # Decisiones de stack y diseño del sistema
├── herramientas/          # Integración con cada herramienta EDA
├── operaciones/           # Operaciones específicas: merge, CI, caché
└── ux/                    # Experiencia de usuario: flujos reales y variaciones
```

---

## arquitectura/

| Documento | Descripción |
|---|---|
| [lenguajes_y_stack.md](arquitectura/lenguajes_y_stack.md) | Análisis Python vs Rust vs otros. Recomendación: Python (MVP) + Rust (streaming GDS). |
| [arquitectura_cli_y_orquestacion.md](arquitectura/arquitectura_cli_y_orquestacion.md) | Diseño de comandos `miku diff/merge/blame/log/ci`, plugin system, `miku doctor`, `miku.toml`. |
| [metricas_riku.md](arquitectura/metricas_riku.md) | Benchmarks de Riku: parser, git service, diff semántico, cache SVG, annotator. Medidos 2026-04-19. |
| [metricas_y_benchmarks.md](arquitectura/metricas_y_benchmarks.md) | Benchmarks experimentales del binding Rust de gdstk (Miku). Referencia histórica. |
| [svg_annotator_coordenadas.md](arquitectura/svg_annotator_coordenadas.md) | Sistema de calibración de coordenadas .sch → SVG. Formula Xschem, origins.txt, calibración desde wire endpoints. |
| [gotchas_xschem.md](arquitectura/gotchas_xschem.md) | 8 problemas concretos encontrados con Xschem y el SVG annotator, con causa y fix documentados. |
| [gotchas_tecnicos.md](arquitectura/gotchas_tecnicos.md) | 20 gotchas del binding Rust de gdstk (enums dispersos, CRLF, cxx limitaciones, etc.). |
| [decisiones_tecnicas.md](arquitectura/decisiones_tecnicas.md) | Decisiones de arquitectura y refactors del módulo Riku (stack, diff strategy, modelos). |
| [gdstk_rust_bindings_migracion.md](arquitectura/gdstk_rust_bindings_migracion.md) | Plan de migración de gdstk a Rust con PyO3. |
| [gdstk_rust_decisiones.md](arquitectura/gdstk_rust_decisiones.md) | Decisiones específicas del binding Rust de gdstk. |

---

## herramientas/

| Documento | Descripción |
|---|---|
| [gds_klayout_magic_diff.md](herramientas/gds_klayout_magic_diff.md) | GDS binario: diff estructural vs XOR geométrico con KLayout. Formato .mag y Magic headless. |
| [ngspice_diff_y_versionado.md](herramientas/ngspice_diff_y_versionado.md) | Formatos SPICE, diff de netlists, waveform regression con `.meas` y tolerancias. |
| [xschem_diff_y_ecosistema_eda.md](herramientas/xschem_diff_y_ecosistema_eda.md) | Formato .sch, diff nativo de Xschem, comparativa con plotgitsch/KiRI, gap del driver git. |
| [headless_y_compatibilidad_herramientas.md](herramientas/headless_y_compatibilidad_herramientas.md) | Modo batch sin X11, versiones mínimas por herramienta, Docker ~350MB, WSL2/macOS. |

---

## operaciones/

| Documento | Descripción |
|---|---|
| [estrategia_merge_archivos_mixtos.md](operaciones/estrategia_merge_archivos_mixtos.md) | Merge automático por celdas disjuntas, deps.toml, drivers de git, resolución visual. |
| [ci_drc_lvs_regresiones.md](operaciones/ci_drc_lvs_regresiones.md) | DRC/LVS headless, waveform regression, delta de violaciones, YAML para GitHub/GitLab. |
| [cache_y_rendimiento.md](operaciones/cache_y_rendimiento.md) | Tiempos reales de 16 operaciones, caché L1/L2, XOR selectivo 150x más rápido, streaming Rust. |

---

## Mapa de dependencias

```
lenguajes_y_stack
    └── arquitectura_cli_y_orquestacion
            ├── gds_klayout_magic_diff
            │       └── headless_y_compatibilidad_herramientas
            ├── ngspice_diff_y_versionado
            │       └── headless_y_compatibilidad_herramientas
            ├── xschem_diff_y_ecosistema_eda
            │       └── headless_y_compatibilidad_herramientas
            ├── estrategia_merge_archivos_mixtos
            │       ├── gds_klayout_magic_diff
            │       └── xschem_diff_y_ecosistema_eda
            ├── ci_drc_lvs_regresiones
            │       ├── gds_klayout_magic_diff
            │       ├── ngspice_diff_y_versionado
            │       └── cache_y_rendimiento
            └── cache_y_rendimiento
                    └── lenguajes_y_stack  (Rust para streaming)
```

---

## Componentes principales identificados

| Componente | Estado | Documentos relevantes |
|---|---|---|
| CLI + plugin system | Investigado | arquitectura_cli_y_orquestacion |
| Driver GDS/KLayout | Investigado | gds_klayout_magic_diff, headless |
| Driver Xschem | Investigado | xschem_diff_y_ecosistema_eda, headless |
| Driver NGSpice | Investigado | ngspice_diff_y_versionado, headless |
| Driver Magic | Investigado | gds_klayout_magic_diff, headless |
| Merge semántico | Investigado | estrategia_merge_archivos_mixtos |
| CI / DRC-LVS | Investigado | ci_drc_lvs_regresiones |
| Caché L1/L2 | Investigado | cache_y_rendimiento |
| Streaming GDS (Rust) | Pendiente | lenguajes_y_stack, cache_y_rendimiento |
| Flujos reales / UX | Investigado | ux/flujos_reales_y_variaciones |

---

## ux/

| Documento | Descripción |
|---|---|
| [flujos_reales_y_variaciones.md](ux/flujos_reales_y_variaciones.md) | Variaciones al flujo canónico observadas en proyectos reales: KLayout-primary, layout generado por Python, dos fases de simulación, SPICE como fuente primaria, flujos mixtos digital+analógico, cierre de Efabless, pain points documentados. |

---

## Gaps abiertos

- Implementación del parser SAX-style en Rust para GDS multi-GB
- Renderer web de conflictos (similar a KiRI pero para IC analógico)
- Protocolo de sincronización de caché L2 en equipos distribuidos
- Soporte para PDKs distintos de SKY130 (GF180, IHP SG13G2)
- `layout.source` en `miku.toml` para declarar fuente de verdad del layout
- `pdk.version` en `miku.toml` para reproducibilidad de DRC/LVS
- Detección de formato `.sch` por header (Xschem vs Qucs-S vs KiCad)

---

## Fuentes clave para implementación

### Herramientas EDA — repos oficiales
| Herramienta | Repo | Notas |
|---|---|---|
| KLayout | https://github.com/KLayout/klayout | Python API via `pip install klayout` |
| Xschem | https://github.com/StefanSchippers/xschem | `--no_x` requiere v3.1.0+ |
| NGSpice | https://github.com/ngspice/ngspice | Manual en sourceforge |
| Magic VLSI | https://github.com/RTimothyEdwards/magic | Batch con `-dnull -noconsole` |
| Netgen (LVS) | https://github.com/RTimothyEdwards/netgen | JSON output para CI |
| open_pdks | https://github.com/RTimothyEdwards/open_pdks | Incluye setup.tcl para Netgen+SKY130 |

### Librerías Python
| Librería | Repo | Uso en Riku |
|---|---|---|
| klayout (PyPI) | https://github.com/KLayout/klayout | XOR, diff, DRC — sin X11 |
| gdstk | https://github.com/heitzmann/gdstk | Parse/write GDS rápido |
| spicelib | https://github.com/nunobrum/spicelib | Parse netlists y `.raw` |
| pygit2 | https://github.com/libgit2/pygit2 | Acceso a objetos git |
| click / typer | https://github.com/pallets/click | CLI framework |

### Librerías Rust
| Librería | Repo | Uso en Riku |
|---|---|---|
| gds21 | https://github.com/dan-fritchman/Layout21 | Parser GDS streaming |
| Layout21 | https://github.com/dan-fritchman/Layout21 | Framework layout en Rust |
| git2-rs | https://github.com/rust-lang/git2-rs | Git operations desde Rust |

### PDKs open-source (para pruebas)
| PDK | Repo |
|---|---|
| SKY130 | https://github.com/google/skywater-pdk |
| GF180MCU | https://github.com/google/gf180mcu-pdk |
| IHP SG13G2 | https://github.com/IHP-GmbH/IHP-Open-PDK |

### Proyectos de referencia (patrones a seguir)
| Proyecto | Repo | Qué aprender |
|---|---|---|
| caravel_user_project | https://github.com/efabless/caravel_user_project | CI completo con DRC/LVS real |
| OpenLane | https://github.com/The-OpenROAD-Project/OpenLane | Flujo RTL-to-GDS automatizado |
| IIC-OSIC-TOOLS | https://github.com/iic-jku/iic-osic-tools | Docker con todas las herramientas EDA |
| KiRI | https://github.com/leoheck/kiri | UX de diff visual para esquemáticos |
| plotgitsch | https://github.com/jnavila/plotkicadsch | Integración git + diff de esquemáticos |
| DVC | https://github.com/iterative/dvc | Manejo de artefactos derivados |
| sccache | https://github.com/mozilla/sccache | Caché L1/L2 con backend S3 |
