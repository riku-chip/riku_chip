//! Transistores de una celda aplanada: compuertas, tipo, W y L.
//!
//! 1. **Compuertas:** difusión ∩ poly (las dos capas de la regla de cada
//!    tipo, ver [`DeviceRules::gate_layers`]); cada región es un finger.
//! 2. **Tipo:** la regla del `.tech` evaluada en un punto interior.
//! 3. **W y L,** como el extractor de KLayout (`dbNetlistDeviceExtractorClasses.cc`):
//!    fuente y drenaje son las dos regiones de difusión fuera de la
//!    compuerta que la tocan; `W` es el promedio de sus bordes con la
//!    compuerta (la suma / 2) y `L = área / W`. Exacto en un rectángulo y la
//!    convención estándar en una compuerta doblada. Como en KLayout, una
//!    compuerta que no toca exactamente dos regiones (un roce de poly en la
//!    esquina de una difusión) no es un transistor.

use std::collections::{BTreeSet, HashMap};

use gdstk_rs::{boolean_owned, BoolOp, GdsTag, OwnedPolygon, Point2D};

use super::regions::RegionEval;
use super::rules::{DeviceRules, GdsLayer};

/// Un transistor (un finger).
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// Modelo SPICE (`sky130_fd_pr__nfet_01v8`).
    pub model: String,
    /// Tipo de Magic (`nfet`).
    pub magic: String,
    /// La compuerta, en unidades de la librería.
    pub gate: OwnedPolygon,
    /// Un punto dentro de la compuerta, en unidades de la librería.
    pub at: (f64, f64),
    pub w_um: f64,
    pub l_um: f64,
    /// Un punto en cada una de las dos regiones de fuente y drenaje, justo
    /// afuera de la compuerta (unidades de la librería): de ahí salen sus
    /// redes.
    pub sd_at: Vec<(f64, f64)>,
}

/// Los polígonos aplanados de una celda, por capa GDS, con un índice para
/// preguntar qué capas hay en un punto.
#[derive(Default)]
pub struct LayerPolys {
    by_tag: HashMap<(u32, u32), Grid>,
}

impl LayerPolys {
    pub fn new(polys: impl IntoIterator<Item = OwnedPolygon>) -> Self {
        let mut groups: HashMap<(u32, u32), Vec<OwnedPolygon>> = HashMap::new();
        for p in polys {
            groups.entry((p.layer, p.datatype)).or_default().push(p);
        }
        Self { by_tag: groups.into_iter().map(|(k, v)| (k, Grid::new(v))).collect() }
    }

    fn matching(&self, (l, d): GdsLayer) -> impl Iterator<Item = &Grid> {
        self.by_tag.iter().filter(move |((tl, td), _)| *tl == l && d.is_none_or(|d| d == *td)).map(|(_, g)| g)
    }

    /// Los polígonos de esas capas.
    pub(crate) fn polys(&self, layers: &[GdsLayer]) -> Vec<OwnedPolygon> {
        layers.iter().flat_map(|&gl| self.matching(gl)).flat_map(|g| g.polys.iter().cloned()).collect()
    }

    /// Hay polígonos en la capa GDS `gl`.
    pub(crate) fn has(&self, gl: GdsLayer) -> bool {
        self.matching(gl).any(|g| !g.polys.is_empty())
    }

    /// El punto está en alguna capa GDS `gl`.
    pub fn contains(&self, gl: GdsLayer, x: f64, y: f64) -> bool {
        self.matching(gl).any(|g| g.contains(x, y))
    }
}

/// Polígonos con sus cajas en una grilla uniforme: un punto mira solo los
/// de su casilla.
pub(crate) struct Grid {
    pub(crate) polys: Vec<OwnedPolygon>,
    boxes: Vec<[f64; 4]>,
    origin: (f64, f64),
    cell: f64,
    cells: HashMap<(i64, i64), Vec<u32>>,
}

