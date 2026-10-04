//! Abiertos, cortos y renombres entre dos versiones de una celda.
//!
//! Las redes no tienen identidad propia: se comparan por **anclas** que sí
//! la tienen en los dos lados, las etiquetas (por texto) y los terminales
//! de los transistores emparejados por posición (como en `devices::diff`).
//! Cada ancla une la red de A donde está con la red de B donde está; en cada
//! grupo conexo de ese grafo:
//!
//! - varias redes de A y una de B: un **corto**;
//! - una red de A y varias de B: un **abierto**; salvo que la de A ya fuera
//!   un corto (tenía varias etiquetas) y cada red de B se quede con alguna
//!   de ellas: entonces el corto se **resolvió**;
//! - una y una, con otra etiqueta: un **renombre**.
//!
//! Lo que existe de un solo lado (un transistor agregado) no es ancla:
//! agregar un transistor no es un corto.

use crate::devices::extract::point_in;
use crate::devices::{flat_polygon_estimate, Device, DeviceRules};

use super::{cell_nets, Netlist};

/// Qué le pasó a una red.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetChangeKind {
    /// Una red se partió.
    Open,
    /// Dos o más redes se unieron.
    Short,
    /// Una red con varias etiquetas (un corto) se separó en redes que se
    /// quedan cada una con alguna: el corto se resolvió.
    Separated,
    /// La misma red con otra etiqueta.
    Renamed,
}

/// Un cambio de conectividad en una celda.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NetChange {
    pub cell: String,
    pub kind: NetChangeKind,
    /// Las redes de antes (un corto: dos o más) y de después (un abierto:
    /// dos o más), por su nombre o cómo encontrarlas.
    pub before: Vec<String>,
    pub after: Vec<String>,
    /// Dónde: los cambios de geometría que tocan esas redes, o la caja de la
    /// red si ninguno (µm, `[min_x, min_y, max_x, max_y]`).
    pub bbox_um: [f64; 4],
}

/// Cómo mostrar una red: sus etiquetas (`B = Y` si tiene dos, como tras un
/// corto), o el terminal de un transistor que la toca (`d de nfet_01v8 en
/// (1.20, 0.50)`).
pub fn net_label(nl: &Netlist, i: usize, unit_um: f64) -> String {
    if nl.nets[i].labels.len() > 1 {
        return nl.nets[i].labels.join(" = ");
    }
    if let Some(n) = &nl.nets[i].name {
        return n.clone();
    }
    if nl.nets[i].substrate {
        return "sustrato".into();
    }
    for (dev, t) in &nl.devices {
        for (term, n) in [("g", t.g), ("d", t.d), ("s", t.s), ("b", t.b)] {
            if n == i {
                let short = dev.model.rsplit("__").next().unwrap_or(&dev.model);
                return format!("{term} de {short} en ({:.2}, {:.2})", dev.at.0 * unit_um, dev.at.1 * unit_um);
            }
        }
    }
    for (r, e) in &nl.resistors {
        if e.contains(&i) {
            return format!("{} en ({:.2}, {:.2})", r.model, r.at.0 * unit_um, r.at.1 * unit_um);
        }
    }
    format!("n{i}")
}

/// Los transistores de B emparejados con los de A (índices), por posición:
/// uno contiene el punto interior del otro.
fn pair_devices(a: &[(Device, super::Terminals)], b: &[(Device, super::Terminals)]) -> Vec<(usize, usize)> {
    let mut used = vec![false; a.len()];
    let mut out = Vec::new();
    for (j, (db, _)) in b.iter().enumerate() {
        let hit = a.iter().enumerate().position(|(i, (da, _))| {
            !used[i] && (point_in(&da.gate.points, db.at.0, db.at.1) || point_in(&db.gate.points, da.at.0, da.at.1))
        });
        if let Some(i) = hit {
            used[i] = true;
            out.push((i, j));
        }
    }
    out
}

fn dist2(p: (f64, f64), q: (f64, f64)) -> f64 {
    (p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)
}

