"""Polígonos y área por capa de una jerarquía Magic según KLayout, aplanada.

    python3 klayout_mag_area.py top.mag LAMBDA_UM [DIR_DE_LIBRERIA ...]

Salida: `capa<TAB>polígonos<TAB>área_um2`, ordenada por nombre de capa, igual
que el ejemplo `mag_area` de gdstk-rs, para compararlas con `compare_mag.sh`.
Sin unir polígonos (el área es la suma, con superposiciones): así la
comparación es exacta, polígono por polígono, y rápida en layouts grandes.
"""
import sys

import klayout.db as db


def main():
    top, lam, libs = sys.argv[1], float(sys.argv[2]), sys.argv[3:]
    opt = db.LoadLayoutOptions()
    opt.mag_lambda = lam
    opt.mag_dbu = 0.0005
    opt.mag_library_paths = libs
    opt.mag_keep_layer_names = True
    opt.mag_merge = False
    ly = db.Layout()
    ly.read(top, opt)
    cell = ly.top_cell()
    rows = []
    for li in ly.layer_indexes():
        info = ly.get_info(li)
        name = info.name or "%d/%d" % (info.layer, info.datatype)
        region = db.Region(cell.begin_shapes_rec(li))
        region.merged_semantics = False
        count = region.count()
        area = region.area() * ly.dbu ** 2
        if count > 0:
            rows.append((name, count, area))
    for name, count, area in sorted(rows):
        print("%s\t%d\t%.6f" % (name, count, area))


main()
