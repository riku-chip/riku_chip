//! Cambios heredados: un cambio en una sub-celda puede cortar o abrir redes
//! recién en una celda de arriba (el metal de la sub-celda que ahora toca
//! algo del padre, o una pista de bit que toca la de la instancia vecina).
//!
//! Analizar cada ancestro entero cuesta segundos (el banco, la macro). En
//! cambio, se mira solo alrededor del cambio: su caja, llevada a cada lugar
//! donde está instanciada la sub-celda en cada ancestro (las **ventanas**),
//! se compara la conectividad recortada a ellas y, si aparece un abierto o
//! un corto, se confirma con la celda entera (el recorte puede partir una
//! red y dar un falso positivo; confirmarlo es caro, pero solo cuando hay
//! algo).

use std::collections::{BTreeSet, HashMap};

use gdstk_rs::{Library, Point2D};

use crate::labels::{offsets, Affine};

/// Hasta cuántas ventanas por celda: con más, una sola caja que las cubre.
const MAX_WINDOWS: usize = 20_000;

/// Las ventanas de cada celda (cajas en unidades de la librería). `own`: las
/// cajas de los cambios propios de cada celda; `via`: por celda, las hijas
/// directas por las que le llega un cambio.
pub(crate) fn windows(
    lib: &Library,
    own: &HashMap<String, Vec<[f64; 4]>>,
    via: &HashMap<String, BTreeSet<String>>,
) -> HashMap<String, Vec<[f64; 4]>> {
    let mut memo: HashMap<String, Vec<[f64; 4]>> = HashMap::new();
    let cells: BTreeSet<&String> = own.keys().chain(via.keys()).collect();
    for c in cells {
        of(lib, c, own, via, &mut memo, 0);
    }
    memo
}

fn of(
    lib: &Library,
    cell: &str,
    own: &HashMap<String, Vec<[f64; 4]>>,
    via: &HashMap<String, BTreeSet<String>>,
    memo: &mut HashMap<String, Vec<[f64; 4]>>,
    depth: usize,
) -> Vec<[f64; 4]> {
    if let Some(w) = memo.get(cell) {
        return w.clone();
    }
    let mut out: Vec<[f64; 4]> = own.get(cell).cloned().unwrap_or_default();
    if let (Some(children), Some(c), true) = (via.get(cell), lib.find_cell(cell), depth < 64) {
        for child in children {
            let inner = of(lib, child, own, via, memo, depth + 1);
            if inner.is_empty() {
                continue;
            }
            for r in c.references().filter(|r| r.cell_name() == child.as_str()) {
                let o = r.origin();
                for off in offsets(r.repetition_count(), |i| r.repetition_offset(i)) {
                    let t = Affine::reference(
                        Point2D { x: o.x + off.x, y: o.y + off.y },
                        r.rotation(),
                        r.magnification(),
                        r.x_reflection(),
                    );
                    out.extend(inner.iter().map(|b| transform(&t, b)));
                }
            }
        }
    }
    if out.len() > MAX_WINDOWS {
        let all = out.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |a, b| {
            [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
        });
        out = vec![all];
    }
    memo.insert(cell.to_string(), out.clone());
    out
}

/// La caja de un rectángulo transformado.
fn transform(t: &Affine, b: &[f64; 4]) -> [f64; 4] {
    let corners = [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3])].map(|(x, y)| t.apply(Point2D { x, y }));
    corners.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |a, p| {
        [a[0].min(p.x), a[1].min(p.y), a[2].max(p.x), a[3].max(p.y)]
    })
}
