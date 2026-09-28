#!/usr/bin/env python3
"""Transistores que reconoce Riku contra la netlist de referencia del PDK.

    compare_spice.py <salida de `devices`> <netlist .spice/.cdl> [--scale S] [--known a,b]

Por celda compara el multiconjunto de (modelo, W, L) en µm, redondeados a
1 nm, solo de transistores (modelos con `fet`, `nmos` o `pmos`; los diodos
de antena y demás no son de este nivel).

Del lado de la netlist: `.option scale=` (o `--scale`, si la escala viene
de otro archivo, como en SKY130: `w=650000u` con escala 1e-6) multiplica W
y L; `m` (multiplicidad) repite el dispositivo y `ng`/`nf` (fingers) lo
parte en fingers de W/ng, porque Riku cuenta cada finger. El modelo se compara sin el prefijo de la librería
(`sky130_fd_pr__nfet_01v8` = `nfet_01v8`), porque algunas netlists lo omiten.

`--known`: celdas cuya netlist de referencia no coincide con su propio
layout (Riku da lo mismo que el extractor de KLayout, ver
`klayout_gates.py`); se informan pero no hacen fallar.

Imprime las celdas que difieren y un resumen; sale con 1 si alguna difiere.
"""
import re
import sys
from collections import Counter

SI = {"f": 1e-15, "p": 1e-12, "n": 1e-9, "u": 1e-6, "m": 1e-3, "k": 1e3, "meg": 1e6, "g": 1e9}


def value(s):
    """`650000u`, `1e+06u`, `8.2e-07`, `740.00n` → valor en unidades SI."""
    m = re.fullmatch(r"([-+0-9.eE]+)(meg|[fpnumkg])?", s.strip().lower())
    if not m:
        raise ValueError(s)
    return float(m.group(1)) * SI.get(m.group(2) or "", 1.0)


def model_key(model):
    return model.split("__")[-1].lower()


def read_riku(path):
    cells, cur = {}, None
    for line in open(path, encoding="utf-8"):
        w = line.split()
        if not w:
            continue
        if w[0] == "CELL":
            cur = cells.setdefault(w[1], Counter())
        elif w[0] == "DEV" and cur is not None:
            wv, lv = float(w[2][2:]), float(w[3][2:])
            cur[(model_key(w[1]), round(wv, 3), round(lv, 3))] += 1
    return cells


def is_mos(model):
    m = model.lower()
    return "fet" in m or "nmos" in m or "pmos" in m


def read_netlist(path, scale=1.0):
    """Transistores por subcircuito, con las instancias de otros
    subcircuitos (`X… sub`) expandidas, como Riku aplana la celda."""
    cells, subs, cur = {}, {}, None
    lines, pending = [], ""
    for raw in open(path, encoding="utf-8", errors="replace"):
        line = raw.rstrip("\n")
        if line.startswith("+"):
            pending += " " + line[1:]
            continue
        if pending:
            lines.append(pending)
        pending = line
    lines.append(pending)
    for line in lines:
        w = line.split()
        if not w or w[0].startswith("*"):
            continue
        head = w[0].lower()
        if head == ".option":
            for p in w[1:]:
                if p.lower().startswith("scale="):
                    scale = value(p.split("=", 1)[1])
            continue
        if head == ".subckt":
            cur = cells.setdefault(w[1], Counter())
            subs[w[1]] = []
            cur_name = w[1]
            continue
        if head == ".ends":
            cur = None
            continue
        if cur is None or head[0] not in "xm":
            continue
        params = {k.lower(): v for k, v in (p.split("=", 1) for p in w if "=" in p)}
        if "w" not in params or "l" not in params:
            # Instancia de otro subcircuito: su nombre es la última palabra.
            words = [t for t in w[1:] if "=" not in t]
            if head[0] == "x" and words:
                subs[cur_name].append(words[-1])
            continue
        # El modelo es la última palabra sin '=' antes de los parámetros.
        words = [t for t in w[1:] if "=" not in t]
        model = words[-1]
        if not is_mos(model):
            continue
        mult = int(float(params.get("m", params.get("mult", "1"))))
        ng = int(float(params.get("ng", params.get("nf", "1"))))
        wv, lv = value(params["w"]) * scale * 1e6 / ng, value(params["l"]) * scale * 1e6
        cur[(model_key(model), round(wv, 3), round(lv, 3))] += mult * ng

    def flat(name, depth=0):
        out = Counter(cells.get(name, Counter()))
        if depth < 16:
            for sub in subs.get(name, []):
                out += flat(sub, depth + 1)
        return out

    return {name: flat(name) for name in cells}


def main():
    args = sys.argv[1:]
    if len(args) < 2:
        print(__doc__)
        return 2
    scale = float(args[args.index("--scale") + 1]) if "--scale" in args else 1.0
    known = set(args[args.index("--known") + 1].split(",")) if "--known" in args else set()
    riku, ref = read_riku(args[0]), read_netlist(args[1], scale)
    same, differ, missing = 0, [], []
    for name, got in sorted(riku.items()):
        want = ref.get(name)
        if want is None:
            missing.append(name)
        elif got == want:
            same += 1
        else:
            differ.append((name, got, want))
    for name, got, want in differ[:40]:
        extra, lack = got - want, want - got
        tag = "CONOCIDA" if name in known else "DIFIERE"
        print(f"{tag} {name}: Riku de más {dict(extra)} · le falta {dict(lack)}")
    total = same + len(differ)
    unknown = [d for d in differ if d[0] not in known]
    print(f"{same} de {total} celdas iguales a la netlist · {len(differ) - len(unknown)} conocidas · {len(unknown)} difieren · {len(missing)} sin subcircuito en la netlist")
    return 1 if unknown else 0


if __name__ == "__main__":
    sys.exit(main())
