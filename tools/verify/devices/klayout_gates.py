#!/usr/bin/env python3
"""Transistores de una celda según el extractor de MOS de KLayout
(`DeviceExtractorMOS3Transistor`): compuerta = difusión ∩ poly, fuente y
drenaje = difusión fuera de la compuerta. Sirve de segunda opinión en las
celdas donde Riku y la netlist de referencia del PDK no coinciden.

    klayout_gates.py <gds> <celda> <difusión L/D> <poly L/D>

Imprime `GATE W=<µm> L=<µm>` por transistor, ordenados. No distingue el
tipo (N o P): eso lo dan las reglas del PDK, que acá no hacen falta.
"""
import sys

import klayout.db as db


def main():
    gds, cell_name, diff, poly = sys.argv[1:5]
    ly = db.Layout()
    ly.read(gds)
    cell = ly.cell(cell_name)
    # Aplanada, como Riku: la extracción jerárquica cuenta una sola vez los
    # transistores de las instancias repetidas.
    cell.flatten(True)
    l2n = db.LayoutToNetlist(db.RecursiveShapeIterator(ly, cell, []))
    layer = lambda s: l2n.make_polygon_layer(ly.layer(*map(int, s.split("/"))))
    rdiff, rpoly = layer(diff), layer(poly)
    gate = rdiff & rpoly
    sd = rdiff - gate
    l2n.extract_devices(db.DeviceExtractorMOS3Transistor("M"), {"SD": sd, "G": gate, "P": rpoly})
    l2n.extract_netlist()
    netlist = l2n.netlist()
    out = []
    for circuit in netlist.each_circuit():
        for d in circuit.each_device():
            out.append(f"GATE W={d.parameter('W'):.3f} L={d.parameter('L'):.3f}")
    for line in sorted(out):
        print(line)


if __name__ == "__main__":
    main()
