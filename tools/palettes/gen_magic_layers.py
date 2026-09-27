"""Genera riku-mod-layout/src/magic_layers_generated.rs desde los .tech de Magic.

    python3 tools/palettes/gen_magic_layers.py [PDK_ROOT]     # por defecto /foss/pdks

Por cada tipo de capa de Magic (sección `types`, con sus alias) guarda su
plano y, si es un contacto (sección `contact`), el plano de su residuo de más
arriba: `viali` está en el plano `locali` y conecta hacia `metal1`. Con eso,
`palette.rs` le da a cada capa de Magic el color y el orden de apilado de la
capa GDS equivalente del PDK (`metal1` → met1, `viali` → mcon, `ndiffc` →
licon1), sin reproducir la conversión completa a GDS (`cifoutput`).

Correr de nuevo cuando cambie el PDK.
"""
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "riku-mod-layout", "src", "magic_layers_generated.rs")

# (constante, ruta relativa a PDK_ROOT)
PDKS = [
    ("SKY130_MAGIC", "sky130A/libs.tech/magic/sky130A.tech"),
    ("GF180_MAGIC", "gf180mcuD/libs.tech/magic/gf180mcuD.tech"),
    ("IHP_MAGIC", "ihp-sg13g2/libs.tech/magic/ihp-sg13g2.tech"),
]


def sections(path):
    """Líneas sin comentarios de cada sección de primer nivel del .tech."""
    out, current = {}, None
    with open(path, encoding="utf-8", errors="replace") as f:
        for raw in f:
            line = raw.split("#", 1)[0].rstrip()
            if not line.strip():
                continue
            word = line.strip()
            if current is None:
                current = word.split()[0]
                out.setdefault(current, [])
            elif word == "end":
                current = None
            else:
                out[current].append(word)
    return out


def table(path):
    sec = sections(path)
    planes = {}  # alias -> (nombre del plano, orden)
    for i, line in enumerate(sec.get("planes", [])):
        names = line.split()[0].split(",")
        for n in names:
            planes[n] = (names[0], i)
    types = {}  # alias -> (tipo canónico, plano)
    for line in sec.get("types", []):
        parts = line.lstrip("-").split()
        if len(parts) < 2:
            continue
        plane = planes.get(parts[0], (parts[0], 99))[0]
        names = parts[1].split(",")
        for n in names:
            types[n] = (names[0], plane)
    upper = {}  # tipo canónico del contacto -> plano del residuo de arriba
    for line in sec.get("contact", []):
        parts = line.split()
        if parts[0] in ("stackable", "lock", "unlock") or parts[0] not in types:
            continue
        residues = [types[r][1] for r in parts[1:] if r in types]
        if residues:
            upper[types[parts[0]][0]] = max(residues, key=lambda p: planes.get(p, (p, 99))[1])
    rows = []
    for alias, (canonical, plane) in types.items():
        rows.append((alias, plane, upper.get(canonical, "")))
    return sorted(set(rows))


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("PDK_ROOT", "/foss/pdks")
    out = [
        "// @generated por tools/palettes/gen_magic_layers.py desde los .tech de Magic. No editar.",
        "//",
        "// Por cada nombre de capa de Magic (con sus alias): su plano y, si es un",
        "// contacto, el plano de su residuo de más arriba. Ordenado por nombre.",
        "",
    ]
    for const, rel in PDKS:
        rows = table(os.path.join(root, rel))
        out.append(f"/// {len(rows)} nombres de `{rel}`: (nombre, plano, plano de arriba del contacto o \"\").")
        out.append(f"pub(crate) const {const}: &[(&str, &str, &str)] = &[")
        for name, plane, up in rows:
            out.append(f'    ("{name}", "{plane}", "{up}"),')
        out.append("];")
        out.append("")
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(out))
    print(f"{OUT}: " + ", ".join(f"{c} {len(table(os.path.join(root, r)))}" for c, r in PDKS))


main()
