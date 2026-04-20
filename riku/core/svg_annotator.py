"""
Anotador de SVGs de Xschem.

Flujo:
1. Calcular transform .sch -> SVG:
   a. Si existe origins.txt (xorigin, yorigin de Xschem), calcular mooz por
      minimos cuadrados sobre eje X fijando los origins. Precision ~0px en wires.
   b. Fallback: fit libre por minimos cuadrados con eliminacion de outliers.
2. Dibujar bounding boxes sobre componentes cambiados.
3. Dibujar trayectos de wires para nets añadidas/eliminadas.
"""
import re
import statistics
from dataclasses import dataclass
from pathlib import Path

from riku.core.models import Schematic, DiffReport

_TEXT_RE = re.compile(
    r'<text[^>]+transform="translate\(([0-9.\-]+),\s*([0-9.\-]+)\)"[^>]*>\s*([^<]+?)\s*</text>'
)

_PATH_RE = re.compile(r'M([\d.\-]+) ([\d.\-]+)L([\d.\-]+) ([\d.\-]+)')

_COMPONENT_NAME_COLOR = "#cccccc"

# Offset tipografico en Y cuando se usa fit libre (minimos cuadrados sin origins exactos).
# Los textos #cccccc tienen su baseline ~11px por debajo del anclaje real del componente.
_TEXT_Y_OFFSET = 11.0


@dataclass
class _Transform:
    mooz: float
    offset_x: float
    offset_y: float
    exact: bool = False  # True si viene de origins.txt, False si es fit inferido

    def to_svg(self, sch_x: float, sch_y: float) -> tuple[float, float]:
        return (
            sch_x * self.mooz + self.offset_x,
            sch_y * self.mooz + self.offset_y,
        )


def _extract_name_positions(svg_content: str) -> dict[str, tuple[float, float]]:
    """Extrae {nombre: (svg_x, svg_y)} de los textos de nombres de componentes (#cccccc)."""
    positions: dict[str, tuple[float, float]] = {}
    for m in _TEXT_RE.finditer(svg_content):
        x, y, text = float(m.group(1)), float(m.group(2)), m.group(3).strip()
        if _COMPONENT_NAME_COLOR in m.group(0):
            positions[text] = (x, y)
    return positions


def _lstsq_free(pairs: list) -> "_Transform | None":
    """Minimos cuadrados libre: estima mooz, offset_x, offset_y sin restricciones."""
    n = len(pairs)
    sum_sx   = sum(p[0] for p in pairs)
    sum_sy   = sum(p[1] for p in pairs)
    sum_vx   = sum(p[2] for p in pairs)
    sum_vy   = sum(p[3] for p in pairs)
    sum_sx2  = sum(p[0] ** 2 for p in pairs)
    sum_sy2  = sum(p[1] ** 2 for p in pairs)
    sum_sxvx = sum(p[0] * p[2] for p in pairs)
    sum_syvy = sum(p[1] * p[3] for p in pairs)

    denom_x = n * sum_sx2 - sum_sx ** 2
    denom_y = n * sum_sy2 - sum_sy ** 2

    if abs(denom_x) < 1e-9 or abs(denom_y) < 1e-9:
        return None

    mooz_x = (n * sum_sxvx - sum_sx * sum_vx) / denom_x
    mooz_y = (n * sum_syvy - sum_sy * sum_vy) / denom_y
    mooz = (mooz_x + mooz_y) / 2

    return _Transform(
        mooz=mooz,
        offset_x=(sum_vx - mooz * sum_sx) / n,
        offset_y=(sum_vy - mooz * sum_sy) / n,
        exact=False,
    )


def _extract_wire_endpoints(svg_content: str) -> set[tuple[float, float]]:
    """Extrae todos los endpoints de paths SVG tipo 'M x yL x y' (wires de Xschem)."""
    pts: set[tuple[float, float]] = set()
    for m in _PATH_RE.finditer(svg_content):
        pts.add((float(m.group(1)), float(m.group(2))))
        pts.add((float(m.group(3)), float(m.group(4))))
    return pts