/// Las anclas: pares (red en A, red en B).
fn anchors(a: &Netlist, b: &Netlist) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (i, j) in pair_devices(&a.devices, &b.devices) {
        let ((da, ta), (db, tb)) = (&a.devices[i], &b.devices[j]);
        out.push((ta.g, tb.g));
        out.push((ta.b, tb.b));
        // Fuente y drenaje: el de B más cerca de cada uno de A.
        if let ([sa, _], [sb, db_]) = (da.sd_at.as_slice(), db.sd_at.as_slice()) {
            if dist2(*sa, *sb) <= dist2(*sa, *db_) {
                out.extend([(ta.s, tb.s), (ta.d, tb.d)]);
            } else {
                out.extend([(ta.s, tb.d), (ta.d, tb.s)]);
            }
        }
    }
    // Etiquetas: la de B con el mismo texto más cerca.
    for (la, na) in a.labels.iter().zip(&a.label_nets) {
        let Some(na) = *na else { continue };
        let near = b
            .labels
            .iter()
            .zip(&b.label_nets)
            .filter(|(lb, nb)| lb.text == la.text && nb.is_some())
            .min_by(|(x, _), (y, _)| dist2(x.at, la.at).total_cmp(&dist2(y.at, la.at)));
        if let Some((_, Some(nb))) = near {
            out.push((na, *nb));
        }
    }
    out
}