impl Grid {
    pub(crate) fn new(polys: Vec<OwnedPolygon>) -> Self {
        let boxes: Vec<[f64; 4]> = polys.iter().map(bbox).collect();
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for b in &boxes {
            (x0, y0, x1, y1) = (x0.min(b[0]), y0.min(b[1]), x1.max(b[2]), y1.max(b[3]));
        }
        // Del orden de la raíz de la cantidad de casillas por lado.
        let side = ((polys.len() as f64).sqrt().ceil()).clamp(1.0, 512.0);
        let cell = ((x1 - x0).max(y1 - y0) / side).max(1e-9);
        let mut cells: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
        for (i, b) in boxes.iter().enumerate() {
            let (cx0, cy0) = (((b[0] - x0) / cell).floor() as i64, ((b[1] - y0) / cell).floor() as i64);
            let (cx1, cy1) = (((b[2] - x0) / cell).floor() as i64, ((b[3] - y0) / cell).floor() as i64);
            for cx in cx0..=cx1 {
                for cy in cy0..=cy1 {
                    cells.entry((cx, cy)).or_default().push(i as u32);
                }
            }
        }
        Self { polys, boxes, origin: (x0, y0), cell, cells }
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        self.find(x, y).is_some()
    }

    /// El índice de un polígono que contiene el punto.
    pub(crate) fn find(&self, x: f64, y: f64) -> Option<u32> {
        let key = (((x - self.origin.0) / self.cell).floor() as i64, ((y - self.origin.1) / self.cell).floor() as i64);
        self.cells.get(&key)?.iter().copied().find(|&i| {
            let b = self.boxes[i as usize];
            x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3] && point_in(&self.polys[i as usize].points, x, y)
        })
    }
}