def _lstsq_fixed_origins(
    pairs: list,
    xorigin: float,
    yorigin: float,
    svg_wire_pts: "set[tuple[float,float]] | None" = None,
    sch_wire_pts: "set[tuple[float,float]] | None" = None,
) -> "_Transform | None":
    """
    Minimos cuadrados con xorigin/yorigin fijos (de Xschem TCL).
    Formula: svg = (sch + origin) * mooz  =>  mooz = mean(svg / (sch + origin))

    Si se proveen svg_wire_pts y sch_wire_pts, calibra mooz desde endpoints de wires
    (mas preciso que textos de nombres, que tienen offset tipografico).
    Fallback: eje X de los textos.
    """
    # --- calibracion desde wire endpoints (maxima precision) ---
    if svg_wire_pts and sch_wire_pts:
        mooz_approx_x = []
        for sch_x, sch_y, svg_x, svg_y in pairs:
            d = sch_x + xorigin
            if abs(d) > 1e-6:
                mooz_approx_x.append(svg_x / d)
        if mooz_approx_x:
            mooz_approx = statistics.mean(mooz_approx_x)
            wire_pairs = []
            for sx, sy in sch_wire_pts:
                px = (sx + xorigin) * mooz_approx
                py = (sy + yorigin) * mooz_approx
                best = min(svg_wire_pts, key=lambda p: (p[0] - px) ** 2 + (p[1] - py) ** 2)
                dist = ((best[0] - px) ** 2 + (best[1] - py) ** 2) ** 0.5
                if dist < 8.0:
                    wire_pairs.append((sx, sy, best[0], best[1]))
            if len(wire_pairs) >= 4:
                mooz_wx = [svgx / (schx + xorigin) for schx, _, svgx, _ in wire_pairs if abs(schx + xorigin) > 1e-6]
                mooz_wy = [svgy / (schy + yorigin) for _, schy, _, svgy in wire_pairs if abs(schy + yorigin) > 1e-6]
                if mooz_wx and mooz_wy:
                    # Descartar outliers > 2*sigma
                    for samples in (mooz_wx, mooz_wy):
                        if len(samples) >= 4:
                            mean_m = statistics.mean(samples)
                            std_m = statistics.pstdev(samples)
                            samples[:] = [m for m in samples if abs(m - mean_m) <= 2 * std_m]
                    if mooz_wx and mooz_wy:
                        mooz = (statistics.mean(mooz_wx) + statistics.mean(mooz_wy)) / 2
                        return _Transform(
                            mooz=mooz,
                            offset_x=xorigin * mooz,
                            offset_y=yorigin * mooz,
                            exact=True,
                        )

    # --- fallback: eje X de textos de nombres ---
    mooz_samples = []
    for sch_x, sch_y, svg_x, svg_y in pairs:
        denom = sch_x + xorigin
        if abs(denom) > 1e-6:
            mooz_samples.append(svg_x / denom)

    if not mooz_samples:
        return None

    if len(mooz_samples) >= 4:
        mean_m = statistics.mean(mooz_samples)
        std_m  = statistics.pstdev(mooz_samples)
        mooz_samples = [m for m in mooz_samples if abs(m - mean_m) <= 2 * std_m]

    if not mooz_samples:
        return None

    mooz = statistics.mean(mooz_samples)
    return _Transform(
        mooz=mooz,
        offset_x=xorigin * mooz,
        offset_y=yorigin * mooz,
        exact=True,
    )


def _fit_transform(
    svg_positions: dict[str, tuple[float, float]],
    schematic: Schematic,
    svg_path: Path | None = None,
    svg_content: str | None = None,
) -> "_Transform | None":
    """
    Calcula el transform .sch -> SVG.
    Si svg_path tiene origins.txt, usa origins exactos + mooz calibrado desde wire endpoints.
    Fallback: fit libre con eliminacion de outliers.
    """
    pairs = [
        (schematic.components[name].x, schematic.components[name].y, svg_x, svg_y)
        for name, (svg_x, svg_y) in svg_positions.items()
        if name in schematic.components
    ]

    if len(pairs) < 2:
        return None

    # --- intentar con origins exactos ---
    if svg_path is not None:
        origins_file = svg_path.parent / "origins.txt"
        if origins_file.exists():
            try:
                lines = origins_file.read_text().strip().splitlines()
                xorigin, yorigin = float(lines[0]), float(lines[1])
                svg_wire_pts = _extract_wire_endpoints(svg_content) if svg_content else None
                sch_wire_pts = (
                    {(w.x1, w.y1) for w in schematic.wires} | {(w.x2, w.y2) for w in schematic.wires}
                ) if schematic.wires else None
                t = _lstsq_fixed_origins(pairs, xorigin, yorigin, svg_wire_pts, sch_wire_pts)
                if t is not None:
                    return t
            except Exception:
                pass

    # --- fallback: fit libre con eliminacion de outliers ---
    t = _lstsq_free(pairs)
    if t is None or len(pairs) < 4:
        return t

    residuals = [(t.to_svg(p[0], p[1])[0] - p[2]) ** 2 + (t.to_svg(p[0], p[1])[1] - p[3]) ** 2
                 for p in pairs]
    mean_r = statistics.mean(residuals)
    std_r  = statistics.pstdev(residuals)
    threshold = mean_r + 2 * std_r
    inliers = [p for p, r in zip(pairs, residuals) if r <= threshold]

    return _lstsq_free(inliers) if len(inliers) >= 2 else t


