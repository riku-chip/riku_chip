"""Genera context_a.gds y context_b.gds: un corto que aparece recién en la
celda de arriba.

TOP tiene su propio metal1 (etiqueta B) y una instancia de PAD, cuyo metal1
tiene la etiqueta A en TOP. En la versión "b" el metal1 de PAD es más ancho
y toca el de TOP: dentro de PAD no pasa nada; en TOP, A y B quedan unidas.

    python3 gen_context.py        # requiere klayout (pip install klayout)
"""
import os

import klayout.db as db

HERE = os.path.dirname(os.path.abspath(__file__))


def build(pad_width):
    ly = db.Layout()
    ly.dbu = 0.001
    met1, li, text = ly.layer(68, 20), ly.layer(67, 20), ly.layer(68, 5)
    pad = ly.create_cell("PAD")
    pad.shapes(met1).insert(db.DBox(0, 0, pad_width, 0.5))
    pad.shapes(li).insert(db.DBox(0, 2, 0.5, 2.5))
    top = ly.create_cell("TOP")
    top.insert(db.DCellInstArray(pad.cell_index(), db.DTrans(0, 0)))
    top.shapes(met1).insert(db.DBox(1.3, 0, 2.3, 0.5))
    top.shapes(text).insert(db.DText("A", 0.5, 0.25))
    top.shapes(text).insert(db.DText("B", 1.8, 0.25))
    return ly


def main():
    build(1.0).write(os.path.join(HERE, "context_a.gds"))
    build(1.4).write(os.path.join(HERE, "context_b.gds"))


if __name__ == "__main__":
    main()
