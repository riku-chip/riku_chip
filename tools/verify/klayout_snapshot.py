"""Captura de referencia de KLayout con la paleta oficial del PDK.

    python3 klayout_snapshot.py <layout> <paleta.lyp> <salida.png> [celda] [ancho] [alto]

Sirve para comparar lado a lado con una captura de riku-gui (misma celda,
toda la jerarquia visible, zoom para encuadrar). Requiere klayout.lay.
"""
import sys

import klayout.lay as lay

if __name__ == "__main__":
    if len(sys.argv) < 4:
        sys.exit(__doc__)
    layout, lyp, out = sys.argv[1:4]
    cell = sys.argv[4] if len(sys.argv) > 4 else None
    w = int(sys.argv[5]) if len(sys.argv) > 5 else 800
    h = int(sys.argv[6]) if len(sys.argv) > 6 else 600
    lv = lay.LayoutView()
    lv.load_layout(layout, True)
    lv.load_layer_props(lyp)
    if cell:
        lv.active_cellview().cell_name = cell
    lv.max_hier()
    lv.zoom_fit()
    lv.set_config("background-color", "#141418")
    lv.save_image(out, w, h)
    print(out)
