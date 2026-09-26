"""Fixtures para tests programaticos de select_top_cell.

- top_single.gds: una unica top cell "ALPHA".
- top_multi.gds: tres top cells "ZETA"/"ALPHA"/"BETA"; la politica
  alfabetica debe elegir "ALPHA".
- top_nested.gds: TOP -> SREF -> INV -> SREF -> GATE; un solo top "TOP".

Cyclic libraries no se generan: gdstk valida la jerarquia al escribir
y rechaza ciclos. El caso `count == 0 -> None` queda fuera.
"""
import os

import gdstk


def make_single(path: str) -> None:
    lib = gdstk.Library("SINGLE", unit=1e-6, precision=1e-9)
    cell = lib.new_cell("ALPHA")
    cell.add(gdstk.rectangle((0, 0), (1, 1), layer=1))
    lib.write_gds(path)


def make_multi(path: str) -> None:
    lib = gdstk.Library("MULTI", unit=1e-6, precision=1e-9)
    for name in ("ZETA", "ALPHA", "BETA"):
        c = lib.new_cell(name)
        c.add(gdstk.rectangle((0, 0), (1, 1), layer=1))
    lib.write_gds(path)


def make_nested(path: str) -> None:
    lib = gdstk.Library("NESTED", unit=1e-6, precision=1e-9)
    gate = lib.new_cell("GATE")
    gate.add(gdstk.rectangle((0, 0), (1, 1), layer=1))
    inv = lib.new_cell("INV")
    inv.add(gdstk.Reference(gate, (5, 5)))
    top = lib.new_cell("TOP")
    top.add(gdstk.Reference(inv, (10, 10)))
    lib.write_gds(path)


if __name__ == "__main__":
    out = os.path.dirname(os.path.abspath(__file__))
    make_single(os.path.join(out, "top_single.gds"))
    make_multi(os.path.join(out, "top_multi.gds"))
    make_nested(os.path.join(out, "top_nested.gds"))
    print("OK: top_single, top_multi, top_nested")
