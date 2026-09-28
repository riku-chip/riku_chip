"""Demo `ota`: un OTA de 5 transistores de SKY130 (esquemático, testbench,
layout y simulación) con 11 commits, una rama y un merge.

Fuente: el ejemplo `ota-5t` de CACE en iic-osic-tools (Apache-2.0, ver
LICENSE en esta carpeta).

    python3 tools/demos/ota.py <salida.bundle>
"""
import os
import sys

import klayout.db as db

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import Repo, move_all, set_param, simulate  # noqa: E402

SRC = os.path.join(os.path.dirname(os.path.abspath(__file__)), "ota")
GDS = "layout/ota-5t.gds"
SCH = "xschem/ota-5t.sch"
TB = "xschem/ota-5t_tb.sch"

README = """# 5-transistor OTA (SKY130) · Riku demo

An operational transconductance amplifier in SKY130 with its schematic
(`xschem/`), testbench, layout (`layout/ota-5t.gds`) and ngspice simulation
(`sim/ota-5t_tb.raw`), in 11 commits with a branch and a merge.

Things to try:

```bash
riku log --graph                 # the history, with the narrow-input-pair branch
riku show HEAD~7                 # a wider PMOS load, and how much the simulation moved
riku show HEAD~3                 # a short in the layout: ! Vout = Vp
riku show HEAD~2                 # the fix
riku diff v0.1 v1.0              # everything that changed
riku diff v0.1 v1.0 layout/ota-5t.gds -f visual   # the layout, in the viewer
riku open                        # the viewer; H opens the history
```

Source: the `ota-5t` example of [CACE](https://github.com/efabless/cace) in
iic-osic-tools (Apache-2.0, see `LICENSE`). The changes are Riku's, to show
its diffs.
"""


def edit_gds(repo, fn):
    ly = db.Layout()
    ly.read(repo.file(GDS))
    fn(ly, ly.top_cell())
    ly.write(repo.file(GDS))


def layer(ly, l, d):
    return ly.layer(l, d)


def box(ly, x0, y0, x1, y1):
    return db.Box(round(x0 / ly.dbu), round(y0 / ly.dbu), round(x1 / ly.dbu), round(y1 / ly.dbu))


def straps(ly, top):
    """Correas de metal5 sobre VDD y VSS, con vías a metal4."""
    m5, v4 = layer(ly, 72, 20), layer(ly, 71, 44)
    for y0, y1 in [(12.3, 13.9), (-16.4, -14.8)]:
        top.shapes(m5).insert(box(ly, -6.0, y0, 11.0, y1))
        x = -5.5
        while x + 0.8 < 10.8:
            top.shapes(v4).insert(box(ly, x, y0 + 0.4, x + 0.8, y0 + 1.2))
            x += 2.0


def trim_input_pair(ly, top):
    """La difusión del par de entrada 0,5 µm más corta (W por dedo 5 → 4,5)."""
    diff = layer(ly, 65, 20)
    region = db.Region(top.shapes(diff)) - db.Region(box(ly, -0.4, 5.6, 5.7, 6.1))
    top.shapes(diff).clear()
    top.shapes(diff).insert(region)


# Las pistas de metal3 de Vout (horizontal, arriba) y de Vp (horizontal, a
# la izquierda), a la altura de sus etiquetas.
VOUT_Y, VOUT_LEFT = 9.35, -0.36
VP_Y, VP_LEFT = -0.36, -3.41


def route_vout(ly, top, short):
    """Vout llevado al borde izquierdo en metal3. Con `short`, la pista baja
    por el borde y entra en la de Vp."""
    m3 = layer(ly, 70, 20)
    top.shapes(m3).insert(box(ly, -5.6, VOUT_Y - 0.2, VOUT_LEFT + 0.1, VOUT_Y + 0.2))
    if short:
        top.shapes(m3).insert(box(ly, -5.6, VP_Y - 0.2, -5.2, VOUT_Y + 0.2))
        top.shapes(m3).insert(box(ly, -5.6, VP_Y - 0.2, VP_LEFT + 0.3, VP_Y + 0.2))


def unroute_short(ly, top):
    """Saca la bajada que tocaba a Vp: queda la pista horizontal de Vout."""
    m3 = layer(ly, 70, 20)
    region = db.Region(top.shapes(m3))
    region -= db.Region(box(ly, -5.7, VP_Y - 0.3, VP_LEFT - 0.05, VP_Y + 0.3))
    region -= db.Region(box(ly, -5.7, VP_Y + 0.3, -5.1, VOUT_Y - 0.2))
    top.shapes(m3).clear()
    top.shapes(m3).insert(region.merged())


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "examples/demos/ota.bundle"
    repo = Repo("/tmp/riku-demo-ota")
    print("ota")
    repo.write("README.md", README)
    repo.copy(os.path.join(SRC, "LICENSE"), "LICENSE")
    for f in ["ota-5t.sch", "ota-5t.sym", "ota-5t_tb.sch", "xschemrc"]:
        repo.copy(os.path.join(SRC, f), "xschem/" + f)
    repo.copy(os.path.join(SRC, "ota-5t.gds"), GDS)
    m = simulate(repo, "xschem", "ota-5t_tb.sch", "sim")
    repo.commit("Initial OTA: schematic, testbench, layout and simulation", tag="v0.1")
    print(f"    a0={m.get('a0'):.1f} dB ugf={m.get('ugf', 0) / 1e6:.1f} MHz")

    sch = repo.read(SCH)
    for inst in ["M1", "M2"]:
        sch = set_param(sch, inst, "W", "4")
    repo.write(SCH, sch)
    simulate(repo, "xschem", "ota-5t_tb.sch", "sim")
    repo.commit("Wider PMOS load: M1, M2 W 2u -> 4u")

    repo.branch("narrow-input-pair")
    sch = repo.read(SCH)
    for inst in ["M3", "M4"]:
        sch = set_param(sch, inst, "W", "18")
    repo.write(SCH, sch)
    simulate(repo, "xschem", "ota-5t_tb.sch", "sim")
    repo.commit("Narrower input pair: M3, M4 W 20u -> 18u")
    edit_gds(repo, trim_input_pair)
    repo.commit("Layout: trim the input pair diffusion to match")

    repo.checkout("main")
    edit_gds(repo, straps)
    repo.commit("Layout: metal5 power straps on VDD and VSS")
    repo.write(SCH, move_all(repo.read(SCH), 60, 40))
    repo.commit("Tidy up the schematic (move everything)")
    # Git no fusiona el GDS (binario): se toma el de main y se le aplica el
    # recorte de la rama, como haría el diseñador.
    def resolve():
        repo.git("checkout", "--ours", GDS)
        edit_gds(repo, trim_input_pair)
    repo.merge("narrow-input-pair", "Merge branch 'narrow-input-pair'", resolve)

    edit_gds(repo, lambda ly, top: route_vout(ly, top, short=True))
    repo.commit("Layout: route Vout to the left edge")
    edit_gds(repo, unroute_short)
    repo.commit("Fix: the Vout route no longer touches Vp")

    tb = repo.read(TB).replace("name=C1\nm=1\nvalue=1p}", "name=C1\nm=1\nvalue=2p}")
    assert "value=2p" in tb
    repo.write(TB, tb)
    simulate(repo, "xschem", "ota-5t_tb.sch", "sim")
    repo.commit("Testbench: 2 pF load")
    repo.write(SCH, repo.read(SCH).replace("lab=node", "lab=tail"))
    repo.commit("Rename the tail node: node -> tail", tag="v1.0")
    repo.bundle(os.path.abspath(out))


if __name__ == "__main__":
    main()
