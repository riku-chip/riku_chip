<div align="center">

# Riku

**VCS semántico para diseño de chips.**
Revisa cambios en esquemáticos, layouts y simulaciones al nivel del circuito, no del texto.

[![CI](https://github.com/riku-chip/riku_chip/actions/workflows/ci.yml/badge.svg)](https://github.com/riku-chip/riku_chip/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/riku-chip/riku_chip)](https://github.com/riku-chip/riku_chip/releases)
[![Platform](https://img.shields.io/badge/platform-Linux%20x86__64-lightgrey)](#instalación)

</div>

Un `git diff` sobre un esquemático de Xschem muestra coordenadas; sobre un GDS, `Binary files differ`. Riku lee las versiones del historial de Git y dice lo que importa:

- **Esquemáticos (Xschem):** componentes añadidos, eliminados, renombrados o con otro valor; nets conectadas o desconectadas; si fue solo un reordenamiento visual.
- **Layouts (GDS, OASIS, Magic):** qué área cambió, en qué capa y celda, y si viene de una sub-celda instanciada. En Magic, con capas por nombre, sub-celdas del mismo commit y puertos.
- **Simulaciones (ngspice `.raw`):** qué señales cambiaron y cuánto, separando el ruido numérico.

Todo en la terminal (texto o JSON para scripts y CI) y en un **visor** de escritorio con las versiones antes/después. No hace falta tener Xschem, KLayout ni Magic instalados.

| Formato | Extensión | Diff | Visor |
|---|---|---|:-:|
| Xschem | `.sch` (`.sym` solo en el visor) | semántico | ✓ |
| GDSII / OASIS | `.gds`, `.oas` | geométrico (XOR) | ✓ |
| Magic | `.mag` | geométrico y puertos, con la jerarquía del mismo commit | ✓ |
| ngspice | `.raw` | formas de onda, con tolerancia | ✓ (curvas) |

## Instalación

Un solo ejecutable, `riku` (CLI, shell y visor), para Linux x86_64 con glibc 2.35+ (Ubuntu 22.04+, Debian 12+, Fedora 36+, iic-osic-tools):

```bash
curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sh   # en ~/.local/bin
```

`sh -s -- v0.2.1` instala una versión concreta y `sudo sh -s -- latest --system` la deja en `/usr/local/bin`. También están el `.tar.gz` y el `.deb` en [Releases](https://github.com/riku-chip/riku_chip/releases). Para compilar desde el código, ver [`docs/desarrollo.md`](docs/desarrollo.md).

## Primeros pasos

Riku trabaja sobre un repositorio Git (se crea y se commitea con `git`; Riku solo lee):

```bash
cd mi_proyecto
riku doctor                            # entorno y PDK
riku status                            # qué cambió en el disco respecto al último commit
riku diff amp.sch                      # el detalle de un archivo
riku log                               # historial con resumen por archivo
riku diff HEAD~1 HEAD chip.gds -f visual
riku open                              # el visor, en la pantalla de inicio
riku                                   # shell interactivo (Tab completa)
```

```text
Archivo : design/op_amp.sch
Cambios : 3

  + M5
      symbol: sky130_fd_pr/nfet_01v8_lvt.sym
  - R2
  ~ C1
      value: 1p → 2p
```

## Documentación

| | |
|---|---|
| [`docs/cli.md`](docs/cli.md) | Comandos, JSON, códigos de salida, `.riku.toml` |
| [`docs/gui.md`](docs/gui.md) | El visor |
| [`docs/formatos.md`](docs/formatos.md) | Qué compara cada formato: Xschem, layouts (GDS/OASIS/Magic) y simulaciones |
| [`docs/desarrollo.md`](docs/desarrollo.md) | Compilar, probar, arquitectura, reglas del proyecto y publicar |
| [`docs/pendientes.md`](docs/pendientes.md) | Lo que falta: pendientes, ideas y limitaciones |

**Estado:** alpha. Los tres tipos de archivo funcionan de punta a punta; los layouts de millones de polígonos se comparan en segundos y con menos de 1 GB. Para contribuir: `cargo test --workspace` en verde y commits `tipo(alcance): …`.

## Licencia

[Apache-2.0](LICENSE), la misma que [`xschem-viewer-rust`](https://github.com/carloscl03/xschem-viewer-rust), el motor de Xschem. El motor de layouts, [`gdstk_rust`](https://github.com/Adriel2503/gdstk_rust), mantiene la de gdstk (Boost 1.0), compatible con esta.
