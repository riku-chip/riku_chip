"""Volcado de un layout segun KLayout, en el formato de `verify_dump` (Riku).

    python3 klayout_dump.py cells <layout>        # top cells: bbox, poligonos/area por capa, labels
    python3 klayout_dump.py xor <a> <b> <cell>    # area anadida / eliminada por capa

Coordenadas y areas en um / um2. Requiere el modulo Python de KLayout.
"""
import sys

import klayout.db as db


def load(path):
    ly = db.Layout()
    ly.read(path)
    return ly


def cells(path):
    ly = load(path)
    dbu = ly.dbu
    idx = sorted(ly.layer_indexes(), key=lambda i: (ly.get_info(i).layer, ly.get_info(i).datatype))
    for name in sorted(ly.cell(t).name for t in ly.each_top_cell()):
        c = ly.cell(name)
        print(f"CELL {name}")
        rows, labels = [], []
        for li in idx:
            info = ly.get_info(li)
            n, area = 0, 0.0
            it = c.begin_shapes_rec(li)
            while not it.at_end():
                s, t = it.shape(), it.trans()
                if s.is_text():
                    p = s.text.transformed(t).trans.disp
                    labels.append(f"{info.layer}/{info.datatype} {s.text_string} ({p.x * dbu:.3f},{p.y * dbu:.3f})")
                elif s.polygon is not None:
                    n += 1
                    area += s.polygon.area() * dbu * dbu
                it.next()
            if n:
                rows.append(f"L {info.layer}/{info.datatype} {n} {area:.6f}")
        if rows:
            b = c.dbbox()
            print(f"BBOX {b.left:.3f} {b.bottom:.3f} {b.right:.3f} {b.top:.3f}")
        else:
            print("BBOX empty")
        for r in rows:
            print(r)
        for l in sorted(labels):
            print(f"T {l}")


def xor(a, b, name):
    la, lb = load(a), load(b)
    ca = la.cell(name) if la.has_cell(name) else None
    cb = lb.cell(name) if lb.has_cell(name) else None
    dbu = lb.dbu
    tags = sorted({(i.layer, i.datatype) for ly in (la, lb) for i in (ly.get_info(k) for k in ly.layer_indexes())})
    rows = []
    for l, d in tags:
        ra = db.Region(ca.begin_shapes_rec(la.layer(l, d))) if ca else db.Region()
        rb = db.Region(cb.begin_shapes_rec(lb.layer(l, d))) if cb else db.Region()
        add, rem = (rb - ra).area() * dbu * dbu, (ra - rb).area() * dbu * dbu
        if add or rem:
            rows.append(f"{l}/{d} +{add:.6f} -{rem:.6f}")
    for r in sorted(rows):
        print(r)


if __name__ == "__main__":
    args = sys.argv[1:]
    if len(args) == 2 and args[0] == "cells":
        cells(args[1])
    elif len(args) == 4 and args[0] == "xor":
        xor(*args[1:])
    else:
        sys.exit(__doc__)