_BBOX_HALF = 15  # mitad del lado del bounding box en unidades .sch

_COLORS = {
    "added":    ("rgba(0,200,0,0.25)",   "rgba(0,200,0,0.8)"),
    "removed":  ("rgba(200,0,0,0.25)",   "rgba(200,0,0,0.8)"),
    "modified": ("rgba(255,180,0,0.25)", "rgba(255,180,0,0.8)"),
}

_WIRE_STROKE = {
    "added":   "rgba(0,200,0,0.9)",
    "removed": "rgba(200,0,0,0.9)",
}
_WIRE_WIDTH = 2.5


def _wire_elements(wires, net_names: set[str], kind: str, transform: _Transform) -> list[str]:
    stroke = _WIRE_STROKE[kind]
    elements = []
    for w in wires:
        if w.label not in net_names:
            continue
        x1, y1 = transform.to_svg(w.x1, w.y1)
        x2, y2 = transform.to_svg(w.x2, w.y2)
        elements.append(
            f'<line x1="{x1:.2f}" y1="{y1:.2f}" x2="{x2:.2f}" y2="{y2:.2f}" '
            f'stroke="{stroke}" stroke-width="{_WIRE_WIDTH}" stroke-linecap="round"/>'
        )
    return elements


def annotate(
    svg_content: str,
    sch_b: Schematic,
    diff_report: DiffReport,
    sch_a: Schematic | None = None,
    svg_path: Path | None = None,
) -> str:
    """
    Retorna el SVG anotado con bounding boxes sobre componentes cambiados
    y trayectos de wires para nets añadidas (verde) / eliminadas (rojo).

    svg_path: ruta al SVG en cache — permite leer origins.txt para precision exacta.
    sch_a: necesario para dibujar wires de nets eliminadas.
    """
    svg_positions = _extract_name_positions(svg_content)
    transform = _fit_transform(svg_positions, sch_b, svg_path=svg_path, svg_content=svg_content)
    if transform is None:
        return svg_content

    elements = []
    half = _BBOX_HALF * transform.mooz

    # --- bounding boxes de componentes ---
    # El anchor mas preciso para el box es la posicion del texto del nombre
    # en el SVG (#cccccc) — es exactamente donde Xschem lo dibuja.
    # Fallback al transform cuando el nombre no aparece en el SVG
    # (componentes removed que solo existen en sch_a).
    for cd in diff_report.components:
        source = (sch_a if sch_a is not None else sch_b) if cd.kind == "removed" else sch_b
        if cd.name not in source.components:
            continue

        if cd.name in svg_positions:
            cx, cy = svg_positions[cd.name]
        else:
            comp = source.components[cd.name]
            cx, cy = transform.to_svg(comp.x, comp.y)

        fill, stroke = _COLORS.get(cd.kind, _COLORS["modified"])
        elements.append(
            f'<rect x="{cx - half:.2f}" y="{cy - half:.2f}" '
            f'width="{2*half:.2f}" height="{2*half:.2f}" '
            f'fill="{fill}" stroke="{stroke}" stroke-width="1.5" rx="3" ry="3"/>'
        )

    # --- wires de nets añadidas ---
    if diff_report.nets_added:
        elements.extend(_wire_elements(
            sch_b.wires, set(diff_report.nets_added), "added", transform
        ))

    # --- wires de nets eliminadas ---
    if diff_report.nets_removed and sch_a is not None:
        elements.extend(_wire_elements(
            sch_a.wires, set(diff_report.nets_removed), "removed", transform
        ))

    if not elements:
        return svg_content

    annotation_layer = (
        '\n<g id="riku-diff-annotations">\n'
        + "\n".join(elements)
        + "\n</g>\n"
    )
    return svg_content.replace("</svg>", annotation_layer + "</svg>")
