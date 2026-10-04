"""Demo `inversor`: el inversor de 5 V de SKY130 en Magic y Xschem, con
11 commits, una rama y un merge. Muestra lo propio de Magic: las capas por
nombre, un transistor que cambia en su sub-celda visto desde la de arriba,
puertos que cambian de clase, un abierto y una re-grabación que solo toca los
`timestamp`; y el LVS en cada paso.

Fuente: `demo_sky130A/ana` de iic-osic-tools (Apache-2.0, ver LICENSE en
`tools/demos/inversor`).

    python3 tools/demos/inversor.py <salida.bundle>

Antes de escribir el bundle comprueba cada commit: el veredicto del LVS de
`riku log --lvs` contra la tabla `EXPECTED`, y que la netlist que extrae Riku
de cada layout sea la misma que la de Magic (`extract all` + Netgen). Usa
`RIKU` (el ejecutable) y `NETS` (el ejemplo `nets` de riku-mod-layout).
"""
import json
import os
import re
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import Repo, set_param, sh  # noqa: E402

SRC = os.path.join(os.path.dirname(os.path.abspath(__file__)), "inversor")
NFET = "layout/sky130_fd_pr__nfet_g5v0d10v5_H9JWFY.mag"
PFET = "layout/sky130_fd_pr__pfet_g5v0d10v5_5AEDG4.mag"
TOP = "layout/inv.mag"
SCH = "xschem/inv.sch"
TB = "xschem/tb_inv.sch"
TARGET = os.environ.get("CARGO_TARGET_DIR", "/headless/riku-target/ws")
RIKU = os.environ.get("RIKU", f"{TARGET}/debug/riku")
NETS = os.environ.get("NETS", f"{TARGET}/debug/examples/nets")
PDK_ROOT = os.environ.get("PDK_ROOT", "/foss/pdks")

# Un paso de la grilla de estos .mag (`magscale 1 2`) son 0,005 µm.
STEP_UM = 0.005

README = """# Inverter in Magic + Xschem (SKY130, 5 V) · Riku demo

A 5 V inverter in SKY130: the layout in Magic (`layout/inv.mag`, which uses two
transistor sub-cells) and the schematic in Xschem (`xschem/inv.sch`), in 11
commits with a branch and a merge. Each commit shows something Riku sees in a
Magic layout, and the LVS follows the whole history.

Things to try:

```bash
riku log --graph --lvs                 # the history and the LVS of each commit
riku show HEAD~8                       # metal2 by its Magic name, not a GDS number
riku show HEAD~7                       # the NMOS sub-cell got wider: seen from inv, w 2 -> 2.5
riku lvs HEAD~7                        # ... and the LVS stopped matching (M9 w 2 vs 2.5)
riku show HEAD~5                       # ports that changed class (input, output)
riku show HEAD~4                       # an open: out split in two
riku show HEAD~3                       # the fix: the two pieces of out join again
riku show HEAD~2                       # Magic re-saved the cells: only timestamps, no changes
riku diff v0.1 v1.0                    # everything between the first and the fixed design
riku gui .                             # the viewer: H for the history, LVS for the cross-probing
```

(`HEAD~N` counts back from the merge; `riku log --graph` shows the commits.)

Source: the `demo_sky130A/ana` inverter of
[iic-osic-tools](https://github.com/iic-jku/iic-osic-tools) (Apache-2.0, see
`LICENSE`). The changes are Riku's, to show its diffs.
"""

# Veredicto y transición del LVS de cada commit (por el asunto), respecto de su
# primer padre: lo que tiene que dar `riku log --lvs`.
EXPECTED = {
    "Initial inverter: Magic layout and Xschem schematic": ("match", None),
    "Layout: wider metal2 on out": ("match", None),
    "Layout: wider NMOS (W 2 -> 2.5 um) in its sub-cell": ("property_errors", "broke"),
    "Schematic: M9 W 2 -> 2.5 to match the layout": ("match", "fixed"),
    "Layout: in is an input port, out an output": ("match", None),
    "Layout: route out in two pieces (open)": ("mismatch", "broke"),
    "Fix: join out again": ("match", "fixed"),
    "Re-save in Magic (timestamps only)": ("match", None),
    "Longer NMOS (L 0.5 -> 0.6 um): layout and schematic": ("match", None),
    "Testbench: 100 ps input edges": ("match", None),
    "Merge branch 'longer-nmos'": ("match", None),
}

