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
- **Layouts (GDS, OASIS, Magic):** qué área cambió, en qué capa y celda, y si viene de una sub-celda instanciada. En Magic, con capas por nombre, sub-celdas del mismo commit y puertos. En SKY130, GF180MCU e IHP, además, qué transistores cambiaron de modelo, W o L, y qué redes se **abrieron o se cortaron**.
- **Simulaciones (ngspice `.raw`):** qué señales cambiaron y cuánto, separando el ruido numérico.

Todo en la terminal (texto o JSON para scripts y CI) y en un **visor** de escritorio con las versiones antes/después. No hace falta tener Xschem, KLayout ni Magic instalados.

| Formato | Extensión | Diff | Visor |
|---|---|---|:-:|
| Xschem | `.sch` (`.sym` solo en el visor) | semántico | ✓ |
| GDSII / OASIS | `.gds`, `.oas` | geométrico (XOR); transistores y redes con el PDK | ✓ |
| Magic | `.mag` | geométrico, puertos, transistores y redes, con la jerarquía del mismo commit | ✓ |
| ngspice | `.raw` | formas de onda, con tolerancia | ✓ (curvas) |

## Instalación

Un solo ejecutable, `riku` (CLI, shell y visor), para Linux x86_64 con glibc 2.35+ (Ubuntu 22.04+, Debian 12+, Fedora 36+, iic-osic-tools):

```bash
curl -fsSL https://raw.githubusercontent.com/riku-chip/riku_chip/main/packaging/get.sh | sh   # en ~/.local/bin
```

