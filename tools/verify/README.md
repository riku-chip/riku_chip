# Verificación contra KLayout

Scripts para comprobar que Riku lee los layouts igual que KLayout y para probar el visor (`riku gui`) sin mouse. No corren en la CI porque necesitan KLayout, los PDKs y un servidor X. Se corren a mano en el contenedor **iic-osic-tools**, antes de tocar el render, las etiquetas o el diff.

```bash
docker exec -it <contenedor-iic-osic-tools> bash
cd /foss/designs/riku_chip
tools/verify/compare.sh
```

## Geometría, etiquetas y XOR

| Script | Qué hace |
|---|---|
| `compare.sh` | Compila `verify_dump` (ejemplo de `riku-mod-layout`), vuelca cada librería con Riku y con KLayout y compara los dos textos. Sale con código 1 si hay diferencias |
| `klayout_dump.py` | Lado KLayout del volcado (`cells` y `xor`) |
| `riku-mod-layout/examples/verify_dump.rs` | Lado Riku, mismo formato |

```bash
tools/verify/compare.sh                          # SKY130, GF180 e IHP (librerías de celdas estándar)
tools/verify/compare.sh mi_chip.gds otro.oas     # librerías propias (GDSII u OASIS)
tools/verify/compare.sh --xor a.gds b.gds CELDA  # área añadida/eliminada por capa
```

El volcado de `cells` tiene, por cada top cell:
- `BBOX`, o `BBOX empty` si la celda no tiene geometría;
- por capa, el número de polígonos y el área (µm², sin fusionar solapes);
- cada etiqueta de toda la jerarquía con su posición en la celda raíz.

Los archivos quedan en `$OUT` (por defecto `/tmp/riku-verify`) para revisar diferencias con `diff`.

Variables: `PDK_ROOT` (por defecto `/foss/pdks`), `OUT` y `CARGO_TARGET_DIR`.

Resultados de referencia (2026-09): idéntico a KLayout en todo.

| Qué | Dónde |
|---|---|
| Geometría por celda: bbox, polígonos y área por capa, labels con posición | 437 celdas de `sky130_fd_sc_hd`, 230 de `gf180mcu_fd_sc_mcu7t5v0`, 78 de `sg13g2_stdcell` (~30 s) |
| Lo mismo leyendo OASIS | `hier_inv_b.oas` |
| XOR por capa | `inv_1` con met1 añadido, mcon borrado, poly movido 0,05 µm, licon movido 5 nm |
| XOR jerárquico | met1 dentro de `inv_2` → `macro_sparecell`; AREF 3×2 |
| Layout de 42 MB | reexportado sin cambios, y con cambios en las capas 6/0 y 19/0 |

## Magic (`mag/`)

`mag/compare_mag.sh` compara jerarquías `.mag` leídas por `gdstk-rs` (ejemplo `mag_area`) y por KLayout (`mag/klayout_mag_area.py`): aplanadas y sin unir, la **misma cantidad de polígonos** y la **misma suma de áreas** por capa (exacto y rápido aun en jerarquías de cientos de celdas).

```bash
tools/verify/mag/compare_mag.sh                         # 8 jerarquías de SKY130 y GF180
tools/verify/mag/compare_mag.sh top.mag 0.01 DIR...     # una propia: lambda en µm y dónde buscar sub-celdas
```

`mag/mag_bench.sh` mide el diff de una jerarquía real (la librería `sky130_fd_io` en un repo en `/tmp`, con una sub-celda editada y otra re-escrita en tiras): tiempo, memoria y cantidad de cambios.

Hace falta **KLayout 0.30.12 o más nuevo** (0.30.4 y anteriores ignoran `magscale`): `python3 -m venv /tmp/kl && /tmp/kl/bin/pip install klayout==0.30.12` y `KLAYOUT_PY=/tmp/kl/bin/python`. Las capas que Riku deja fuera a propósito (`checkpaint`, `error_*`…) no cuentan como diferencia.

## Medir (`riku-mod-layout/examples/`)

- `profile_diff a.gds b.gds`: tiempo y memoria de cada etapa del diff (`SKIP_FP=1`, `PRINTS=1`, `CANON=1` para diagnósticos).
- `profile_view layout.gds`: cuánto tarda el visor en armar la escena y el índice.
- `profile_prints layout.gds [celda] [hilos]`: la huella por pedazos (reparto, tiempos, memoria, escalado por hilos).
- `profile_xor a.gds b.gds <celda> <layer> <datatype>`: el XOR de una capa, entero y por cuadrantes (`SKIP_WHOLE=1` si el entero tarda minutos).
- `verify_dump`: el volcado que usa `compare.sh`; `mag_area` (en `external/gdstk/rust/examples`), el de `compare_mag.sh`.

Correrlos en release y sobre una copia en `/tmp`: un montaje lento distorsiona los tiempos.

## Comparación visual

```bash
python3 tools/verify/klayout_snapshot.py \
  /foss/pdks/sky130A/libs.ref/sky130_fd_sc_hd/gds/sky130_fd_sc_hd.gds \
  /foss/pdks/sky130A/libs.tech/klayout/tech/sky130A.lyp \
  /tmp/klayout_inv1.png sky130_fd_sc_hd__inv_1 800 600
```

Genera la captura de KLayout con la paleta oficial, para ponerla al lado de una del visor abierto en la misma celda (`riku gui <archivo> --cell <celda>`).

## Pruebas de la GUI sin mouse (`gui/`)

`xt.py` usa XTest para simular clics, arrastres, rueda y teclado sobre la ventana del visor, y captura la ventana con `xwd`:

```bash
env -u WAYLAND_DISPLAY cargo run --release -- gui archivo.gds &   # XWayland para poder capturar
W=$(xwininfo -root -tree | grep 'riku-gui")' | awk '{print $1}')
python3 tools/verify/gui/xt.py $W raise
python3 tools/verify/gui/xt.py $W click 120 80
python3 tools/verify/gui/xt.py $W drag 600 400 500 350 300     # arrastre de 300 ms
python3 tools/verify/gui/xt.py $W keytap f                     # atajo "encuadrar"
python3 tools/verify/gui/xt.py $W shot /tmp/riku.png
```

`gui/xwd2png.py` convierte una captura `xwd` suelta a PNG. Para matar la GUI usar `pkill -f "riku gui"` (cuidado: `-f` también mata la shell que lo lanzó si su línea contiene ese texto).
