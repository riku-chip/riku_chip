#!/usr/bin/env python3
"""Netlists que extrae Riku contra las de referencia del PDK, con Netgen.

    compare_netgen.py <salida de `nets`> <netlist de referencia> <setup.tcl> [--known a,b] [--jobs N]

Por cada `.subckt` de la salida de Riku corre `netgen -batch lvs` contra la
celda del mismo nombre en la referencia, con el setup del PDK (el mismo que
usa un LVS real: compara la topología entera, qué transistor va a qué red,
y W/L). Las celdas sin subcircuito en la referencia se cuentan aparte.

`--known`: celdas cuya netlist de referencia no coincide con su propio
layout (ver `tools/verify/devices/`); se informan pero no hacen fallar.
"""
import os
import re
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

NETGEN = os.environ.get("NETGEN", "netgen")


def subckts(path):
    """Nombre → texto de cada .subckt del archivo (sin distinguir mayúsculas)."""
    out, name, lines = {}, None, []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            w = line.split()
            if w and w[0].lower() == ".subckt":
                name, lines = w[1], [line]
            elif name is not None:
                lines.append(line)
                if w and w[0].lower() == ".ends":
                    out[name] = "".join(lines)
                    name = None
    return out


def run(cell, ours_text, ref, setup, tmp):
    d = tempfile.mkdtemp(dir=tmp)
    ours = os.path.join(d, "riku.sp")
    with open(ours, "w") as f:
        f.write(ours_text)
    log = os.path.join(d, "lvs.out")
    try:
        subprocess.run(
            [NETGEN, "-batch", "lvs", f"{ours} {cell}", f"{ref} {cell}", setup, log],
            cwd=d,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=300,
        )
    except subprocess.TimeoutExpired:
        return cell, "timeout"
    text = open(log, encoding="utf-8", errors="replace").read() if os.path.exists(log) else ""
    final = re.findall(r"Final result:\s*\n?\s*(.*)", text)
    result = final[-1].strip() if final else "sin resultado"
    if "Property errors were found" in text:
        result += " (con diferencias de W/L)"
    return cell, result


def main():
    args = sys.argv[1:]
    if len(args) < 3:
        print(__doc__)
        return 2
    known = set(args[args.index("--known") + 1].split(",")) if "--known" in args else set()
    jobs = int(args[args.index("--jobs") + 1]) if "--jobs" in args else os.cpu_count() or 4
    ours, ref, setup = args[0], args[1], args[2]
    mine = subckts(ours)
    theirs = {k.lower() for k in subckts(ref)}
    cells = sorted(c for c in mine if c.lower() in theirs)
    missing = len(mine) - len(cells)
    with tempfile.TemporaryDirectory() as tmp, ThreadPoolExecutor(jobs) as pool:
        results = list(pool.map(lambda c: run(c, mine[c], ref, setup, tmp), cells))
    # Celdas sin transistores (relleno, tomas, diodos): Netgen no las compara.
    empty = [c for c, r in results if "no elements" in r or not any(l[:1] in "XxMmRr" for l in mine[c].splitlines()[1:-1])]
    results = [(c, r) for c, r in results if c not in empty]
    same = [c for c, r in results if "match uniquely" in r and "W/L" not in r]
    differ = [(c, r) for c, r in results if c not in same]
    for c, r in differ[:40]:
        tag = "CONOCIDA" if c in known else "DIFIERE"
        print(f"{tag} {c}: {r}")
    unknown = [c for c, _ in differ if c not in known]
    print(f"{len(same)} de {len(results)} celdas iguales (Netgen) · {len(differ) - len(unknown)} conocidas · {len(unknown)} difieren · {missing} sin subcircuito en la referencia · {len(empty)} vacías")
    return 1 if unknown else 0


if __name__ == "__main__":
    sys.exit(main())