`sh -s -- v0.1.0` instala una versión concreta y `sudo sh -s -- latest --system` la deja en `/usr/local/bin`. También están el `.tar.gz` y el `.deb` en [Releases](https://github.com/riku-chip/riku_chip/releases). Para compilar desde el código, ver [`docs/desarrollo.md`](docs/desarrollo.md).

## Probar en un minuto

`riku demo` crea proyectos de ejemplo con historia real (repos Git) en `~/riku-demos`, para ver Riku sin un diseño propio:

```bash
riku demo
cd ~/riku-demos/ota
riku log --graph
```

| Demo | Qué tiene |
|---|---|
| `ota` | Un amplificador OTA de SKY130 con esquemático, testbench, layout y simulación: 11 commits, una rama con merge, un transistor más ancho y un corto en el layout que después se arregla |
| `sram` | Una SRAM 16×8 de OpenRAM (SKY130): un cambio en la celda de bit que aparece en sus 153 instancias, una celda renombrada y relleno de metal en una rama |

Cada uno trae un `README.md` con qué probar. Los ejemplos de abajo salen de `ota`, con `RIKU_LANG=es` (la salida está en inglés por defecto).

## Comandos

Riku trabaja sobre un repositorio Git: se crea y se commitea con `git`, y Riku solo lee. Todos los comandos funcionan también dentro del shell interactivo (`riku`, con Tab para completar).

### `riku log`: el historial, con qué cambió en cada commit

```text
$ riku log --graph
● 120ee0b  Layout: route Vout to the left edge
│   layout/ota-5t.gds  1 corto, 1 componente añadido
○   ad104a7 [merge]  Merge branch 'narrow-input-pair'
├─╮
● │ 6568371  Tidy up the schematic (move everything)
│ │   xschem/ota-5t.sch  (solo cambios cosméticos)
│ ● 0f05efe  Narrower input pair: M3, M4 W 20u -> 18u
│ │   sim/ota-5t_tb.raw  71 señales cambiaron
│ │   xschem/ota-5t.sch  2 componentes modificados
├─╯
```

Los cortos y abiertos van primero y en rojo. `riku log amp.sch` muestra solo los commits que tocan ese archivo.

### `riku show`: el detalle de un commit

Un parámetro que cambió en el esquemático:

```text
$ riku show HEAD~7
Archivo  : xschem/ota-5t.sch
  ~ M1
      W: 2 → 4
  ~ M2
      W: 2 → 4
```

En el layout, los transistores que cambiaron de tamaño (se reconocen con las reglas del PDK):

```text
$ riku show narrow-input-pair
Archivo  : layout/ota-5t.gds
  - ota-5t:L65/20
      -1 polys / -3.050 µm²
      bbox: (-0.400, 5.600) → (5.700, 6.100) µm
  ~ ota-5t:sky130_fd_pr__nfet_01v8 @ (0.150, 3.367)
      w_um: 5.000 → 4.500
```

Y lo que un diff de área no ve: un metal nuevo que une dos redes.

```text
$ riku show HEAD~3
Archivo  : layout/ota-5t.gds
  ! ota-5t:net:Vout = Vp
      corto (redes unidas): Vout, Vp → Vout = Vp
      bbox: (-5.600, -0.560) → (-0.400, 9.550) µm
  + ota-5t:L70/20
      +1 polys / +6.716 µm²
```

El commit siguiente lo arregla: `redes separadas (corto resuelto): Vout = Vp → Vout, Vp`.

### `riku diff`: entre dos versiones cualesquiera

Como `git diff`: sin argumentos compara el disco contra el último commit; con dos versiones (commits, ramas o tags), entre ellas; con un archivo, solo ese.

```bash
riku diff                                   # el disco contra HEAD, todos los archivos
riku diff v0.1 v1.0                         # todo lo que cambió entre dos tags
riku diff v0.1 v1.0 layout/ota-5t.gds -f visual   # en el visor
riku diff v0.1 v1.0 -f json                 # para scripts y CI
```

En una simulación, cuánto cambió cada señal, o una medida calculada con la sintaxis de ngspice:

```text
$ riku diff v0.1 v1.0 sim/ota-5t_tb.raw --expr "ac: a0 = max(db(v(vout)))"
  ~ a0
      = max(db(v(vout)))
      38.292 dB → 38.881 dB · Δ 0.589 dB (1.51 %)  (AC Analysis)
```

En JSON, cada cambio es tipado, y los abiertos y cortos llevan `"severity": "error"`:

```json
{
  "kind": "modified",
  "element": { "type": "layout_net", "cell": "ota-5t", "name": "Vout = Vp" },
  "details": [
    { "key": "kind", "after": "short" },
    { "key": "nets", "before": "Vout, Vp", "after": "Vout = Vp" }
  ],
  "location": { "min_x": -5.6, "min_y": -0.56, "max_x": -0.4, "max_y": 9.55 },
  "severity": "error"
}
```

### `riku status`: qué cambió en el disco

```text
$ riku status
En rama main (HEAD 421f72d)

Modificados con cambios semánticos:
  xschem/ota-5t.sch    1 componente modificado
```

`--ci` termina con error si hay cambios funcionales; `-f json` da la misma información para scripts.

### `riku open`: el visor

`riku open` abre el visor del proyecto, y `riku open layout/ota-5t.gds` un archivo. En un diff muestra las dos versiones superpuestas, con los cambios listados y un clic para encuadrar cada uno. En un layout también muestra:
- la capa de transistores;
- la red del polígono bajo el cursor;
- un clic para resaltar una red entera.

**H** abre el historial. Todos los controles: [`docs/gui.md`](docs/gui.md).

### Otros

| Comando | Qué hace |
|---|---|
| `riku render archivo -o imagen.png` | Una imagen (PNG o SVG) de un archivo o de una versión (`--rev`), sin ventana |
| `riku doctor` | Revisa el entorno: el repo, el PDK y qué formatos se pueden comparar |
| `riku demo [--list]` | Los proyectos de ejemplo |
| `riku completions bash` | Autocompletado para bash, zsh, fish, powershell o elvish |

Todas las opciones, el JSON y los códigos de salida: [`docs/cli.md`](docs/cli.md).

## Documentación

| | |
|---|---|
| [`docs/cli.md`](docs/cli.md) | Comandos, JSON, códigos de salida, `.riku.toml` |
| [`docs/gui.md`](docs/gui.md) | El visor |
| [`docs/formatos.md`](docs/formatos.md) | Qué compara cada formato: Xschem, layouts (GDS/OASIS/Magic, con transistores y redes) y simulaciones |
| [`docs/desarrollo.md`](docs/desarrollo.md) | Compilar, probar, verificar (KLayout, Magic, Netgen), arquitectura, reglas y publicar |
| [`docs/arquitectura.html`](docs/arquitectura.html) | Diagrama interactivo de la arquitectura: CLI y visor, núcleo, módulos por formato, motor de layouts y PDK (descargarlo y abrirlo en el navegador) |
| [`docs/pendientes.md`](docs/pendientes.md) | Lo que falta: pendientes, ideas (LVS, chequeos eléctricos) y limitaciones |

**Estado:** alpha. Los tres tipos de archivo funcionan de punta a punta; los layouts de millones de polígonos se comparan en segundos y con menos de 1 GB. Para contribuir: `cargo test --workspace` en verde y commits `tipo(alcance): …`.

## Licencia

[Apache-2.0](LICENSE), la misma que [`xschem-viewer-rust`](https://github.com/carloscl03/xschem-viewer-rust), el motor de Xschem. El motor de layouts, [`gdstk_rust`](https://github.com/Adriel2503/gdstk_rust), mantiene la de gdstk (Boost 1.0), compatible con esta.