fn union_box(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Los cambios de conectividad de una celda. `unit_um`: µm por unidad de
/// las dos netlists; `changed`: las cajas (µm) de los cambios de geometría
/// de la celda, para ubicar cada abierto o corto.
pub fn net_changes(cell: &str, a: &Netlist, b: &Netlist, unit_um: f64, changed: &[[f64; 4]]) -> Vec<NetChange> {
    let na = a.nets.len();
    let mut parent: Vec<usize> = (0..na + b.nets.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for (x, y) in anchors(a, b) {
        let (rx, ry) = (find(&mut parent, x), find(&mut parent, na + y));
        if rx != ry {
            parent[rx.max(ry)] = rx.min(ry);
        }
    }
    let mut groups: std::collections::BTreeMap<usize, (Vec<usize>, Vec<usize>)> = std::collections::BTreeMap::new();
    let mut anchored = vec![false; parent.len()];
    for (x, y) in anchors(a, b) {
        anchored[x] = true;
        anchored[na + y] = true;
    }
    for n in 0..parent.len() {
        if !anchored[n] {
            continue;
        }
        let r = find(&mut parent, n);
        let g = groups.entry(r).or_default();
        if n < na {
            g.0.push(n);
        } else {
            g.1.push(n - na);
        }
    }
    let um = |b: [f64; 4]| b.map(|v| v * unit_um);
    let place = |bbox: [f64; 4]| changed.iter().filter(|c| overlaps(**c, bbox)).copied().reduce(union_box).unwrap_or(bbox);
    let mut out = Vec::new();
    for (ga, gb) in groups.values() {
        let mut before: Vec<String> = ga.iter().map(|&i| net_label(a, i, unit_um)).collect();
        let mut after: Vec<String> = gb.iter().map(|&j| net_label(b, j, unit_um)).collect();
        before.sort();
        after.sort();
        let bbox_b = gb.iter().map(|&j| um(b.nets[j].bbox)).reduce(union_box);
        let bbox_a = ga.iter().map(|&i| um(a.nets[i].bbox)).reduce(union_box);
        if ga.len() > 1 {
            let bbox = place(bbox_b.or(bbox_a).unwrap_or_default());
            out.push(NetChange {
                cell: cell.into(),
                kind: NetChangeKind::Short,
                before: before.clone(),
                after: after.clone(),
                bbox_um: bbox,
            });
        }
        if gb.len() > 1 {
            let bbox = place(bbox_a.or(bbox_b).unwrap_or_default());
            let fixed = match ga.as_slice() {
                [i] => {
                    let had = &a.nets[*i].labels;
                    had.len() > 1
                        && gb.iter().all(|&j| !b.nets[j].labels.is_empty() && b.nets[j].labels.iter().all(|l| had.contains(l)))
                }
                _ => false,
            };
            let kind = if fixed { NetChangeKind::Separated } else { NetChangeKind::Open };
            out.push(NetChange { cell: cell.into(), kind, before: before.clone(), after: after.clone(), bbox_um: bbox });
        }
        if let ([i], [j]) = (ga.as_slice(), gb.as_slice()) {
            if let (Some(x), Some(y)) = (&a.nets[*i].name, &b.nets[*j].name) {
                if x != y {
                    let bbox = bbox_b.unwrap_or_default();
                    out.push(NetChange { cell: cell.into(), kind: NetChangeKind::Renamed, before, after, bbox_um: bbox });
                }
            }
        }
    }
    out
}

/// Si la conectividad cambió entre dos netlists recortadas a unas ventanas
/// (ver `nets::context`), sin depender de etiquetas ni transistores, que en
/// una ventana chica puede no haber: cada pedazo de un lado cuyo punto
/// interior cae en un pedazo del mismo tipo del otro lado une sus dos redes.
/// Si alguna red de un lado queda unida a más de una del otro, cambió.
pub fn pieces_changed(a: &Netlist, b: &Netlist) -> bool {
    use crate::devices::extract::{interior_point, Grid};
    use std::collections::HashMap;
    fn grids(nl: &Netlist) -> HashMap<&str, (Grid, Vec<usize>)> {
        let mut by: HashMap<&str, (Vec<gdstk_rs::OwnedPolygon>, Vec<usize>)> = HashMap::new();
        for p in &nl.pieces {
            let e = by.entry(p.magic.as_str()).or_default();
            e.0.push(p.poly.clone());
            e.1.push(p.net);
        }
        by.into_iter().map(|(t, (polys, nets))| (t, (Grid::new(polys), nets))).collect()
    }
    let (ga, gb) = (grids(a), grids(b));
    let na = a.nets.len();
    let mut parent: Vec<usize> = (0..na + b.nets.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    let mut join = |x: usize, y: usize| {
        let (rx, ry) = (find(&mut parent, x), find(&mut parent, y));
        if rx != ry {
            parent[rx.max(ry)] = rx.min(ry);
        }
    };
    for (from, to, flip) in [(a, &gb, false), (b, &ga, true)] {
        for p in &from.pieces {
            let (Some(pt), Some((grid, nets))) = (interior_point(&p.poly.points), to.get(p.magic.as_str())) else { continue };
            if let Some(i) = grid.find(pt.0, pt.1) {
                let other = nets[i as usize];
                if flip {
                    join(other, na + p.net)
                } else {
                    join(p.net, na + other)
                }
            }
        }
    }
    let mut count: HashMap<usize, (usize, usize)> = HashMap::new();
    for n in 0..parent.len() {
        let r = find(&mut parent, n);
        let e = count.entry(r).or_default();
        if n < na {
            e.0 += 1
        } else {
            e.1 += 1
        }
    }
    count.values().any(|&(x, y)| x > 1 || y > 1)
}

/// Las redes de una celda en los dos lados, si no es demasiado grande
/// (`max_polygons` aplanados); `None` si lo es.
#[allow(clippy::too_many_arguments)]
pub fn cell_net_changes(
    la: &gdstk_rs::Library,
    lb: &gdstk_rs::Library,
    name: &str,
    rules: &DeviceRules,
    max_polygons: u64,
    info: (Option<&gdstk_rs::magic::MagInfo>, Option<&gdstk_rs::magic::MagInfo>),
    changed: &[[f64; 4]],
) -> Option<Vec<NetChange>> {
    let (ca, cb) = (la.find_cell(name)?, lb.find_cell(name)?);
    if flat_polygon_estimate(la, &ca).max(flat_polygon_estimate(lb, &cb)) > max_polygons {
        return None;
    }
    let (a, b) = rayon::join(|| cell_nets(la, &ca, rules, info.0), || cell_nets(lb, &cb, rules, info.1));
    Some(net_changes(name, &a, &b, lb.unit() / 1e-6, changed))
}

#[cfg(test)]
mod tests {
    use super::super::{Net, NetLabel, Terminals};
    use super::*;
    use gdstk_rs::{OwnedPolygon, Point2D};

    fn net(name: Option<&str>) -> Net {
        Net {
            name: name.map(String::from),
            labels: name.into_iter().map(String::from).collect(),
            port: true,
            substrate: false,
            bbox: [0.0, 0.0, 1.0, 1.0],
        }
    }

    fn dev(x: f64) -> Device {
        let p = |x, y| Point2D { x, y };
        Device {
            model: "sky130_fd_pr__nfet_01v8".into(),
            magic: "nfet".into(),
            gate: OwnedPolygon {
                layer: 0,
                datatype: 0,
                points: vec![p(x, 0.0), p(x + 0.15, 0.0), p(x + 0.15, 0.65), p(x, 0.65)],
            },
            at: (x + 0.075, 0.3),
            w_um: 0.65,
            l_um: 0.15,
            sd_at: vec![(x - 0.01, 0.3), (x + 0.16, 0.3)],
        }
    }

    fn label(text: &str, x: f64) -> NetLabel {
        NetLabel { text: text.into(), at: (x, 1.0), types: vec!["locali".into()], port: true }
    }

    /// Dos inversores: A0 → Y0 y A1 → Y1 (redes 0..4), con VGND (4).
    fn two_inverters(nets: Vec<Net>, t0: Terminals, t1: Terminals, label_nets: Vec<Option<usize>>) -> Netlist {
        Netlist {
            pieces: Vec::new(),
            nets,
            devices: vec![(dev(0.0), t0), (dev(2.0), t1)],
            resistors: Vec::new(),
            labels: vec![label("A0", 0.0), label("Y0", 0.2), label("A1", 2.0), label("Y1", 2.2), label("VGND", 5.0)],
            label_nets,
            warnings: Vec::new(),
        }
    }

    fn before() -> Netlist {
        let nets = ["A0", "Y0", "A1", "Y1", "VGND"].iter().map(|n| net(Some(n))).collect();
        two_inverters(
            nets,
            Terminals { g: 0, d: 1, s: 4, b: 4 },
            Terminals { g: 2, d: 3, s: 4, b: 4 },
            vec![Some(0), Some(1), Some(2), Some(3), Some(4)],
        )
    }

    #[test]
    fn nothing_changed_means_no_net_changes() {
        assert!(net_changes("C", &before(), &before(), 1.0, &[]).is_empty());
    }

    #[test]
    fn a_short_joins_two_nets() {
        // Y0 y A1 unidos (una sola red 1 en B, con las dos etiquetas).
        let nets = vec![net(Some("A0")), net(Some("A1")), net(Some("Y1")), net(Some("VGND"))];
        let b = two_inverters(
            nets,
            Terminals { g: 0, d: 1, s: 3, b: 3 },
            Terminals { g: 1, d: 2, s: 3, b: 3 },
            vec![Some(0), Some(1), Some(1), Some(2), Some(3)],
        );
        let ch = net_changes("C", &before(), &b, 1.0, &[[0.5, 0.5, 0.6, 0.6], [9.0, 9.0, 9.5, 9.5]]);
        assert_eq!(ch.len(), 1, "{ch:?}");
        assert_eq!(
            (ch[0].kind, ch[0].before.clone(), ch[0].after.clone()),
            (NetChangeKind::Short, vec!["A1".to_string(), "Y0".into()], vec!["A1".to_string()])
        );
        assert_eq!(ch[0].bbox_um, [0.5, 0.5, 0.6, 0.6], "el cambio de geometría que toca la red");
    }

    #[test]
    fn an_open_splits_a_net() {
        // VGND partido: el segundo inversor queda en otra red sin nombre.
        let nets = ["A0", "Y0", "A1", "Y1", "VGND"].iter().map(|n| net(Some(n))).chain([net(None)]).collect();
        let b = two_inverters(
            nets,
            Terminals { g: 0, d: 1, s: 4, b: 4 },
            Terminals { g: 2, d: 3, s: 5, b: 5 },
            vec![Some(0), Some(1), Some(2), Some(3), Some(4)],
        );
        let ch = net_changes("C", &before(), &b, 1.0, &[]);
        assert_eq!(ch.len(), 1, "{ch:?}");
        assert_eq!(ch[0].kind, NetChangeKind::Open);
        assert_eq!(ch[0].before, ["VGND"]);
        assert_eq!(ch[0].after, ["VGND".to_string(), format!("s de nfet_01v8 en ({:.2}, 0.30)", 2.075)]);
    }

    #[test]
    fn splitting_a_short_along_its_labels_is_a_fix() {
        // Al revés que `a_short_joins_two_nets`: la red "A1 = Y0" vuelve a
        // ser dos, cada una con su etiqueta.
        let nets = vec![
            net(Some("A0")),
            Net {
                name: Some("A1".into()),
                labels: vec!["A1".into(), "Y0".into()],
                port: true,
                substrate: false,
                bbox: [0.0, 0.0, 1.0, 1.0],
            },
            net(Some("Y1")),
            net(Some("VGND")),
        ];
        let shorted = two_inverters(
            nets,
            Terminals { g: 0, d: 1, s: 3, b: 3 },
            Terminals { g: 1, d: 2, s: 3, b: 3 },
            vec![Some(0), Some(1), Some(1), Some(2), Some(3)],
        );
        let ch = net_changes("C", &shorted, &before(), 1.0, &[]);
        assert_eq!(ch.len(), 1, "{ch:?}");
        assert_eq!((ch[0].kind, ch[0].after.clone()), (NetChangeKind::Separated, vec!["A1".to_string(), "Y0".into()]));
    }

    #[test]
    fn a_new_label_is_a_rename() {
        let mut b = before();
        b.nets[1] = net(Some("OUT"));
        b.labels[1].text = "OUT".into();
        let ch = net_changes("C", &before(), &b, 1.0, &[]);
        assert_eq!(ch.len(), 1, "{ch:?}");
        assert_eq!(
            (ch[0].kind, ch[0].before.clone(), ch[0].after.clone()),
            (NetChangeKind::Renamed, vec!["Y0".to_string()], vec!["OUT".to_string()])
        );
    }
}
