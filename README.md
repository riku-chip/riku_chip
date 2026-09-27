<div align="center">

# Riku

**VCS semántico para diseño de chips.**
Revisa cambios en esquemáticos y layouts al nivel del circuito, no del texto.

[![CI](https://github.com/riku-chip/riku_chip/actions/workflows/ci.yml/badge.svg)](https://github.com/riku-chip/riku_chip/actions/workflows/ci.yml)
[![Status](https://img.shields.io/badge/status-alpha-yellow)](docs/roadmap.md)
[![Platform](https://img.shields.io/badge/platform-Linux%20x86__64-lightgrey)](#instalación)

[Qué hace](#qué-hace) · [Instalación](#instalación) · [Primeros pasos](#primeros-pasos) · [Documentación](#documentación)

</div>

---

## Qué hace

Un `git diff` sobre un esquemático de Xschem muestra coordenadas; sobre un GDS, nada legible. Riku lee el historial de Git y responde lo que importa:

- **Esquemáticos:** qué componentes se añadieron, eliminaron, renombraron o cambiaron de valor; qué nets se conectaron o desconectaron; si fue solo un reordenamiento visual (Move All).
- **Layouts GDS, OASIS y Magic:** qué área cambió, en qué capa y en qué celda; si viene de una sub-celda instanciada; qué celdas cambiaron en una librería; si es ruido por debajo de la grilla. En Magic, con las capas por nombre (`metal1`), las sub-celdas leídas del mismo commit y los puertos (`A: input → inout`).
- **Simulaciones (ngspice `.raw`):** qué señales cambiaron, cuánto (error máximo, dónde, RMS) y si la diferencia es solo ruido numérico.

Y lo muestra en un **visor** con las versiones antes/después y los cambios resaltados. Todo en Rust: no hace falta tener xschem, KLayout ni Magic instalados.

| | |
|---|---|
| **Diff semántico y geométrico** | Texto para leer, JSON versionado (`riku-diff/v2`) para scripts; cambios funcionales separados de los cosméticos |
| **Historial** | `riku log`, `riku show` y `riku status` con resumen semántico por archivo; `--ci` para usar en GitHub Actions |
| **Visor** | Esquemáticos y layouts por la misma ruta: diff visual, capas, tooltip, selector de celdas, paletas de SKY130/GF180/IHP; layouts de millones de polígonos fluidos |
| **PDK automático** | Encuentra los símbolos por `.xschemrc`, `$PDK_ROOT`/`$PDK`, o detectando qué PDK instalado usa el esquemático |
| **Verificado** | La lectura de layouts y el XOR dan lo mismo que KLayout en las librerías estándar de los tres PDKs |
| **Modular** | Un núcleo que no conoce formatos y un módulo por formato; sumar uno (Magic se sumó así; KiCad…) no toca la CLI ni el visor |

| Formato | Extensión | Diff | Visor |
|---|---|:-:|:-:|
| Xschem | `.sch`, `.sym` | semántico | ✓ |
| GDSII | `.gds` | geométrico (XOR) | ✓ |
| OASIS | `.oas` | geométrico (XOR) | ✓ |
| ngspice | `.raw` | formas de onda (con tolerancia) | ✓ (curvas) |
| Magic | `.mag` | geométrico (XOR) y puertos, con la jerarquía del mismo commit | ✓ |

## Instalación

Un solo ejecutable, `riku`, con la CLI, el shell y el visor. Linux x86_64 con glibc 2.35 o más nueva (Ubuntu 22.04+, Debian 12+, Fedora 36+, iic-osic-tools); para el visor, X11 o Wayland.

Desde [Releases](https://github.com/riku-chip/riku_chip/releases):

```bash
tar xf riku-<versión>-linux-x86_64.tar.gz && ./riku-<versión>-linux-x86_64/install.sh   # o --system
sudo apt install ./riku_<versión>-1_amd64.deb                                           # o el .deb
```

Desde el código (ver [`docs/desarrollo.md`](docs/desarrollo.md)):

```bash
git clone --recurse-submodules https://github.com/riku-chip/riku_chip
cd riku_chip && cargo build --release          # target/release/riku
```

## Primeros pasos

```bash
cd tu-proyecto
riku doctor                                    # verifica el entorno y el PDK
riku status                                    # qué cambió en el working tree
riku log                                       # historial con resumen semántico
riku show HEAD                                 # qué cambió en el último commit
riku diff HEAD~1 HEAD design/op_amp.sch        # diff de un archivo
riku diff HEAD~1 HEAD chip.gds -f visual       # el mismo diff, en el visor
riku gui sky130_fd_sc_hd.gds --cell sky130_fd_sc_hd__inv_1
riku                                           # shell interactivo (Tab completa)
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
| [`docs/cli.md`](docs/cli.md) | Comandos, JSON y códigos de salida |
| [`docs/gui.md`](docs/gui.md) | El visor |
| [`docs/xschem.md`](docs/xschem.md) · [`docs/layouts.md`](docs/layouts.md) · [`docs/spice.md`](docs/spice.md) | Esquemáticos y PDK · layouts GDS/OASIS/Magic · simulaciones de ngspice |
| [`docs/arquitectura.md`](docs/arquitectura.md) · [`docs/desarrollo.md`](docs/desarrollo.md) | Cómo está hecho · cómo compilar, probar y publicar |
| [`docs/roadmap.md`](docs/roadmap.md) | Estado, fases y pendientes |

## Estado

**Alpha.** Esquemáticos Xschem, layouts GDS/OASIS/Magic y simulaciones de ngspice funcionan de punta a punta: diff en la CLI, historial (`log --graph`) y visor. Los layouts grandes (millones de polígonos) se comparan en segundos y con menos de 1 GB. `log`, `show` y `status` usan todos los núcleos (`--jobs N` para limitarlos). Ver [`docs/roadmap.md`](docs/roadmap.md).

## Contribuir

Antes de abrir un PR: `cargo test --workspace` en verde (la CI lo corre con `-D warnings`) y commits con formato convencional (`feat:`, `fix:`, `perf:`…). Los cambios grandes conviene conversarlos antes en un issue.

## Licencia

[Apache-2.0](LICENSE), la misma que [`xschem-viewer-rust`](https://github.com/carloscl03/xschem-viewer-rust), el motor de Xschem. El motor de layouts, [`gdstk_rust`](https://github.com/Adriel2503/gdstk_rust), mantiene la licencia de gdstk (Boost 1.0), compatible con esta.
