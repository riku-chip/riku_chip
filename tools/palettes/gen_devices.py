"""Genera riku-mod-layout/src/devices/devices_generated.rs desde los .tech de Magic.

    python3 tools/palettes/gen_devices.py [PDK_ROOT]     # por defecto /foss/pdks

De cada `.tech` guarda solo lo que usan las reglas de transistores
(`riku-mod-layout/src/devices/rules.rs`), en el mismo formato, para que Riku
las lea con el mismo lector cuando el PDK no está instalado:

- el primer estilo de `cifinput`: `layer`, `templayer`, sus operaciones y
  los `calma`;
- el primer estilo de `extract`: las líneas `device` de transistores MOS;
- `types`: los alias de cada tipo (un `.mag` puede usar cualquiera).

Con el PDK instalado, Riku lee su `.tech` (ver `pdk_tech.rs`). Correr de
nuevo cuando cambie el PDK.
"""
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "riku-mod-layout", "src", "devices", "devices_generated.rs")

# (constante, ruta relativa a PDK_ROOT)
PDKS = [
    ("SKY130", "sky130A/libs.tech/magic/sky130A.tech"),
    ("GF180", "gf180mcuD/libs.tech/magic/gf180mcuD.tech"),
    ("IHP", "ihp-sg13g2/libs.tech/magic/ihp-sg13g2.tech"),
]

CIF_KEEP = {"layer", "templayer", "fault", "and", "and-not", "or", "copyup", "grow", "grow-grid", "shrink", "calma", "gds"}
MOS_CLASSES = {"mosfet", "msubcircuit"}


def read_tech(path, depth=0):
    """El texto del .tech con sus `include` puestos en su lugar."""
    out = []
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            s = line.strip()
            if s.startswith("include ") and depth < 4:
                inc = os.path.join(os.path.dirname(path), s.split(None, 1)[1])
                if os.path.isfile(inc):
                    out.append(read_tech(inc, depth + 1))
                    continue
            out.append(line)
    return "".join(out)


def sections(text):
    """Líneas de cada sección de primer nivel, sin comentarios y con las
    continuaciones (`\\` al final) unidas."""
    out, current, pending = {}, None, ""
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if line.endswith("\\"):
            pending += line[:-1] + " "
            continue
        line = (pending + line).strip()
        pending = ""
        if not line:
            continue
        if current is None:
            current = line.split()[0]
            out.setdefault(current, [])
        elif line == "end":
            current = None
        else:
            out[current].append(line)
    return out


def first_style(lines):
    out, seen = [], 0
    for line in lines:
        if line.split()[0] == "style":
            seen += 1
            if seen > 1:
                break
            continue
        out.append(line)
    return out


def trimmed(path):
    s = sections(read_tech(path))
    cif = [l for l in first_style(s.get("cifinput", [])) if l.split()[0] in CIF_KEEP]
    dev = [
        " ".join(l.split())
        for l in first_style(s.get("extract", []))
        if l.split()[0] == "device" and len(l.split()) > 3 and l.split()[1] in MOS_CLASSES
    ]
    types = [l for l in s.get("types", []) if len(l.split()) > 1 and "," in l.split()[1]]
    return (
        "types\n" + "\n".join(types) + "\nend\n"
        + "cifinput\nstyle riku\n" + "\n".join(cif) + "\nend\n"
        + "extract\nstyle riku\n" + "\n".join(dev) + "\nend\n"
    )


def main():
    pdk_root = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("PDK_ROOT", "/foss/pdks")
    parts = [
        "// @generated por tools/palettes/gen_devices.py desde los .tech de Magic. No editar.\n",
        "//! Reglas de transistores de cada PDK para cuando no está instalado: lo que\n",
        "//! `rules.rs` lee del `.tech` (primer estilo de `cifinput`, líneas `device`).\n",
    ]
    for const, rel in PDKS:
        path = os.path.join(pdk_root, rel)
        text = trimmed(path)
        n_dev = text.count("\ndevice ")
        parts.append(f"\n/// `{rel}`: {n_dev} líneas `device` de MOS.\npub const {const}: &str = r#\"{text}\"#;\n")
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write("".join(parts))
    print(f"{OUT}: {os.path.getsize(OUT) // 1024} KiB")


if __name__ == "__main__":
    main()