RECT = re.compile(r"^(rect|rlabel \S+) (-?\d+) (-?\d+) (-?\d+) (-?\d+)", re.M)


def stretch(mag, cut, dy):
    """Estira un .mag como `stretch` de Magic: las coordenadas `y` del lado de
    `cut` hacia donde va `dy` se corren `dy`; lo que cruza la línea se alarga.
    Así cambia el W de un transistor sin mover lo que lo conecta afuera."""
    def move(y):
        y = int(y)
        return y + dy if (dy > 0 and y > cut) or (dy < 0 and y < cut) else y

    def rect(m):
        x1, y1, x2, y2 = m.group(2), move(m.group(3)), m.group(4), move(m.group(5))
        return f"{m.group(1)} {x1} {y1} {x2} {y2}"

    mag = RECT.sub(rect, mag)
    return re.sub(r"(string FIXED_BBOX -?\d+ )(-?\d+)( -?\d+ )(-?\d+)",
                  lambda m: f"{m.group(1)}{move(m.group(2))}{m.group(3)}{move(m.group(4))}", mag)


def set_box(top, cell, dy_low, dy_high, dx=0):
    """La caja de la instancia de `cell` en la celda de arriba, como la
    recalcula Magic al cargarla."""
    def box(m):
        x1, y1, x2, y2 = (int(v) for v in m.group(2).split())
        return f"{m.group(1)}box {x1 - dx} {y1 + dy_low} {x2 + dx} {y2 + dy_high}"
    out, n = re.subn(r"(use " + re.escape(cell) + r" [^\n]*\n(?:[^\n]*\n){2})box ([-\d ]+)", box, top)
    assert n == 1, cell
    return out


def stretch_x(mag, dx):
    """Estira en horizontal desde el centro: lo de la derecha de x = 0, `dx` a
    la derecha; lo de la izquierda, a la izquierda."""
    def move(x):
        x = int(x)
        return x + dx if x > 0 else x - dx if x < 0 else x

    mag = RECT.sub(lambda m: f"{m.group(1)} {move(m.group(2))} {m.group(3)} {move(m.group(4))} {m.group(5)}", mag)
    return re.sub(r"(string FIXED_BBOX )(-?\d+)( -?\d+ )(-?\d+)",
                  lambda m: f"{m.group(1)}{move(m.group(2))}{m.group(3)}{move(m.group(4))}", mag)


def set_w(mag, w):
    out, n = re.subn(r"(string parameters w )[\d.]+", lambda m: f"{m.group(1)}{w}", mag)
    assert n == 1
    return out


def replace_once(text, old, new):
    assert text.count(old) == 1, old
    return text.replace(old, new)


def timestamps(mag, value):
    return re.sub(r"^timestamp \d+", f"timestamp {value}", mag, flags=re.M)


