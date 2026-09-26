"""Genera gds-renderer/src/palette_generated.rs desde los .lyp oficiales.

    python3 tools/palettes/gen_palettes.py [PDK_ROOT]     # por defecto /foss/pdks

Por cada capa del .lyp (layer/datatype) toma nombre y color de relleno. El
rol sale de la convencion de datatypes de cada PDK (la misma que usa
`layer_spec` para capas desconocidas) y del estilo del .lyp: una capa sin
tramado (`I1`) o invisible por defecto se dibuja solo con contorno.

Las tablas curadas a mano en palette.rs siguen mandando: estas solo cubren
las capas que ellas no tienen. Correr de nuevo cuando cambie el PDK.
"""
import os
import re
import sys
import xml.etree.ElementTree as ET

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "gds-renderer", "src", "palette_generated.rs")

# (constante, ruta relativa a PDK_ROOT, datatypes que se rellenan)
PDKS = [
    ("GF180_LYP", "gf180mcuD/libs.tech/klayout/tech/gf180mcu.lyp", {0, 4}),
    ("IHP_LYP", "ihp-sg13g2/libs.tech/klayout/tech/sg13g2.lyp", {0, 20, 22}),
]

SOURCE = re.compile(r"(?:^|\s)(\d+)/(\d+)(?:@\d+)?\s*$")


def layers(path, filled_datatypes):
    seen, out = set(), []
    for p in ET.parse(path).getroot().iter("properties"):
        m = SOURCE.search(p.findtext("source") or "")
        if not m:
            continue
        tag = (int(m.group(1)), int(m.group(2)))
        if tag in seen:
            continue
        seen.add(tag)
        src_name = (p.findtext("source") or "")[: m.start()].strip()
        name = (p.findtext("name") or "").strip() or src_name or f"{tag[0]}/{tag[1]}"
        color = (p.findtext("fill-color") or p.findtext("frame-color") or "#808080").lstrip("#")
        hollow = (p.findtext("dither-pattern") or "") == "I1" or (p.findtext("visible") or "true") == "false"
        role = "D" if tag[1] in filled_datatypes and not hollow else "O"
        out.append((tag, name, color[:6].lower(), role))
    return out


def rust_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main():
    pdk_root = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("PDK_ROOT", "/foss/pdks")
    lines = [
        "// @generated por tools/palettes/gen_palettes.py desde los .lyp oficiales. No editar.",
        "//",
        "// Capas completas de cada PDK (nombre y color del .lyp). `layer_spec` las",
        "// usa solo para capas que no estan en las tablas curadas de palette.rs.",
        "",
        "use crate::palette::{pl, rgb, PdkLayer};",
        "use crate::palette::LayerRole::{Device as D, Outline as O};",
        "",
    ]
    for const, rel, filled in PDKS:
        path = os.path.join(pdk_root, rel)
        rows = layers(path, filled)
        lines.append(f"/// {len(rows)} capas de `{rel}`.")
        lines.append(f"pub(crate) const {const}: &[PdkLayer] = &[")
        for (l, d), name, c, role in rows:
            lines.append(f"    pl({l}, {d}, {rust_str(name)}, rgb(0x{c[0:2]}, 0x{c[2:4]}, 0x{c[4:6]}), {role}),")
        lines.append("];")
        lines.append("")
        print(f"{const}: {len(rows)} capas")
    with open(OUT, "w", newline="\n") as f:
        f.write("\n".join(lines))
    print(OUT)


if __name__ == "__main__":
    main()
