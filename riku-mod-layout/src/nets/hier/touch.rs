//! Qué pedazos de distintas celdas conducen juntos: la misma regla que
//! `extract::build_with` (los tipos de una línea `connect` que se tocan o se
//! superponen, con todo agrandado medio nanómetro), sobre pedazos que ya
//! traen su red.
//!
//! En lugar de unir todos los pedazos de un grupo (Clipper es caro con
//! decenas de miles, o con un pozo que cubre todo), se prueban de a pares
//! los que tienen las cajas cerca: dos rectángulos se tocan si sus cajas
//! agrandadas se superponen; si no, con Clipper, solo esos dos. Antes, cada
//! pedazo se recorta a la zona donde puede haber contacto.

use std::collections::HashMap;
use std::sync::OnceLock;

use gdstk_rs::{boolean_owned, offset_owned, BoolOp, GdsTag, OwnedPolygon, Point2D};
use rayon::prelude::*;

use crate::box_grid::BoxGrid;
use crate::devices::extract::bbox;
use crate::devices::DeviceRules;
use crate::nets::extract::connect_groups;

/// Un pedazo de un tipo (canónico) con el nodo al que pertenece.
#[derive(Clone, Debug)]
pub(crate) struct Typed {
    pub magic: String,
    pub poly: OwnedPolygon,
    pub node: u64,
}

const TAG: GdsTag = GdsTag { layer: 0, datatype: 0 };

/// Un rectángulo alineado a los ejes (4 vértices).
fn is_rect(p: &[Point2D]) -> bool {
    let p: Vec<&Point2D> = match p {
        [a, b, c, d] => vec![a, b, c, d],
        [a, b, c, d, e] if a.x == e.x && a.y == e.y => vec![a, b, c, d],
        _ => return false,
    };
    (0..4).all(|i| {
        let (a, b) = (p[i], p[(i + 1) % 4]);
        a.x == b.x || a.y == b.y
    })
}

fn rect(b: [f64; 4]) -> OwnedPolygon {
    let p = |x, y| Point2D { x, y };
    OwnedPolygon { layer: 0, datatype: 0, points: vec![p(b[0], b[1]), p(b[2], b[1]), p(b[2], b[3]), p(b[0], b[3])] }
}

/// Un pedazo listo para probar: su caja, si es un rectángulo y, perezoso,
/// el polígono agrandado.
struct Ready {
    node: u64,
    poly: OwnedPolygon,
    bbox: [f64; 4],
    rect: bool,
    grown: OnceLock<Vec<OwnedPolygon>>,
}

