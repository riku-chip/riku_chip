"""Genera nand2_a.gds y nand2_b.gds para los tests de transistores.

`sky130_fd_sc_hd__nand2_1` de la librería estándar de SKY130 (Apache 2.0),
sola. En la versión "b" la difusión N es 0,19 µm más baja (desde abajo,
sin salir del implante ni del poly): los dos nfet pasan de W 0,65 a 0,46 µm
y los pfet no cambian.

    python3 gen_nand2_devices.py        # requiere klayout (pip install klayout) y $PDK_ROOT
"""
import os

import klayout.db as db

HERE = os.path.dirname(os.path.abspath(__file__))
CELL = "sky130_fd_sc_hd__nand2_1"
GDS = os.path.join(os.environ.get("PDK_ROOT", "/foss/pdks"), "sky130A/libs.ref/sky130_fd_sc_hd/gds/sky130_fd_sc_hd.gds")


def load():
    ly = db.Layout()
    ly.read(GDS)
    keep = ly.cell(CELL).cell_index()
    for c in [c for c in ly.each_cell() if c.cell_index() != keep]:
        ly.delete_cell(c.cell_index())
    return ly


def main():
    load().write(os.path.join(HERE, "nand2_a.gds"))
    ly = load()
    cell = ly.cell(CELL)
    diff = ly.layer(65, 20)
    dbu = ly.dbu
    # La difusión N es la de abajo (la P está en el nwell, arriba).
    shapes = sorted(cell.shapes(diff).each(), key=lambda s: s.bbox().center().y)
    n = shapes[0]
    box = n.bbox()
    cut = int(round(0.19 / dbu))
    n.box = db.Box(box.left, box.bottom + cut, box.right, box.top)
    ly.write(os.path.join(HERE, "nand2_b.gds"))


if __name__ == "__main__":
    main()
