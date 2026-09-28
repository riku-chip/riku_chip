//! Redes de una celda aplanada, a partir de la región de cada tipo de Magic
//! (ver `docs/formatos.md`, «Transistores y redes»):
//!
//! 1. **Pedazos:** cada polígono de la unión de un tipo.
//! 2. **Conexiones:** dos pedazos de tipos que `connect` une (en Magic,
//!    `*li` incluye los contactos que tocan `li`) se unen si se tocan o se
//!    superponen: se unen las regiones de los dos tipos y los pedazos que caen
//!    en el mismo polígono de la unión son la misma red (*union-find*). La
//!    unión se hace con todo agrandado medio nanómetro: Clipper puede devolver
//!    separados dos polígonos que solo comparten un borde.
//! 3. **Sustrato:** los pedazos de los tipos de `substrate` (`pwell`, las
//!    tomas `psd`) que no están en uno que lo excluye (`dnwell`) son una sola
//!    red, aunque el sustrato no esté dibujado.
//! 4. **Terminales:** la compuerta de un transistor es el pedazo de su tipo
//!    (`nfet`, que `connect` une al poly); fuente y drenaje, los pedazos de
//!    difusión en los dos puntos que encuentra el nivel 2; el cuerpo, el
//!    pozo que lo contiene o el sustrato.
//! 5. **Nombres:** una etiqueta sobre un pedazo de su tipo nombra su red.

use std::collections::{BTreeSet, HashMap, HashSet};

use gdstk_rs::{boolean_owned, offset_owned, BoolOp, GdsTag, OwnedPolygon};

use crate::devices::extract::{area, bbox, interior_point, outward_edges, Grid};
use crate::devices::{Device, DeviceRules};

/// Una etiqueta que puede nombrar una red.
#[derive(Clone, Debug, PartialEq)]
pub struct NetLabel {
    pub text: String,
    /// Unidades de la librería.
    pub at: (f64, f64),
    /// Los tipos (canónicos) sobre los que puede estar.
    pub types: Vec<String>,
    /// Es un pin de la celda.
    pub port: bool,
}

/// Una red.
#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    /// El nombre de su etiqueta (la de un pin, si tiene) o `None`.
    pub name: Option<String>,
    /// Todos los textos de sus etiquetas, sin repetir (más de uno es un aviso).
    pub labels: Vec<String>,
    /// Tiene una etiqueta de pin.
    pub port: bool,
    /// Es el sustrato.
    pub substrate: bool,
    /// Caja de todos sus pedazos, en unidades de la librería.
    pub bbox: [f64; 4],
}

/// Las redes de los cuatro terminales de un transistor (índices en `nets`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Terminals {
    pub d: usize,
    pub g: usize,
    pub s: usize,
    pub b: usize,
}

/// Un resistor: el cuerpo entre sus dos terminales.
#[derive(Clone, Debug, PartialEq)]
pub struct Resistor {
    pub model: String,
    /// Tipo de Magic (`rm5`).
    pub magic: String,
    /// El cuerpo, en unidades de la librería.
    pub body: OwnedPolygon,
    /// Un punto dentro del cuerpo, en unidades de la librería.
    pub at: (f64, f64),
    /// W: el borde con los terminales / 2; L: área / W.
    pub w_um: f64,
    pub l_um: f64,
    pub subckt: bool,
}

/// Un pedazo conductor de una red (para mostrarla y resaltarla).
#[derive(Clone, Debug, PartialEq)]
pub struct NetPiece {
    /// Tipo de Magic (canónico).
    pub magic: String,
    pub poly: OwnedPolygon,
    pub net: usize,
}

