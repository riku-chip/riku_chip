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

Resultado de referencia (2026-09-26): idéntico en `sky130_fd_sc_hd` (437 celdas), `gf180mcu_fd_sc_mcu7t5v0` (230) y `sg13g2_stdcell` (78), en ~30 s.

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
