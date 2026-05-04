"""Genera fixtures GDSII para el test de regresion datatype-aware XOR.

Requiere: gdstk (pip install gdstk). Solo se corre cuando hay que regenerar
los fixtures; los .gds resultantes estan commiteados.
"""
import os

import gdstk


def make(path: str, datatype: int) -> None:
    lib = gdstk.Library("DT_FIX", unit=1e-6, precision=1e-9)
    cell = lib.new_cell("TOP")
    cell.add(gdstk.rectangle((0, 0), (10, 10), layer=1, datatype=datatype))
    lib.write_gds(path)


if __name__ == "__main__":
    out = os.path.dirname(os.path.abspath(__file__))
    make(os.path.join(out, "datatype_a.gds"), 0)
    make(os.path.join(out, "datatype_b.gds"), 1)
    print("OK: datatype_a.gds (datatype=0), datatype_b.gds (datatype=1)")
