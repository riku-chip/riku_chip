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

use gdstk_rs::{BoundingBox, Cell, OwnedPolygon, Point2D};

use crate::box_grid::BoxGrid;

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

/// Repeticiones por encima de las cuales no se busca la instancia exacta: se
/// atribuye al arreglo completo (evita O(polígonos × repeticiones)).
const MAX_REPETITIONS: u64 = 4096;

/// Origen de un poligono del XOR.
#[derive(Clone, Debug, PartialEq)]
pub struct Origin {
    pub path: OriginPath,
    /// Posicion (unidades de usuario) de la instancia que lo aporto: origen
    /// de la reference mas el offset de su repeticion. `None` si nace en la
    /// propia cell raiz. Para arreglos enormes, el origen del arreglo.
    pub instance_at: Option<(f64, f64)>,
}

impl Origin {
    /// Clave estable entre A y B: la misma instancia (no movida) tiene el
    /// mismo origen en ambos lados aunque cambie el orden de references.
    pub fn key(&self) -> (OriginPath, Option<(i64, i64)>) {
        let q = |v: f64| (v * 1e6).round() as i64;
        (self.path.clone(), self.instance_at.map(|(x, y)| (q(x), q(y))))
    }
}

/// Las instancias de una cell con su bbox, calculadas una sola vez para
/// atribuir muchos polígonos: el bbox de una referencia lo calcula gdstk
/// recorriendo todo su subárbol, y pedirlo por cada polígono del XOR (cientos
/// de miles cuando se mueve una instancia grande) tardaba horas.
pub struct Origins {
    cell: String,
    /// (bbox, cell de la instancia, posición): una entrada por repetición de
    /// cada referencia, o una por el arreglo entero si es enorme.
    instances: Vec<(BoundingBox, String, Point2D)>,
    /// Índice espacial de `instances`: un polígono mira solo las instancias
    /// de su zona, no todas (una celda estándar usada 50 000 veces eran
    /// 50 000 chequeos por polígono).
    grid: BoxGrid,
}

impl Origins {
    pub fn new(cell: &Cell<'_>) -> Self {
        let mut instances = Vec::new();
        for r in cell.references() {
            let all = r.bbox();
            let o = r.origin();
            let target = r.cell_name().to_string();
            let n = r.repetition_count();
            if n <= 1 || n > MAX_REPETITIONS {
                instances.push((all, target, o));
                continue;
            }
            // El bbox de gdstk cubre todas las repeticiones: es el de la
            // instancia 0 desplazado por el rango de offsets (suma de Minkowski).
            let offsets: Vec<Point2D> = r.repetition().offsets().collect();
            let (mut lo, mut hi) = (offsets[0], offsets[0]);
            for p in &offsets {
                lo = Point2D { x: lo.x.min(p.x), y: lo.y.min(p.y) };
                hi = Point2D { x: hi.x.max(p.x), y: hi.y.max(p.y) };
            }
            for off in &offsets {
                let b = BoundingBox {
                    min_x: all.min_x - lo.x + off.x,
                    min_y: all.min_y - lo.y + off.y,
                    max_x: all.max_x - hi.x + off.x,
                    max_y: all.max_y - hi.y + off.y,
                };
                instances.push((b, target.clone(), Point2D { x: o.x + off.x, y: o.y + off.y }));
            }
        }
        let boxes: Vec<[f64; 4]> = instances.iter().map(|(b, ..)| [b.min_x, b.min_y, b.max_x, b.max_y]).collect();
        Self { cell: cell.name().to_string(), grid: BoxGrid::new(&boxes), instances }
    }

    /// Atribuye el poligono a la instancia mas especifica (bbox mas chico)
    /// que contenga su centro. Cada repeticion de un AREF cuenta como una
    /// instancia propia. Si ninguna lo contiene, lo atribuye a la propia
    /// cell raiz.
    pub fn of(&self, poly: &OwnedPolygon) -> Origin {
        let root = || Origin { path: vec![self.cell.clone()], instance_at: None };
        let Some((cx, cy)) = polygon_bbox_center(poly) else {
            return root();
        };
        let area = |b: &BoundingBox| (b.max_x - b.min_x).max(0.0) * (b.max_y - b.min_y).max(0.0);
        // (area, cell, x, y): el menor gana; cell y posicion desempatan.
        let best = self
            .grid
            .containing(cx, cy)
            .map(|k| &self.instances[k])
            .map(|(b, target, at)| (area(b), target.as_str(), at.x, at.y))
            .min_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        match best {
            Some((_, target, x, y)) => {
                Origin { path: vec![self.cell.clone(), target.to_string()], instance_at: Some((x, y)) }
            }
            None => root(),
        }
    }
}
