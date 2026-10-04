"""Demo `chip`: una SRAM de 1 KB de OpenRAM para SKY130 (9,9 MB de GDS, 161
celdas, 8 192 bitcells) con 6 commits, una rama y un merge. Es para ver el
rendimiento de Riku con un layout grande. El bundle pesa ~1,4 MB (el GDS se
comprime bien y los cambios son chicos), así que va embebido como los demás.

Fuente: `sky130_sram_1kbyte_1rw1r_32x256_8` de
[sky130_sram_macros](https://github.com/VLSIDA/sky130_sram_macros)
(Apache-2.0), tal como viene en open_pdks.

    python3 tools/demos/chip.py <salida.bundle>
"""
import os
import sys

import klayout.db as db

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import Repo  # noqa: E402

PDK_ROOT = os.environ.get("PDK_ROOT", "/foss/pdks")
NAME = "sky130_sram_1kbyte_1rw1r_32x256_8"
SRC = f"{PDK_ROOT}/sky130A/libs.ref/sky130_sram_macros/gds/{NAME}.gds"
LICENSE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "ota", "LICENSE")
GDS = f"{NAME}.gds"
BITCELL = "sky130_fd_bd_sram__openram_dp_cell"
CONTROL = f"{NAME}_control_logic_r"

README = f"""# 1 KB OpenRAM SRAM (SKY130) · Riku demo

A real macro: the `{NAME}` SRAM of
[sky130_sram_macros](https://github.com/VLSIDA/sky130_sram_macros) (9.9 MB of
GDS, 161 cells, 8 192 bitcells), in 6 commits with a branch and a merge. It is
here to see how Riku does with a large layout.

Things to try:

```bash
riku log --graph                 # the history
riku show HEAD~3                 # one metal1 shape of the bitcell, 10 nm wider: seen in all 8 192 instances
riku show HEAD~2                 # metal5 straps on the top cell
riku show HEAD~1                 # a renamed cell (same geometry)
riku show pin-fix                # the din0 pins, a bit wider
riku diff v1.0 HEAD              # everything
riku gui {GDS}                   # the viewer; H opens the history
```

Times measured on a 12-core machine with the release build (in `/tmp`):

| Command | First time | Again (diff cache) |
|---|---|---|
| `riku show HEAD~3` (the bitcell, 8 192 instances) | 9.4 s | 0.2 s |
| `riku show` of the other commits | 0.5 s | 0.15 s |
| `riku diff v1.0 HEAD` | 7.1 s | 0.13 s |
| `riku log -n 10` | 7.4 s | 0.25 s |

The first `riku show HEAD~3` spends ~2 s on the XOR of the bitcell change and the
rest on the nets of the sub-cells; the top cells are too large to compare nets
(more than 2 million polygons), and Riku says so.

Source: the `{NAME}` macro of sky130_sram_macros (Apache-2.0, see `LICENSE`),
as shipped in open_pdks. The changes are Riku's, to show its diffs.
"""


def write(ly, path):
    """Sin fechas en el GDS: el mismo contenido da el mismo archivo (y el
    repo es reproducible)."""
    opt = db.SaveLayoutOptions()
    opt.format = "GDS2"
    opt.gds2_write_timestamps = False
    ly.write(path, opt)


def edit(repo, fn):
    ly = db.Layout()
    ly.read(repo.file(GDS))
    fn(ly)
    write(ly, repo.file(GDS))


def wider_bitcell_met1(ly):
    """Un rect de met1 del bitcell, 10 nm más ancho a la izquierda."""
    cell, m1 = ly.cell(BITCELL), ly.layer(68, 20)
    old = db.Box(2190, -190, 2400, 190)
    shapes = [s for s in cell.shapes(m1).each() if s.is_box() and s.box == old]
    assert len(shapes) == 1, "el rect de met1 del bitcell"
    shapes[0].box = db.Box(2180, -190, 2400, 190)


def straps(ly):
    """Straps horizontales de met5 sobre la matriz, todavía sin vías."""
    top, m5 = ly.top_cell(), ly.layer(72, 20)
    w = top.bbox().width()
    for y_um in (100, 200, 300):
        top.shapes(m5).insert(db.Box(0, int(y_um / ly.dbu), w, int((y_um + 1.6) / ly.dbu)))


def rename_control(ly):
    ly.rename_cell(ly.cell(CONTROL).cell_index(), f"{NAME}_control_logic_read")


def wider_din_pins(ly):
    """El tramo de borde de met4 de cada pin din0[*], 60 nm más ancho de cada
    lado (sigue unido a su ruta)."""
    top, m4, txt = ly.top_cell(), ly.layer(71, 20), ly.layer(71, 5)
    n = 0
    for t in top.shapes(txt).each():
        if not (t.is_text() and t.text.string.startswith("din0[")):
            continue
        p = t.text.trans.disp
        for s in list(top.shapes(m4).each_overlapping(db.Box(p.x - 1, p.y - 1, p.x + 1, p.y + 1))):
            b = s.box if s.is_box() else None
            if b and b.bottom == 0 and b.height() < 2000:
                s.box = db.Box(b.left - 60, b.bottom, b.right + 60, b.top)
                n += 1
    assert n == 32, f"{n} pines din0"


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "examples/demos/chip.bundle"
    repo = Repo("/tmp/riku-demo-chip")
    print("chip")
    repo.write("README.md", README)
    repo.copy(LICENSE, "LICENSE")
    ly = db.Layout()
    ly.read(SRC)
    write(ly, repo.file(GDS))
    repo.commit(f"Initial macro: {NAME} from sky130_sram_macros", tag="v1.0")

    edit(repo, wider_bitcell_met1)
    repo.commit("Bitcell: one metal1 shape 10 nm wider")

    repo.branch("pin-fix")
    edit(repo, wider_din_pins)
    repo.commit("Wider din0 pin stubs (+60 nm each side)")

    repo.checkout("main")
    edit(repo, straps)
    repo.commit("Top: metal5 straps over the array")
    edit(repo, rename_control)
    repo.commit("Rename control_logic_r to control_logic_read")

    def resolve():
        # Git no fusiona el GDS (binario): el de main con los pines de la rama.
        repo.git("checkout", "--ours", GDS)
        edit(repo, wider_din_pins)
    repo.merge("pin-fix", "Merge branch 'pin-fix'", resolve)
    repo.bundle(os.path.abspath(out))


if __name__ == "__main__":
    main()
