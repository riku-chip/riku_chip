"""Genera fixtures para el diff por instancia.

Estructura (unidad 1 µm, dbu 1 nm):
    INV  rect (0,0)-(2,1) en 1/0; en "b" suma el rect (2,0)-(3,1)
    TOP  2 SREF de INV en (10,10) y (30,10)
    ARR  AREF de INV 3x2 en (0,0), paso (10,0) y (0,5)

El cambio en INV aparece en cada instancia: TOP da 2 items y ARR 6.

Requiere el modulo Python de KLayout (en el contenedor iic-osic-tools).
"""
import os

import klayout.db as db


def make(path: str, with_extra: bool) -> None:
    ly = db.Layout()
    ly.dbu = 0.001
    l1 = ly.layer(1, 0)
    inv = ly.create_cell("INV")
    inv.shapes(l1).insert(db.DBox(0, 0, 2, 1))
    if with_extra:
        inv.shapes(l1).insert(db.DBox(2, 0, 3, 1))
    top = ly.create_cell("TOP")
    for x in (10, 30):
        top.insert(db.DCellInstArray(inv.cell_index(), db.DTrans(db.DVector(x, 10))))
    arr = ly.create_cell("ARR")
    arr.insert(db.DCellInstArray(inv.cell_index(), db.DTrans(db.DVector(0, 0)), db.DVector(10, 0), db.DVector(0, 5), 3, 2))
    ly.write(path)


if __name__ == "__main__":
    out = os.path.dirname(os.path.abspath(__file__))
    make(os.path.join(out, "multi_inst_a.gds"), False)
    make(os.path.join(out, "multi_inst_b.gds"), True)
    print("OK: multi_inst_a.gds, multi_inst_b.gds")