def build():
    repo = Repo("/tmp/riku-demo-inversor")
    print("inversor")
    repo.write("README.md", README)
    repo.copy(os.path.join(SRC, "LICENSE"), "LICENSE")
    for f in ["inv.sch", "inv.sym", "tb_inv.sch"]:
        repo.copy(os.path.join(SRC, f), "xschem/" + f)
    for f in ["inv.mag", os.path.basename(NFET), os.path.basename(PFET)]:
        repo.copy(os.path.join(SRC, f), "layout/" + f)
    repo.commit("Initial inverter: Magic layout and Xschem schematic", tag="v0.1")

    # Capas por nombre: el tramo de metal2 de `out` más ancho.
    top = repo.read(TOP)
    repo.write(TOP, replace_once(top, "rect 310 -420 392 194", "rect 300 -420 402 194"))
    repo.commit("Layout: wider metal2 on out")

    # El nfet, 0,5 µm más ancho hacia arriba en su sub-celda (W 2 → 2,5): el
    # contacto de compuerta sigue bajo `in` y el cuerpo, unido a VSS.
    dy = round(0.5 / STEP_UM)
    repo.write(NFET, set_w(stretch(repo.read(NFET), 163, dy), "2.5"))
    repo.write(TOP, set_box(repo.read(TOP), os.path.basename(NFET)[:-4], 0, dy))
    repo.commit("Layout: wider NMOS (W 2 -> 2.5 um) in its sub-cell")
    repo.write(SCH, set_param(repo.read(SCH), "M9", "W", "2.5"))
    repo.commit("Schematic: M9 W 2 -> 2.5 to match the layout")

    top = repo.read(TOP)
    top = replace_once(top, "rlabel metal1 224 -336 316 134 1 in\nport 1 n", "rlabel metal1 224 -336 316 134 1 in\nport 1 n signal input")
    top = replace_once(top, "rlabel metal2 310 -420 392 204 1 out\nport 2 n", "rlabel metal2 310 -420 392 204 1 out\nport 2 n signal output")
    repo.write(TOP, top)
    repo.commit("Layout: in is an input port, out an output")

    # Rama: el nfet 0,1 µm más largo (L 0,5 → 0,6), estirado a los dos lados
    # de la compuerta: fuente y drenaje siguen sobre sus conexiones de afuera.
    # (El pmos no se puede ensanchar así: sus drenajes tocarían `in` abajo y
    # su anillo se separaría de VDD arriba; habría que re-rutear.)
    repo.branch("longer-nmos")
    dx = round(0.05 / STEP_UM)
    nfet = stretch_x(repo.read(NFET), dx)
    nfet, n = re.subn(r"(string parameters w [\d.]+ l )[\d.]+", lambda m: f"{m.group(1)}0.60", nfet)
    assert n == 1
    repo.write(NFET, nfet)
    repo.write(TOP, set_box(repo.read(TOP), os.path.basename(NFET)[:-4], 0, 0, dx))
    repo.write(SCH, set_param(repo.read(SCH), "M9", "L", "0.6"))
    repo.commit("Longer NMOS (L 0.5 -> 0.6 um): layout and schematic")

    repo.checkout("main")
    # Un abierto: el metal2 que une la vía del drenaje del pmos con la columna
    # que baja al nmos, quitado. La etiqueta `out` queda entera sobre la
    # columna (si quedara sobre el hueco, Magic la dejaría sin conectar).
    top = repo.read(TOP)
    cut = replace_once(top, "rect 322 204 392 348\n", "")
    repo.write(TOP, replace_once(cut, "rect 206 194 392 204\n", ""))
    repo.commit("Layout: route out in two pieces (open)")
    repo.write(TOP, top)
    repo.commit("Fix: join out again", tag="v1.0")

    # Magic reescribe los timestamp de la celda y de sus padres al grabar.
    for f in [TOP, NFET, PFET]:
        repo.write(f, timestamps(repo.read(f), 1772442000))
    repo.commit("Re-save in Magic (timestamps only)")

    tb = repo.read(TB)
    repo.write(TB, replace_once(tb, "pulse 5 0 0 1n 1n 0.05u 0.1u", "pulse 5 0 0 100p 100p 0.05u 0.1u"))
    repo.commit("Testbench: 100 ps input edges")

    def resolve():
        # Los timestamp de main con la geometría de la rama.
        for f in [NFET, TOP]:
            repo.git("checkout", "--theirs", f)
            repo.write(f, timestamps(repo.read(f), 1772442000))
    repo.merge("longer-nmos", "Merge branch 'longer-nmos'", resolve)
    return repo