/// Las redes de una celda, sus transistores y sus resistores.
#[derive(Clone, Debug, Default)]
pub struct Netlist {
    /// Los pedazos conductores de cada red (los de redes con un terminal o
    /// una etiqueta).
    pub pieces: Vec<NetPiece>,
    pub nets: Vec<Net>,
    pub devices: Vec<(Device, Terminals)>,
    /// Cada resistor y las redes de sus dos terminales.
    pub resistors: Vec<(Resistor, [usize; 2])>,
    /// Las etiquetas de la entrada y la red de cada una (`None` si no cayó
    /// sobre su tipo).
    pub labels: Vec<NetLabel>,
    pub label_nets: Vec<Option<usize>>,
    pub warnings: Vec<String>,
}

const TAG: GdsTag = GdsTag { layer: 0, datatype: 0 };

fn union(polys: &[OwnedPolygon]) -> Vec<OwnedPolygon> {
    if polys.is_empty() {
        return Vec::new();
    }
    boolean_owned(polys, &[], BoolOp::Or, TAG).unwrap_or_default()
}

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn add(&mut self) -> usize {
        self.0.push(self.0.len());
        self.0.len() - 1
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            // El menor queda de raíz: el orden de las redes no depende del orden de las uniones.
            let (lo, hi) = (a.min(b), a.max(b));
            self.0[hi] = lo;
        }
    }
}

/// Los pedazos de cada tipo, con sus nodos en el union-find.
struct Pieces {
    by_type: HashMap<String, (Grid, usize)>,
}

impl Pieces {
    /// El nodo del pedazo de alguno de `types` que contiene el punto.
    fn at<'a>(&self, types: impl IntoIterator<Item = &'a String>, (x, y): (f64, f64)) -> Option<usize> {
        types.into_iter().find_map(|t| self.by_type.get(t).and_then(|(g, first)| g.find(x, y).map(|i| first + i as usize)))
    }
}

