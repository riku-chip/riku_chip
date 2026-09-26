"""Convierte fixtures GDSII a OASIS para los tests de .oas.

    hier_inv_a.gds -> hier_inv_a.oas
    hier_inv_b.gds -> hier_inv_b.oas

Misma geometria: el diff de los .oas debe dar lo mismo que el de los .gds.

Requiere el modulo Python de KLayout (en el contenedor iic-osic-tools).
"""
import os

import klayout.db as db

if __name__ == "__main__":
    out = os.path.dirname(os.path.abspath(__file__))
    for name in ("hier_inv_a", "hier_inv_b"):
        ly = db.Layout()
        ly.read(os.path.join(out, name + ".gds"))
        ly.write(os.path.join(out, name + ".oas"))
    print("OK: hier_inv_a.oas, hier_inv_b.oas")