/// Los transistores de una celda. `unit_um`: µm por unidad de la librería.
pub fn extract(rules: &DeviceRules, layers: &LayerPolys, unit_um: f64) -> Vec<Device> {
    // Un par (difusión, poly) por tipo; casi siempre el mismo para todos.
    let pairs: BTreeSet<(Vec<GdsLayer>, Vec<GdsLayer>)> = rules.devices.iter().filter_map(|(d, _)| rules.gate_layers(*d)).collect();
    let tag = GdsTag { layer: 0, datatype: 0 };
    let eps = 1e-4 / unit_um; // 0,1 nm
    let mut out: Vec<Device> = Vec::new();
    let mut sized: HashMap<usize, Grid> = HashMap::new();
    for (active, gate) in &pairs {
        let (a, g) = (layers.polys(active), layers.polys(gate));
        if a.is_empty() || g.is_empty() {
            continue;
        }
        // Clipper puede devolver una compuerta partida en pedazos que comparten
        // un borde (dos difusiones que se solapan): la unión los junta, y así
        // W y L son los de la compuerta entera.
        let Ok(gates) = boolean_owned(&a, &g, BoolOp::And, tag).and_then(|g| boolean_owned(&g, &[], BoolOp::Or, tag)) else { continue };
        // Fuente y drenaje: la difusión fuera del poly, cada región por separado.
        let Ok(sd) = boolean_owned(&a, &g, BoolOp::Not, tag) else { continue };
        let sd = Grid::new(sd);
        for poly in gates {
            let Some(at) = interior_point(&poly.points) else { continue };
            // El último tipo que incluye el punto; si su regla cambia de
            // tamaño, se confirma con su región (ver `DeviceRules::resizes`).
            let candidates = rules.devices_at(&|gl| layers.contains(gl, at.0, at.1));
            let Some(kind) = candidates
                .iter()
                .rev()
                .find(|(d, _)| {
                    !rules.resizes(*d)
                        || sized.entry(*d).or_insert_with(|| Grid::new(RegionEval::new(rules, layers, unit_um).def_region(*d))).contains(at.0, at.1)
                })
                .map(|(_, t)| *t)
            else {
                continue;
            };
            // Mismo finger por otro par de capas: ya está.
            if out.iter().any(|d| point_in(&d.gate.points, at.0, at.1)) {
                continue;
            }
            let Some((w_um, l_um, sd_at)) = measure(&poly.points, &sd, eps, unit_um) else { continue };
            out.push(Device {
                model: kind.model(w_um, l_um).to_string(),
                magic: kind.magic.clone(),
                gate: poly,
                at,
                w_um,
                l_um,
                sd_at,
            });
        }
    }
    out.sort_by(|a, b| (a.at.1, a.at.0).partial_cmp(&(b.at.1, b.at.0)).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Transistores de un layout de Magic, donde ya vienen pintados como capas
/// (`nfet`, o un alias: `scnmos`). `names`: el tipo de Magic de cada capa.
pub fn extract_magic(rules: &DeviceRules, polys: Vec<OwnedPolygon>, names: &HashMap<(u32, u32), String>, unit_um: f64) -> Vec<Device> {
    let tag = GdsTag { layer: 0, datatype: 0 };
    let eps = 1e-4 / unit_um;
    let name_of = |p: &OwnedPolygon| names.get(&(p.layer, p.datatype)).map(String::as_str);
    let mut out = Vec::new();
    // Un tipo puede tener más de una `layer` en `cifinput` (`scpfethvt` en
    // SKY130): cada uno, una vez.
    let mut seen: Vec<&str> = Vec::new();
    for (_, kind) in &rules.devices {
        let c = rules.canonical(&kind.magic);
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        let gates: Vec<OwnedPolygon> = polys.iter().filter(|p| name_of(p).is_some_and(|n| rules.canonical(n) == rules.canonical(&kind.magic))).cloned().collect();
        if gates.is_empty() {
            continue;
        }
        let sd: Vec<OwnedPolygon> = polys.iter().filter(|p| name_of(p).is_some_and(|n| rules.is_sd_of(kind, n))).cloned().collect();
        let (Ok(gates), Ok(sd)) = (boolean_owned(&gates, &[], BoolOp::Or, tag), boolean_owned(&sd, &[], BoolOp::Or, tag)) else { continue };
        let sd = Grid::new(sd);
        for poly in gates {
            let (Some(at), Some((w_um, l_um, sd_at))) = (interior_point(&poly.points), measure(&poly.points, &sd, eps, unit_um)) else { continue };
            out.push(Device { model: kind.model(w_um, l_um).to_string(), magic: kind.magic.clone(), gate: poly, at, w_um, l_um, sd_at });
        }
    }
    out.sort_by(|a, b| (a.at.1, a.at.0).partial_cmp(&(b.at.1, b.at.0)).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// W y L (µm) de una compuerta: sus bordes con fuente y drenaje (dos
/// regiones de `sd`, ni una ni tres, como en KLayout); y un punto en cada una.
fn measure(gate: &[Point2D], sd: &Grid, eps: f64, unit_um: f64) -> Option<(f64, f64, Vec<(f64, f64)>)> {
    let (mut len, mut regions, mut points) = (0.0, Vec::new(), Vec::new());
    for (edge, (x, y)) in outward_edges(gate, eps) {
        if let Some(r) = sd.find(x, y) {
            len += edge;
            if !regions.contains(&r) {
                regions.push(r);
                points.push((x, y));
            }
        }
    }
    if regions.len() != 2 || len <= 0.0 {
        return None;
    }
    let w = len / 2.0;
    Some((w * unit_um, area(gate) / w * unit_um, points))
}

pub(crate) fn bbox(p: &OwnedPolygon) -> [f64; 4] {
    p.points.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, q| {
        [b[0].min(q.x), b[1].min(q.y), b[2].max(q.x), b[3].max(q.y)]
    })
}

fn signed_area(pts: &[Point2D]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y).sum::<f64>() / 2.0
}

pub(crate) fn area(pts: &[Point2D]) -> f64 {
    signed_area(pts).abs()
}

/// Par e impar: sirve también para los polígonos con agujeros que devuelve
/// gdstk (unidos al borde por un corte que se recorre de ida y de vuelta).
pub(crate) fn point_in(pts: &[Point2D], x: f64, y: f64) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (pts[i], pts[j]);
        if (a.y > y) != (b.y > y) && x < (b.x - a.x) * (y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Un punto dentro del polígono: el medio del tramo interior más ancho de
/// una horizontal cerca de la mitad de su alto (no justo en la mitad, para
/// no caer sobre un vértice de la grilla).
pub(crate) fn interior_point(pts: &[Point2D]) -> Option<(f64, f64)> {
    let (y0, y1) = pts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.y), b.max(p.y)));
    if !(y1 > y0) {
        return None;
    }
    let y = y0 + (y1 - y0) * 0.5037;
    let n = pts.len();
    let mut xs: Vec<f64> = (0..n)
        .filter_map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            ((a.y > y) != (b.y > y)).then(|| a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y))
        })
        .collect();
    xs.sort_by(f64::total_cmp);
    xs.chunks_exact(2).max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0]))).map(|c| ((c[0] + c[1]) / 2.0, y))
}

