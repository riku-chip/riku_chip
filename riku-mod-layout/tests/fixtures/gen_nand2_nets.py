"""Genera nand2_short.gds y nand2_open.gds para los tests de redes.

A partir de nand2_a.gds (`sky130_fd_sc_hd__nand2_1`, ver gen_nand2_devices.py):

- **short:** un `li1` de 0,22 µm entre la entrada B y la salida Y.
- **open:** sin las tres vías `mcon` del riel de tierra: el `li1` de abajo
  (y la fuente del nfet) queda separado del `met1` de VGND.

    python3 gen_nand2_nets.py        # requiere klayout (pip install klayout)
"""
import os

import klayout.db as db

HERE = os.path.dirname(os.path.abspath(__file__))


def load():
    ly = db.Layout()
    ly.read(os.path.join(HERE, "nand2_a.gds"))
    return ly, ly.top_cell()


def main():
    ly, cell = load()
    li = ly.layer(67, 20)
    cell.shapes(li).insert(db.DBox(0.40, 1.10, 0.62, 1.28))
    ly.write(os.path.join(HERE, "nand2_short.gds"))

    ly, cell = load()
    mcon = ly.layer(67, 44)
    rail = [s for s in cell.shapes(mcon).each() if s.bbox().to_dtype(ly.dbu).top < 0.1]
    assert len(rail) == 3, rail
    for s in rail:
        cell.shapes(mcon).erase(s)
    ly.write(os.path.join(HERE, "nand2_open.gds"))


if __name__ == "__main__":
    main()
