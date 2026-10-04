//! Qué pedazos de distintas celdas conducen juntos: la misma regla que
//! `extract::build_with` (los tipos de una línea `connect` que se tocan o se
//! superponen, con todo agrandado medio nanómetro), sobre pedazos que ya
//! traen su red.
//!
//! En lugar de unir todos los pedazos (Clipper es caro con decenas de
//! miles, o con un pozo que cubre todo), se prueban de a pares solo los de
//! lados distintos (lo propio contra una hija, una hija contra otra: cada
//! lado ya se resolvió al armar su celda) y con las cajas cerca: dos
//! rectángulos se tocan si sus cajas agrandadas se superponen; si no, con
//! Clipper, solo esos dos.

use std::collections::HashMap;

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

/// Un pedazo listo para probar: su caja y si es un rectángulo.
struct Ready {
    node: u64,
    poly: OwnedPolygon,
    bbox: [f64; 4],
    rect: bool,
}

/// Los pares (nodo de `left`, nodo de `right`) cuyos pedazos conducen
/// juntos, sin probar pedazos del mismo lado entre sí (cada lado ya se
/// resolvió al armar su celda): para cada pedazo de la izquierda, los de la
/// derecha con la caja cerca y, de esos, los que se tocan de verdad.
pub(crate) fn bipartite(rules: &DeviceRules, left: &[Typed], right: &[Typed], unit_um: f64) -> Vec<(u64, u64)> {
    if left.is_empty() || right.is_empty() {
        return Vec::new();
    }
    let touch = 5e-4 / unit_um;
    // Qué tipos pueden unirse y a qué distancia.
    let mut types: Vec<&str> = left.iter().chain(right).map(|p| p.magic.as_str()).collect();
    types.sort_unstable();
    types.dedup();
    let groups = connect_groups(rules, &|t| types.contains(&t));
    let mut pairs: HashMap<(&str, &str), f64> = HashMap::new();
    for g in &groups {
        for x in g {
            for y in g {
                pairs.insert((x.as_str(), y.as_str()), touch);
            }
        }
    }
    for &t in &types {
        pairs.insert((t, t), touch.max(rules.bridge(t) / unit_um / 2.0));
    }
    let far = pairs.values().copied().fold(touch, f64::max);
    let ready = |p: &Typed| Ready { node: p.node, bbox: bbox(&p.poly), rect: is_rect(&p.poly.points), poly: p.poly.clone() };
    let r: Vec<Ready> = right.par_iter().map(ready).collect();
    let boxes: Vec<[f64; 4]> = r.iter().map(|x| [x.bbox[0] - far, x.bbox[1] - far, x.bbox[2] + far, x.bbox[3] + far]).collect();
    let grid = BoxGrid::new(&boxes);
    let mut out: Vec<(u64, u64)> = left
        .par_iter()
        .flat_map_iter(|p| {
            let a = ready(p);
            let mut found: Vec<(u64, u64)> = Vec::new();
            for j in grid.overlapping([a.bbox[0] - far, a.bbox[1] - far, a.bbox[2] + far, a.bbox[3] + far]) {
                let b = &r[j];
                if found.iter().any(|&(_, n)| n == b.node) {
                    continue;
                }
                let Some(&by) = pairs.get(&(p.magic.as_str(), right[j].magic.as_str())) else { continue };
                let near = a.bbox[0] - by <= b.bbox[2] + by
                    && b.bbox[0] - by <= a.bbox[2] + by
                    && a.bbox[1] - by <= b.bbox[3] + by
                    && b.bbox[1] - by <= a.bbox[3] + by;
                if !near {
                    continue;
                }
                // Dos rectángulos alineados se tocan si sus cajas agrandadas se
                // superponen; si no, Clipper con los dos agrandados.
                let hit = (a.rect && b.rect) || {
                    let grow =
                        |q: &OwnedPolygon| offset_owned(std::slice::from_ref(q), by, TAG).unwrap_or_else(|_| vec![q.clone()]);
                    boolean_owned(&grow(&a.poly), &grow(&b.poly), BoolOp::And, TAG).is_ok_and(|v| !v.is_empty())
                };
                if hit {
                    found.push((a.node, b.node));
                }
            }
            found
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}
