"""Genera fixtures GDSII para el test de regresion XOR jerarquico.

Estructura:
    TOP
    └── SREF -> INV @ (10, 10)
        INV
        └── rect (0,0)-(2,1) en (layer=1, datatype=0)

En la version "b", INV tiene un rect adicional (2,0)-(3,1). El XOR
de TOP_a vs TOP_b sin atravesar references reportaria 0 cambios; con
flatten el cambio aparece en coords absolutas (12, 10)-(13, 11).

Requiere: gdstk (pip install gdstk).
"""
import os

import gdstk


def make(path: str, with_extra: bool) -> None:
    lib = gdstk.Library("HIER", unit=1e-6, precision=1e-9)
    inv = lib.new_cell("INV")
    inv.add(gdstk.rectangle((0, 0), (2, 1), layer=1, datatype=0))
    if with_extra:
        inv.add(gdstk.rectangle((2, 0), (3, 1), layer=1, datatype=0))
    top = lib.new_cell("TOP")
    top.add(gdstk.Reference(inv, (10, 10)))
    lib.write_gds(path)


if __name__ == "__main__":
    out = os.path.dirname(os.path.abspath(__file__))
    make(os.path.join(out, "hier_inv_a.gds"), False)
    make(os.path.join(out, "hier_inv_b.gds"), True)
    print("OK: hier_inv_a.gds (INV con 1 rect), hier_inv_b.gds (INV con 2 rects)")