/// Las redes de una celda. `regions`: la región de cada tipo (canónico) que
/// conduce, es sustrato o lo excluye; `devices`: los transistores del nivel
/// 2; `unit_um`: µm por unidad de las coordenadas.
pub fn build(rules: &DeviceRules, regions: Vec<(String, Vec<OwnedPolygon>)>, labels: &[NetLabel], devices: Vec<Device>, unit_um: f64) -> Netlist {
    let mut uf = UnionFind(Vec::new());
    let mut pieces = Pieces { by_type: HashMap::new() };
    let mut order: Vec<String> = Vec::new();
    for (t, region) in regions {
        let t = rules.canonical(&t).to_string();
        let p = union(&region);
        if p.is_empty() || pieces.by_type.contains_key(&t) {
            continue;
        }
        let first = uf.0.len();
        for _ in 0..p.len() {
            uf.add();
        }
        order.push(t.clone());
        pieces.by_type.insert(t, (Grid::new(p), first));
    }
    let substrate = uf.add();

    // Conexiones: una unión por grupo de tipos que conducen juntos.
    let present = |ts: &[String]| -> Vec<String> {
        let mut v: Vec<String> = ts.iter().filter(|t| pieces.by_type.contains_key(*t)).cloned().collect();
        v.sort();
        v.dedup();
        v
    };
    let mut groups: BTreeSet<Vec<String>> = BTreeSet::new();
    for (a, b) in &rules.connect {
        let (pa, pb) = (present(a), present(b));
        if pa == pb {
            groups.insert(pa);
        } else {
            for x in &pa {
                for y in pb.iter().filter(|y| *y != x) {
                    let mut g = vec![x.clone(), y.clone()];
                    g.sort();
                    groups.insert(g);
                }
            }
        }
    }
    let touch = 5e-4 / unit_um;
    for g in groups.iter().filter(|g| !g.is_empty()) {
        let all: Vec<OwnedPolygon> = g.iter().flat_map(|t| pieces.by_type[t].0.polys.iter().cloned()).collect();
        let merged = Grid::new(offset_owned(&all, touch, TAG).unwrap_or_else(|_| union(&all)));
        let mut rep: HashMap<u32, usize> = HashMap::new();
        for t in g {
            let (grid, first) = &pieces.by_type[t];
            for (i, p) in grid.polys.iter().enumerate() {
                let Some(pt) = interior_point(&p.points) else { continue };
                let Some(m) = merged.find(pt.0, pt.1) else { continue };
                let node = first + i;
                match rep.get(&m) {
                    Some(&r) => uf.join(r, node),
                    None => {
                        rep.insert(m, node);
                    }
                }
            }
        }
    }

    // Resistores: los de modelo `None` son cortos; los demás, un dispositivo
    // entre los pedazos de sus terminales que tocan el cuerpo.
    let edge_eps = 1e-4 / unit_um;
    let mut resistors: Vec<(Resistor, Vec<usize>)> = Vec::new();
    for rt in &rules.resistors {
        let Some((grid, first)) = pieces.by_type.get(&rt.magic) else { continue };
        for (i, body) in grid.polys.iter().enumerate() {
            let (mut len, mut ends): (f64, Vec<usize>) = (0.0, Vec::new());
            for (edge, pt) in outward_edges(&body.points, edge_eps) {
                if let Some(n) = pieces.at(&rt.terminals, pt) {
                    len += edge;
                    if !ends.iter().any(|&e| uf.find(e) == uf.find(n)) {
                        ends.push(n);
                    }
                }
            }
            match &rt.model {
                None => {
                    for &e in &ends {
                        uf.join(first + i, e);
                    }
                }
                Some(model) => {
                    let Some(at) = interior_point(&body.points) else { continue };
                    let w = len / 2.0;
                    let (w_um, l_um) = if w > 0.0 { (w * unit_um, area(&body.points) / w * unit_um) } else { (0.0, 0.0) };
                    let r = Resistor { model: model.clone(), magic: rt.magic.clone(), body: body.clone(), at, w_um, l_um, subckt: rt.subckt };
                    resistors.push((r, ends));
                }
            }
        }
    }
    // Un resistor con menos de dos terminales: los que falten son redes sueltas.
    for (_, ends) in &mut resistors {
        while ends.len() < 2 {
            ends.push(uf.add());
        }
    }

    // Sustrato: sus tipos, fuera de los que lo excluyen.
    let (sub_types, not_sub) = &rules.substrate;
    for t in sub_types.iter().filter(|t| *t != "space") {
        let Some((grid, first)) = pieces.by_type.get(t) else { continue };
        for (i, p) in grid.polys.iter().enumerate() {
            let excluded = interior_point(&p.points).is_some_and(|pt| pieces.at(not_sub, pt).is_some());
            if !excluded {
                uf.join(substrate, first + i);
            }
        }
    }

    // Terminales.
    let related = |ts: &[String]| -> Vec<String> {
        let mut out: Vec<String> = ts.iter().map(|t| rules.canonical(t).to_string()).collect();
        for (a, b) in &rules.connect {
            if a.iter().chain(b).any(|x| out.contains(x)) {
                for x in a.iter().chain(b) {
                    if !out.contains(x) {
                        out.push(x.clone());
                    }
                }
            }
        }
        out
    };
    let mut terminals: Vec<[usize; 4]> = Vec::new();
    for dev in &devices {
        let kind = rules.device_type(&dev.magic);
        let own = rules.canonical(&dev.magic).to_string();
        let g = pieces
            .at([&own], dev.at)
            .or_else(|| pieces.at(&related(std::slice::from_ref(&own)), dev.at))
            .unwrap_or_else(|| uf.add());
        let sd_types: Vec<String> = kind.map(|k| k.sd.iter().map(|s| rules.canonical(s).to_string()).collect()).unwrap_or_default();
        let sub: Vec<String> = kind.map(|k| k.sub.iter().map(|s| rules.canonical(s).to_string()).collect()).unwrap_or_default();
        let wide: Vec<String> = related(&sd_types).into_iter().filter(|t| !sub.contains(t) && !sub_types.contains(t) && *t != own).collect();
        let mut sd = [0usize; 2];
        for (k, slot) in sd.iter_mut().enumerate() {
            *slot = match dev.sd_at.get(k) {
                Some(&pt) => pieces.at(&sd_types, pt).or_else(|| pieces.at(&wide, pt)).unwrap_or_else(|| uf.add()),
                None => uf.add(),
            };
        }
        let wells: Vec<String> = sub.iter().filter(|t| *t != "space").cloned().collect();
        let b = match pieces.at(&wells, dev.at) {
            Some(n) => n,
            None if sub.is_empty() || sub.iter().any(|t| t == "space") || wells.iter().any(|t| sub_types.contains(t)) => substrate,
            None => uf.add(),
        };
        terminals.push([sd[0], g, sd[1], b]);
    }

    // Etiquetas.
    // Una etiqueta de Magic se ancla en un borde de su rectángulo (`rlabel
    // metal1 … 1` en el borde norte): también se busca a 1 nm alrededor.
    let near = 1e-3 / unit_um;
    let offsets = [(0.0, 0.0), (0.0, -near), (0.0, near), (-near, 0.0), (near, 0.0), (-near, -near), (near, -near), (-near, near), (near, near)];
    let mut label_nodes: Vec<Option<usize>> = Vec::new();
    for l in labels {
        let node = offsets
            .iter()
            .find_map(|(dx, dy)| pieces.at(&l.types, (l.at.0 + dx, l.at.1 + dy)))
            // Una etiqueta del pozo P sin pozo dibujado: el sustrato. Solo por
            // el tipo, no por sus contactos (una toma P también toca `li`).
            .or_else(|| l.types.iter().any(|t| sub_types.contains(t) && rules.contact_residues(t).is_empty()).then_some(substrate));
        label_nodes.push(node);
    }

    // Las redes: las que tienen un terminal o una etiqueta, en orden de nodo.
    let mut used: Vec<usize> = terminals
        .iter()
        .flatten()
        .copied()
        .chain(label_nodes.iter().flatten().copied())
        .chain(resistors.iter().flat_map(|(_, e)| e[..2].to_vec()))
        .map(|n| uf.find(n))
        .collect();
    used.sort_unstable();
    used.dedup();
    let index: HashMap<usize, usize> = used.iter().enumerate().map(|(i, &r)| (r, i)).collect();
    let mut nets: Vec<Net> = used
        .iter()
        .map(|_| Net {
            name: None,
            labels: Vec::new(),
            port: false,
            substrate: false,
            bbox: [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
        })
        .collect();
    if let Some(&i) = index.get(&uf.find(substrate)) {
        nets[i].substrate = true;
    }
    let mut net_pieces = Vec::new();
    for t in &order {
        let (grid, first) = &pieces.by_type[t];
        for (i, p) in grid.polys.iter().enumerate() {
            if let Some(&n) = index.get(&uf.find(first + i)) {
                let b = bbox(p);
                let nb = &mut nets[n].bbox;
                *nb = [nb[0].min(b[0]), nb[1].min(b[1]), nb[2].max(b[2]), nb[3].max(b[3])];
                net_pieces.push(NetPiece { magic: t.clone(), poly: p.clone(), net: n });
            }
        }
    }
    let mut ports: HashSet<(usize, String)> = HashSet::new();
    let label_nets: Vec<Option<usize>> = label_nodes.iter().map(|n| n.map(|n| index[&uf.find(n)])).collect();
    for (l, n) in labels.iter().zip(&label_nets) {
        let Some(n) = *n else { continue };
        if !nets[n].labels.contains(&l.text) {
            nets[n].labels.push(l.text.clone());
        }
        if l.port {
            nets[n].port = true;
            ports.insert((n, l.text.clone()));
        }
    }
    let mut warnings = Vec::new();
    for (i, net) in nets.iter_mut().enumerate() {
        net.labels.sort();
        // El nombre: el primer pin en orden alfabético, o la primera etiqueta.
        net.name = net.labels.iter().find(|t| ports.contains(&(i, (*t).clone()))).or(net.labels.first()).cloned();
        if net.labels.len() > 1 {
            warnings.push(format!("una red tiene más de un nombre: {}", net.labels.join(", ")));
        }
    }
    let mut unplaced: Vec<&str> = labels.iter().zip(&label_nets).filter(|(_, n)| n.is_none()).map(|(l, _)| l.text.as_str()).collect();
    unplaced.sort_unstable();
    unplaced.dedup();
    if !unplaced.is_empty() {
        warnings.push(format!("etiquetas fuera de una capa conductora de su tipo: {}", unplaced.join(", ")));
    }
    let devices = devices
        .into_iter()
        .zip(terminals)
        .map(|(d, [s, g, dr, b])| {
            let mut n = |x: usize| index[&uf.find(x)];
            (d, Terminals { d: n(dr), g: n(g), s: n(s), b: n(b) })
        })
        .collect();
    let resistors = resistors
        .into_iter()
        .map(|(r, e)| {
            let (a, b) = (index[&uf.find(e[0])], index[&uf.find(e[1])]);
            if e.len() > 2 {
                warnings.push(format!("un resistor {} toca {} redes (se toman dos)", r.model, e.len()));
            }
            (r, [a, b])
        })
        .collect();
    Netlist { pieces: net_pieces, nets, devices, resistors, labels: labels.to_vec(), label_nets, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{extract, DeviceRules, LayerPolys, RegionEval};
    use gdstk_rs::Point2D;

    fn rect(tag: (u32, u32), x0: f64, y0: f64, x1: f64, y1: f64) -> OwnedPolygon {
        let p = |x, y| Point2D { x, y };
        OwnedPolygon { layer: tag.0, datatype: tag.1, points: vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)] }
    }

    const DIFF: (u32, u32) = (65, 20);
    const TAP: (u32, u32) = (65, 44);
    const POLY: (u32, u32) = (66, 20);
    const CONT: (u32, u32) = (66, 44);
    const NSDM: (u32, u32) = (93, 44);
    const NWELL: (u32, u32) = (64, 20);
    const LI: (u32, u32) = (67, 20);
    const MCON: (u32, u32) = (67, 44);
    const MET1: (u32, u32) = (68, 20);

    /// Un inversor: poly vertical común (A); la difusión N a la izquierda
    /// va por un contacto y `li` a VGND, la P a VPWR; los drenajes se unen
    /// por `li` en Y. Una toma P a VGND.
    fn inverter(short: bool) -> Vec<OwnedPolygon> {
        let mut v = vec![
            rect(POLY, 1.0, -0.2, 1.15, 3.2),
            rect(DIFF, 0.5, 0.0, 1.65, 0.65),
            rect(NSDM, 0.3, -0.1, 1.9, 0.8),
            rect(DIFF, 0.5, 2.0, 1.65, 3.0),
            rect(NWELL, 0.0, 1.8, 2.2, 3.4),
            // Fuentes (izquierda): contactos a li.
            rect(CONT, 0.6, 0.2, 0.8, 0.4),
            rect(LI, 0.5, 0.1, 0.85, 0.5),
            rect(CONT, 0.6, 2.4, 0.8, 2.6),
            rect(LI, 0.5, 2.3, 0.85, 2.7),
            // Drenajes (derecha): un solo li vertical, Y.
            rect(CONT, 1.3, 0.2, 1.5, 0.4),
            rect(CONT, 1.3, 2.4, 1.5, 2.6),
            rect(LI, 1.25, 0.1, 1.6, 2.7),
            // Entrada: contacto al poly, fuera de la difusión.
            rect(CONT, 1.0, 1.2, 1.15, 1.35),
            rect(LI, 0.95, 1.15, 1.2, 1.4),
            // Toma P a la fuente N por metal1.
            rect(TAP, -1.0, 0.0, -0.6, 0.4),
            rect(CONT, -0.9, 0.1, -0.7, 0.3),
            rect(LI, -1.0, 0.05, -0.6, 0.35),
            rect(MCON, -0.9, 0.1, -0.7, 0.3),
            rect(MCON, 0.6, 0.2, 0.8, 0.4),
            rect(MET1, -1.0, 0.05, 0.85, 0.45),
        ];
        if short {
            // Un li que une la entrada con la salida.
            v.push(rect(LI, 1.1, 1.2, 1.4, 1.3));
        }
        v
    }

    fn netlist(polys: Vec<OwnedPolygon>, labels: &[NetLabel]) -> Netlist {
        let rules = DeviceRules::parse(crate::devices::rules::tests::TECH).unwrap();
        let layers = LayerPolys::new(polys);
        let devices = extract(&rules, &layers, 1.0);
        let mut ev = RegionEval::new(&rules, &layers, 1.0);
        let mut types = rules.conductors();
        types.extend(rules.substrate.0.iter().chain(&rules.substrate.1).cloned());
        let regions = types.iter().map(|t| (t.clone(), ev.type_region(t))).collect();
        build(&rules, regions, labels, devices, 1.0)
    }

    fn label(text: &str, at: (f64, f64), types: &[&str]) -> NetLabel {
        NetLabel { text: text.into(), at, types: types.iter().map(|t| t.to_string()).collect(), port: true }
    }

    fn labels() -> Vec<NetLabel> {
        vec![
            label("A", (1.05, 1.3), &["locali"]),
            label("Y", (1.4, 1.5), &["locali"]),
            label("VPWR", (0.7, 2.5), &["locali"]),
            label("VGND", (0.0, 0.2), &["metal1"]),
        ]
    }

    #[test]
    fn an_inverter_has_its_four_nets_and_terminals() {
        let nl = netlist(inverter(false), &labels());
        assert!(nl.warnings.is_empty(), "{:?}", nl.warnings);
        let name = |i: usize| nl.nets[i].name.clone().unwrap_or_else(|| if nl.nets[i].substrate { "sub".into() } else { "?".into() });
        let got: Vec<(String, [String; 4])> = nl
            .devices
            .iter()
            .map(|(d, t)| (d.model.clone(), [name(t.d), name(t.g), name(t.s), name(t.b)]))
            .collect();
        // Fuente y drenaje van en cualquier orden.
        let norm = |[d, g, s, b]: [String; 4]| {
            let (x, y) = if d < s { (d, s) } else { (s, d) };
            [x, g, y, b]
        };
        let got: Vec<(String, [String; 4])> = got.into_iter().map(|(m, t)| (m, norm(t))).collect();
        assert_eq!(
            got,
            [
                ("mini__nfet".to_string(), ["VGND", "A", "Y", "VGND"].map(String::from)),
                ("mini__pfet".to_string(), ["VPWR", "A", "Y", "?"].map(String::from)),
            ],
            "{:?}",
            nl.nets
        );
        // El cuerpo del nfet es el sustrato, unido a VGND por la toma.
        assert!(nl.nets.iter().any(|n| n.substrate && n.name.as_deref() == Some("VGND")));
    }

    #[test]
    fn a_short_joins_two_nets_and_warns_of_two_names() {
        let nl = netlist(inverter(true), &labels());
        assert!(nl.warnings.iter().any(|w| w.contains("A, Y")), "{:?}", nl.warnings);
        let (_, t) = &nl.devices[0];
        assert!(t.g == t.d || t.g == t.s, "compuerta y drenaje, la misma red: {t:?}");
    }

    #[test]
    fn a_label_off_its_layer_names_nothing() {
        let nl = netlist(inverter(false), &[label("X", (5.0, 5.0), &["locali"])]);
        assert_eq!(nl.label_nets, [None]);
        assert!(nl.warnings.iter().any(|w| w.contains("de su tipo: X")), "{:?}", nl.warnings);
    }
}