def check_lvs(repo):
    """El veredicto de `riku log --lvs` de cada commit, contra `EXPECTED`."""
    env = dict(os.environ, RIKU_NO_CACHE="1", RIKU_LANG="en")
    out = subprocess.run([RIKU, "log", "--lvs", "-n", "30", "-f", "json"], cwd=repo.path, env=env,
                         capture_output=True, text=True, check=True).stdout
    seen = {}
    for c in json.loads(out)["commits"]:
        subject = c["message"].splitlines()[0]
        lvs = c.get("lvs") or [{}]
        seen[subject] = (lvs[0].get("verdict"), lvs[0].get("transition"))
    bad = [(s, seen.get(s), want) for s, want in EXPECTED.items() if seen.get(s) != want]
    for s, got, want in bad:
        print(f"  LVS distinto en «{s}»: {got}, se esperaba {want}")
    print(f"  LVS: {len(EXPECTED) - len(bad)} de {len(EXPECTED)} commits como se esperaba")
    return not bad


# Lo que dice el README que se ve en cada commit (`riku show`, en inglés).
SHOWS = {
    "HEAD~8": ["metal2"],
    "HEAD~7": ["w_um: 2.000 → 2.500"],
    "HEAD~5": ["class: — → input", "class: — → output"],
    "HEAD~4": ["open (a net split): out"],
    "HEAD~2": ["no semantic changes"],
}


def check_show(repo):
    """Cada `riku show` del README dice lo que el README promete."""
    env = dict(os.environ, RIKU_NO_CACHE="1", RIKU_LANG="en")
    ok = True
    for rev, wants in SHOWS.items():
        out = subprocess.run([RIKU, "show", rev], cwd=repo.path, env=env, capture_output=True, text=True).stdout
        for w in wants:
            if w not in out:
                ok = False
                print(f"  riku show {rev}: falta «{w}»")
    print(f"  riku show: {'lo que dice el README' if ok else 'no coincide con el README'}")
    return ok


def check_magic(repo):
    """La netlist de Riku de cada commit, igual a la que extrae Magic."""
    rc = f"{PDK_ROOT}/sky130A/libs.tech/magic/sky130A.magicrc"
    setup = f"{PDK_ROOT}/sky130A/libs.tech/netgen/sky130A_setup.tcl"
    ok = True
    for line in repo.git("log", "--format=%h %s").splitlines():
        rev, subject = line.split(" ", 1)
        with tempfile.TemporaryDirectory() as d:
            sh(f"git archive {rev} layout | tar -x -C {d}", cwd=repo.path)
            lay = os.path.join(d, "layout")
            with open(os.path.join(lay, "x.tcl"), "w") as f:
                f.write("load inv\nselect top cell\nextract all\next2spice lvs\next2spice -o magic.spice\nquit -noprompt\n")
            sh(["magic", "-dnull", "-noconsole", "-rcfile", rc, "x.tcl"], cwd=lay, env={"PDK_ROOT": PDK_ROOT}, check=False)
            riku = sh([NETS, "--unit", "", os.path.join(lay, "inv.mag")], check=False)
            open(os.path.join(lay, "riku.spice"), "w").write(riku)
            sh(["netgen", "-batch", "lvs", "riku.spice inv", "magic.spice inv", setup, "lvs.out"], cwd=lay, check=False)
            report = open(os.path.join(lay, "lvs.out")).read() if os.path.exists(os.path.join(lay, "lvs.out")) else ""
            same = "Circuits match uniquely" in report and "Property errors were found" not in report
            if not same:
                ok = False
                print(f"  Magic distinto de Riku en {rev} «{subject}»")
    print(f"  Magic: {'cada commit igual a Riku' if ok else 'hay diferencias'}")
    return ok


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "examples/demos/inversor.bundle"
    repo = build()
    lvs_ok, show_ok, magic_ok = check_lvs(repo), check_show(repo), check_magic(repo)
    if not (lvs_ok and show_ok and magic_ok):
        raise SystemExit("el demo no da lo esperado: no se escribe el bundle")
    repo.bundle(os.path.abspath(out))


if __name__ == "__main__":
    main()