/// Cada borde: su largo y un punto a `eps` hacia afuera desde su medio.
pub(crate) fn outward_edges(pts: &[Point2D], eps: f64) -> impl Iterator<Item = (f64, (f64, f64))> + '_ {
    let ccw = signed_area(pts) > 0.0;
    let n = pts.len();
    (0..n).filter_map(move |i| {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 0.0 {
            return None;
        }
        // Normal hacia afuera: a la derecha del avance en antihorario.
        let (nx, ny) = if ccw { (dy / len, -dx / len) } else { (-dy / len, dx / len) };
        Some((len, ((a.x + b.x) / 2.0 + nx * eps, (a.y + b.y) / 2.0 + ny * eps)))
    })
}

#[cfg(test)]
mod tests {
    use super::super::rules::tests::TECH;
    use super::*;

    fn rect(tag: (u32, u32), x0: f64, y0: f64, x1: f64, y1: f64) -> OwnedPolygon {
        let p = |x, y| Point2D { x, y };
        OwnedPolygon { layer: tag.0, datatype: tag.1, points: vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)] }
    }

    const DIFF: (u32, u32) = (65, 20);
    const POLY: (u32, u32) = (66, 20);
    const NSDM: (u32, u32) = (93, 44);
    const NWELL: (u32, u32) = (64, 20);

    #[test]
    fn an_inverter_has_one_n_and_one_p_with_their_w_and_l() {
        let rules = DeviceRules::parse(TECH).unwrap();
        // Poly vertical de 0,15 que cruza una difusión N (W 0,65) y una P (W 1,0).
        let layers = LayerPolys::new([
            rect(POLY, 1.0, -0.2, 1.15, 3.2),
            rect(DIFF, 0.5, 0.0, 1.65, 0.65),
            rect(NSDM, 0.3, -0.1, 1.9, 0.8),
            rect(DIFF, 0.5, 2.0, 1.65, 3.0),
            rect(NWELL, 0.0, 1.8, 2.2, 3.4),
        ]);
        let devs = extract(&rules, &layers, 1.0);
        let got: Vec<(&str, String, String)> =
            devs.iter().map(|d| (d.model.as_str(), format!("{:.3}", d.w_um), format!("{:.3}", d.l_um))).collect();
        assert_eq!(
            got,
            [("mini__nfet", "0.650".into(), "0.150".into()), ("mini__pfet", "1.000".into(), "0.150".into())]
        );
    }

    #[test]
    fn two_fingers_are_two_devices_and_units_scale() {
        let rules = DeviceRules::parse(TECH).unwrap();
        // En nm (unidad 1e-9 m): dos polys sobre la misma difusión.
        let layers = LayerPolys::new([
            rect(DIFF, 0.0, 0.0, 2000.0, 420.0),
            rect(NSDM, -100.0, -100.0, 2100.0, 520.0),
            rect(POLY, 500.0, -200.0, 650.0, 620.0),
            rect(POLY, 1300.0, -200.0, 1450.0, 620.0),
        ]);
        let devs = extract(&rules, &layers, 1e-3);
        assert_eq!(devs.len(), 2);
        for d in &devs {
            assert!((d.w_um - 0.42).abs() < 1e-9 && (d.l_um - 0.15).abs() < 1e-9, "{d:?}");
        }
    }

    #[test]
    fn poly_over_diffusion_without_a_matching_rule_is_not_a_device() {
        let rules = DeviceRules::parse(TECH).unwrap();
        // Sin implante ni pozo: ninguna regla lo acepta.
        let layers = LayerPolys::new([rect(DIFF, 0.0, 0.0, 2.0, 0.5), rect(POLY, 0.9, -0.2, 1.05, 0.7)]);
        assert!(extract(&rules, &layers, 1.0).is_empty());
        // Un poly que no toca la difusión no es compuerta.
        let layers = LayerPolys::new([rect(DIFF, 0.0, 0.0, 2.0, 0.5), rect(NSDM, -1.0, -1.0, 3.0, 2.0), rect(POLY, 3.0, 0.0, 3.2, 0.5)]);
        assert!(extract(&rules, &layers, 1.0).is_empty());
    }

    #[test]
    fn a_gate_split_in_two_pieces_is_one_device() {
        let rules = DeviceRules::parse(TECH).unwrap();
        // Dos difusiones que se solapan (como en fahcin_1 de SKY130): Clipper
        // devolvería la compuerta en dos rectángulos.
        let layers = LayerPolys::new([
            rect(DIFF, 0.0, 0.0, 2.0, 0.675),
            rect(DIFF, 0.5, 0.6, 1.5, 0.84),
            rect(NSDM, -1.0, -1.0, 3.0, 2.0),
            rect(POLY, 0.9, -0.2, 1.05, 1.0),
        ]);
        let devs = extract(&rules, &layers, 1.0);
        assert_eq!(devs.len(), 1, "{devs:?}");
        assert!((devs[0].w_um - 0.84).abs() < 1e-9, "{:?}", devs[0]);
    }

    #[test]
    fn a_poly_grazing_a_diffusion_corner_is_not_a_device() {
        let rules = DeviceRules::parse(TECH).unwrap();
        // El poly pisa la esquina de la difusión: la compuerta toca una sola
        // región de difusión (en KLayout, "expected two polygons").
        let layers = LayerPolys::new([
            rect(DIFF, 0.0, 0.0, 2.0, 0.5),
            rect(NSDM, -1.0, -1.0, 3.0, 2.0),
            rect(POLY, 1.95, 0.45, 2.5, 1.0),
        ]);
        assert!(extract(&rules, &layers, 1.0).is_empty());
    }

    #[test]
    fn a_magic_layout_uses_its_painted_transistors() {
        let rules = DeviceRules::parse(TECH).unwrap();
        // Tags cualesquiera: lo que cuenta es el nombre de Magic de cada capa.
        let names: HashMap<(u32, u32), String> =
            [((1, 0), "nmos"), ((2, 0), "ndiff"), ((3, 0), "ndiffc"), ((4, 0), "pdiff")].into_iter().map(|(t, n)| (t, n.to_string())).collect();
        let polys = vec![
            rect((2, 0), 0.0, 0.0, 0.5, 0.65),  // fuente
            rect((1, 0), 0.5, 0.0, 0.65, 0.65), // compuerta
            rect((3, 0), 0.65, 0.0, 1.0, 0.65), // drenaje, en contacto
            rect((4, 0), 2.0, 0.0, 3.0, 1.0),   // otra difusión, lejos
        ];
        let devs = extract_magic(&rules, polys, &names, 1.0);
        assert_eq!(devs.len(), 1, "{devs:?}");
        assert_eq!(devs[0].model, "mini__nfet");
        assert!((devs[0].w_um - 0.65).abs() < 1e-9 && (devs[0].l_um - 0.15).abs() < 1e-9, "{:?}", devs[0]);
    }

    #[test]
    fn helpers() {
        let p = |x, y| Point2D { x, y };
        let l = [p(0.0, 0.0), p(4.0, 0.0), p(4.0, 1.0), p(1.0, 1.0), p(1.0, 4.0), p(0.0, 4.0)];
        let (x, y) = interior_point(&l).unwrap();
        assert!(point_in(&l, x, y));
        assert!(!point_in(&l, 3.0, 3.0));
        assert!((area(&l) - 7.0).abs() < 1e-12);
    }
}
