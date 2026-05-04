//! Atribucion de origen para poligonos resultantes de un XOR jerarquico.
//!
//! Tras correr `xor_split_flat` sobre la cell completa con `depth=-1`,
//! cada poligono del resultado puede provenir o bien de geometria
//! directa de la cell raiz, o bien de una reference SREF/AREF cuya
//! sub-cell se haya modificado. Este modulo decide cual de los dos
//! buscando en cual de las references de la cell cae el centroide
//! del poligono. Profundidad: la atribucion es de un solo nivel —
//! sub-cells anidadas mas profundo se reportan via su reference
//! inmediata.

use gdstk_rs::{Cell, OwnedPolygon};

/// Identidad de origen: cadena de nombres de cells desde la cell raiz
/// hasta la sub-cell que aporto el poligono. Longitud 1 si nace en la
/// propia cell raiz, 2 si nace via una reference (max en fase 1).
pub type OriginPath = Vec<String>;

/// Centro del bounding box del poligono. Para poligonos convexos simples
/// (resultado tipico de `gdstk::boolean`) coincide aprox. con el
/// centroide geometrico, suficiente para clasificar contra bboxes de
/// references.
fn polygon_bbox_center(p: &OwnedPolygon) -> Option<(f64, f64)> {
    if p.points.is_empty() {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for pt in &p.points {
        if pt.x < min_x {
            min_x = pt.x;
        }
        if pt.y < min_y {
            min_y = pt.y;
        }
        if pt.x > max_x {
            max_x = pt.x;
        }
        if pt.y > max_y {
            max_y = pt.y;
        }
    }
    Some(((min_x + max_x) * 0.5, (min_y + max_y) * 0.5))
}

/// Atribuye el poligono a la reference mas especifica (bbox mas chico)
/// cuyo bbox contenga el centro del poligono. Si ninguna reference lo
/// contiene, atribuye a la propia cell raiz.
pub fn origin_of_polygon<'a>(cell: &Cell<'a>, poly: &OwnedPolygon) -> OriginPath {
    let cell_name = cell.name().to_string();
    let Some((cx, cy)) = polygon_bbox_center(poly) else {
        return vec![cell_name];
    };

    let mut best: Option<(f64, String)> = None;
    for r in cell.references() {
        let bb = r.bbox();
        if cx >= bb.min_x && cx <= bb.max_x && cy >= bb.min_y && cy <= bb.max_y {
            let area = (bb.max_x - bb.min_x).max(0.0) * (bb.max_y - bb.min_y).max(0.0);
            let target = r.cell_name().to_string();
            best = Some(match best {
                None => (area, target),
                Some((a, n)) => {
                    if area < a || (area == a && target < n) {
                        (area, target)
                    } else {
                        (a, n)
                    }
                }
            });
        }
    }

    match best {
        Some((_, target)) => vec![cell_name, target],
        None => vec![cell_name],
    }
}
