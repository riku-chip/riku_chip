"""Genera fixtures para detectar celdas renombradas.

    a: INV (rects 1/0 y 2/0), TOP -> INV @ (10,10), EMPTY_A (vacia),
       OLD (rect 1/0 (0,0)-(1,1))
    b: INV_X1 (misma geometria que INV), TOP -> INV_X1 @ (10,10),
       EMPTY_B (vacia), NEW (rect 1/0 movido a (0.5,0)-(1.5,1))

Esperado: INV -> INV_X1 es renombre; TOP no cambia; las vacias y OLD/NEW
siguen como eliminada + anadida.

Requiere el modulo Python de KLayout (en el contenedor iic-osic-tools).
"""
import os

import klayout.db as db


def make(path: str, after: bool) -> None:
    ly = db.Layout()
    ly.dbu = 0.001
    l1, l2 = ly.layer(1, 0), ly.layer(2, 0)
    inv = ly.create_cell("INV_X1" if after else "INV")
    inv.shapes(l1).insert(db.DBox(0, 0, 2, 1))
    inv.shapes(l2).insert(db.DBox(0.5, 0.2, 1.5, 0.8))
    top = ly.create_cell("TOP")
    top.insert(db.DCellInstArray(inv.cell_index(), db.DTrans(db.DVector(10, 10))))
    ly.create_cell("EMPTY_B" if after else "EMPTY_A")
    other = ly.create_cell("NEW" if after else "OLD")
    x = 0.5 if after else 0.0
    other.shapes(l1).insert(db.DBox(x, 0, x + 1, 1))
    ly.write(path)


if __name__ == "__main__":
    out = os.path.dirname(os.path.abspath(__file__))
    make(os.path.join(out, "rename_a.gds"), False)
    make(os.path.join(out, "rename_b.gds"), True)
    print("OK: rename_a.gds, rename_b.gds")
