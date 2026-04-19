import re
from riku.core.models import Component, Wire, Schematic

# Componente: puede tener atributos multilinea en el ultimo bloque {}
_COMPONENT_RE = re.compile(
    r'^C\s+\{([^}]+)\}\s+([\d.e+\-]+)\s+([\d.e+\-]+)\s+(\d+)\s+(\d+)\s+\{([^}]*)\}',
    re.MULTILINE | re.DOTALL,
)
_WIRE_RE = re.compile(
    r'^N\s+([\d.e+\-]+)\s+([\d.e+\-]+)\s+([\d.e+\-]+)\s+([\d.e+\-]+)\s+\{([^}]*)\}',
    re.MULTILINE,
)
_ATTR_RE = re.compile(r'(\w+)=([^\s}\n\\]+)')


def _parse_attrs(raw: str) -> dict:
    return {k: v for k, v in _ATTR_RE.findall(raw)}


def detect_format(content: bytes) -> str:
    """Detecta el formato del archivo por su header."""
    header = content[:120].decode("utf-8", errors="ignore")
    if "xschem version=" in header:
        return "xschem"
    if "<Qucs Schematic" in header:
        return "qucs"
    if "EESchema Schematic File Version" in header:
        return "kicad_legacy"
    return "unknown"


def parse(content: bytes) -> Schematic:
    """Parsea un .sch de Xschem y retorna un Schematic."""
    sch = Schematic()
    text = content.decode("utf-8", errors="ignore")

    for m in _COMPONENT_RE.finditer(text):
        symbol, x, y, rot, mir, attrs_raw = m.groups()
        attrs = _parse_attrs(attrs_raw)
        name = attrs.get("name")
        if name:
            sch.components[name] = Component(
                name=name,
                symbol=symbol.strip(),
                params={k: v for k, v in attrs.items() if k != "name"},
                x=float(x),
                y=float(y),
                rotation=int(rot),
                mirror=int(mir),
            )

    for m in _WIRE_RE.finditer(text):
        x1, y1, x2, y2, attrs_raw = m.groups()
        attrs = _parse_attrs(attrs_raw)
        label = attrs.get("lab", "")
        wire = Wire(float(x1), float(y1), float(x2), float(y2), label)
        sch.wires.append(wire)
        if label:
            sch.nets.add(label)

    return sch
