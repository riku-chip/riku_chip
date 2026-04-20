# Riku — VCS semántico para diseño de chips

Riku es una herramienta de control de versiones semántico sobre Git para archivos de diseño EDA. En lugar de mostrar diffs de texto crudo en archivos binarios o de formato propietario, Riku interpreta los cambios al nivel de componentes, conexiones y nets.

## Estado actual

- Xschem (`.sch`) — **completamente implementado**: diff semántico, render SVG, anotación visual con bounding boxes y trayectos de wires.
- KLayout (`.gds`/`.oas`), Magic (`.mag`), NGSpice (`.raw`) — arquitectura lista, drivers pendientes.

## Instalación

```bash
pip install -e .
```

Requiere Python 3.11+. Para el diff visual se necesita `xschem` en el PATH.

## Uso

```bash
# Diff semántico entre dos commits
riku diff <commit_a> <commit_b> ruta/al/archivo.sch

# Salida JSON (para CI/scripts)
riku diff <commit_a> <commit_b> archivo.sch --format json

# Diff visual — abre SVG anotado con los cambios
riku diff <commit_a> <commit_b> archivo.sch --format visual

# Historial semántico
riku log archivo.sch --semantic

# Verificar herramientas EDA disponibles
riku doctor
```

## Arquitectura

```
riku/
  cli.py              — comandos Typer: diff, log, doctor
  core/
    models.py         — Component, Wire, Schematic, DiffReport
    driver.py         — RikuDriver (protocolo abstracto)
    analyzer.py       — orquestador: Git + driver + report
    registry.py       — dispatch por extensión de archivo
    semantic_diff.py  — diff semántico de schematics
    svg_annotator.py  — anotación de SVGs con bounding boxes y wires
  parsers/
    xschem.py         — parser de archivos .sch
  adapters/
    xschem_driver.py  — driver Xschem: info, diff, render
tests/
  test_xschem_parser.py
  test_analyzer.py
  test_svg_annotator.py
  bench_*.py          — benchmarks de rendimiento
planificacion/
  decisiones_tecnicas.md       — registro de decisiones de implementación
  decision_migracion_rust.md   — cuándo y qué migrar a Rust (con benchmarks)
  xschem/                      — arquitectura y roadmap
research/
  arquitectura/      — decisiones de stack, benchmarks, gotchas técnicos
  herramientas/      — investigación por herramienta EDA
  operaciones/       — caché, CI, estrategia de merge
  ux/                — flujos de usuario reales
```

## Dependencias

- `pygit2` — acceso a objetos Git (1.4ms/blob, sin fork de proceso)
- `typer` — CLI con type hints
- `xschem` (externo, en PATH) — render SVG headless

## Migración A Rust

- [Plan maestro](planificacion/plan_migracion_rust.md)
- [Checklist corto](planificacion/plan_migracion_rust_checklist.md)
- [Índice por fases](planificacion/plan_migracion_rust_indice.md)
- [Futuro del CLI](planificacion/cli_futuro.md)