/// Los pares de nodos cuyos pedazos conducen juntos. `zone`: solo importa
/// lo que pasa dentro (se recorta antes); `unit_um`: µm por unidad.
pub(crate) fn touching(rules: &DeviceRules, pieces: &[Typed], zone: Option<[f64; 4]>, unit_um: f64) -> Vec<(u64, u64)> {
    if pieces.len() < 2 {
        return Vec::new();
    }
    let touch = 5e-4 / unit_um;
    let mut by_type: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, p) in pieces.iter().enumerate() {
        by_type.entry(p.magic.as_str()).or_default().push(i);
    }
    // Los grupos de `connect` y, como en la unión de cada tipo de la plana,
    // cada tipo consigo mismo (dos pozos de celdas vecinas que se superponen
    // son un pedazo, aunque el pozo no esté en ninguna línea `connect`).
    let mut groups = connect_groups(rules, &|t| by_type.contains_key(t));
    for t in by_type.keys() {
        groups.insert(vec![t.to_string()]);
    }
    // Cuánto se agranda cada tipo para ver si se toca: medio nanómetro o,
    // si su regla tiene un cierre (`grow` + `shrink`), la mitad de lo que el
    // cierre une (ver `DeviceRules::bridge`).
    let reach: HashMap<&str, f64> = by_type.keys().map(|&t| (t, touch.max(rules.bridge(t) / unit_um / 2.0))).collect();
    let far = reach.values().copied().fold(touch, f64::max);
    // Los pedazos recortados a la zona (agrandada: un toque en el borde cuenta).
    let zone = zone.map(|z| [z[0] - 2.0 * far, z[1] - 2.0 * far, z[2] + 2.0 * far, z[3] + 2.0 * far]);
    let ready: Vec<Vec<Ready>> = pieces
        .par_iter()
        .map(|p| {
            let b = bbox(&p.poly);
            let inside = zone.is_none_or(|z| b[0] >= z[0] && b[1] >= z[1] && b[2] <= z[2] && b[3] <= z[3]);
            let polys = if inside {
                vec![p.poly.clone()]
            } else {
                let z = zone.unwrap_or(b);
                if b[0] > z[2] || b[2] < z[0] || b[1] > z[3] || b[3] < z[1] {
                    Vec::new()
                } else if is_rect(&p.poly.points) {
                    vec![rect([b[0].max(z[0]), b[1].max(z[1]), b[2].min(z[2]), b[3].min(z[3])])]
                } else {
                    boolean_owned(std::slice::from_ref(&p.poly), &[rect(z)], BoolOp::And, TAG).unwrap_or_default()
                }
            };
            polys
                .into_iter()
                .map(|poly| Ready { node: p.node, bbox: bbox(&poly), rect: is_rect(&poly.points), poly, grown: OnceLock::new() })
                .collect()
        })
        .collect();
    let groups: Vec<&Vec<String>> = groups.iter().collect();
    let mut out: Vec<(u64, u64)> = groups
        .par_iter()
        .flat_map_iter(|g| {
            let items: Vec<&Ready> =
                g.iter().flat_map(|t| by_type.get(t.as_str()).into_iter().flatten()).flat_map(|&i| ready[i].iter()).collect();
            // Un tipo solo, con su alcance; un grupo de `connect`, al tocarse.
            let by = match g.as_slice() {
                [t] => reach.get(t.as_str()).copied().unwrap_or(touch),
                _ => touch,
            };
            pairs_of(&items, by)
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Los pares de nodos distintos que se tocan, entre `items`.
fn pairs_of(items: &[&Ready], touch: f64) -> Vec<(u64, u64)> {
    let mut out = Vec::new();
    if items.len() < 2 || items.iter().all(|r| r.node == items[0].node) {
        return out;
    }
    let boxes: Vec<[f64; 4]> =
        items.iter().map(|r| [r.bbox[0] - touch, r.bbox[1] - touch, r.bbox[2] + touch, r.bbox[3] + touch]).collect();
    let grid = BoxGrid::new(&boxes);
    // Una unión local por nodo para no probar dos veces lo que ya se unió.
    let mut nodes: Vec<u64> = items.iter().map(|r| r.node).collect();
    nodes.sort_unstable();
    nodes.dedup();
    let pos = |n: u64| nodes.binary_search(&n).unwrap_or(0);
    let mut parent: Vec<usize> = (0..nodes.len()).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    fn grown(r: &Ready, touch: f64) -> &Vec<OwnedPolygon> {
        r.grown.get_or_init(|| offset_owned(std::slice::from_ref(&r.poly), touch, TAG).unwrap_or_else(|_| vec![r.poly.clone()]))
    }
    for (i, a) in items.iter().enumerate() {
        for j in grid.overlapping(boxes[i]) {
            if j <= i {
                continue;
            }
            let b = items[j];
            let (ra, rb) = (find(&mut parent, pos(a.node)), find(&mut parent, pos(b.node)));
            if ra == rb {
                continue;
            }
            let hit = if a.rect && b.rect {
                true
            } else {
                boolean_owned(grown(a, touch), grown(b, touch), BoolOp::And, TAG).is_ok_and(|v| !v.is_empty())
            };
            if hit {
                parent[ra.max(rb)] = ra.min(rb);
                out.push((a.node.min(b.node), a.node.max(b.node)));
            }
        }
    }
    out
}
