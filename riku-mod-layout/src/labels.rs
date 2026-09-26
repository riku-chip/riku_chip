//! Labels de toda la jerarquia de una cell, en coordenadas de la cell raiz.
//!
//! `Cell::get_polygons()` ya aplana la geometria de todas las sub-cells, pero
//! gdstk-rs no ofrece el equivalente para labels: `Cell::labels()` solo trae
//! los de la propia cell. Sin esto, una cell jerarquica (p.ej.
//! `sky130_fd_sc_hd__macro_sparecell`) muestra la geometria de sus
//! instancias pero no sus pines, a diferencia de KLayout.
//!
//! Transformacion de una reference (convencion GDSII/gdstk): reflexion en X,
//! luego magnificacion, rotacion y traslacion al origen (+ offset de
//! repeticion para AREF). Solo se transforma el punto de anclaje; el texto se
//! sigue dibujando horizontal.

use gdstk_rs::{Anchor, Cell, GdsTag, Library, Point2D};

/// Label ya ubicado en coordenadas de la cell raiz.
#[derive(Clone, Debug, PartialEq)]
pub struct FlatLabel {
    pub tag: GdsTag,
    pub text: String,
    pub origin: Point2D,
    pub anchor: Anchor,
}

/// Profundidad maxima de la jerarquia. gdstk rechaza ciclos al escribir,
/// pero un archivo corrupto no debe colgar la GUI.
const MAX_DEPTH: usize = 64;

/// Transformacion afin 2D: `x' = a·x + b·y + tx`, `y' = c·x + d·y + ty`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

impl Affine {
    const IDENTITY: Self = Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

    /// Transformacion de una instancia: reflexion X → escala → rotacion → traslacion.
    fn reference(origin: Point2D, rotation: f64, magnification: f64, x_reflection: bool) -> Self {
        let (s, c) = rotation.sin_cos();
        let m = magnification;
        let r = if x_reflection { -1.0 } else { 1.0 };
        Self { a: m * c, b: -m * s * r, c: m * s, d: m * c * r, tx: origin.x, ty: origin.y }
    }

    fn apply(&self, p: Point2D) -> Point2D {
        Point2D { x: self.a * p.x + self.b * p.y + self.tx, y: self.c * p.x + self.d * p.y + self.ty }
    }

    /// `self ∘ inner`: primero `inner`, despues `self`.
    fn then_inner(&self, inner: &Self) -> Self {
        Self {
            a: self.a * inner.a + self.b * inner.c,
            b: self.a * inner.b + self.b * inner.d,
            c: self.c * inner.a + self.d * inner.c,
            d: self.c * inner.b + self.d * inner.d,
            tx: self.a * inner.tx + self.b * inner.ty + self.tx,
            ty: self.c * inner.tx + self.d * inner.ty + self.ty,
        }
    }
}

/// Offsets de repeticion de un elemento; sin repeticion, un unico `(0, 0)`.
fn offsets(count: u64, at: impl Fn(u64) -> Point2D) -> Vec<Point2D> {
    if count == 0 {
        vec![Point2D { x: 0.0, y: 0.0 }]
    } else {
        (0..count).map(at).collect()
    }
}

/// Labels de `cell` y de todas sus sub-cells (recursivo), en coordenadas de `cell`.
pub fn flatten_labels(lib: &Library, cell: &Cell<'_>) -> Vec<FlatLabel> {
    let mut out = Vec::new();
    collect(lib, cell, &Affine::IDENTITY, 0, &mut out);
    out
}

fn collect(lib: &Library, cell: &Cell<'_>, xf: &Affine, depth: usize, out: &mut Vec<FlatLabel>) {
    for label in cell.labels() {
        let tag = GdsTag { layer: label.layer(), datatype: label.texttype() };
        let base = label.origin();
        for off in offsets(label.repetition_count(), |i| label.repetition_offset(i)) {
            out.push(FlatLabel {
                tag,
                text: label.text().into_owned(),
                origin: xf.apply(Point2D { x: base.x + off.x, y: base.y + off.y }),
                anchor: label.anchor(),
            });
        }
    }

    if depth >= MAX_DEPTH {
        return;
    }
    for r in cell.references() {
        let Some(child) = lib.find_cell(r.cell_name()) else { continue };
        let o = r.origin();
        for off in offsets(r.repetition_count(), |i| r.repetition_offset(i)) {
            let origin = Point2D { x: o.x + off.x, y: o.y + off.y };
            let local = Affine::reference(origin, r.rotation(), r.magnification(), r.x_reflection());
            collect(lib, &child, &xf.then_inner(&local), depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    fn pt(x: f64, y: f64) -> Point2D {
        Point2D { x, y }
    }

    fn close(a: Point2D, b: Point2D) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn reference_transform_matches_gds_order() {
        // Reflexion X, rotacion 90°, escala 2, origen (10, 0): (1, 1) →
        // reflejar (1, -1) → escalar (2, -2) → rotar 90° (2, 2) → trasladar (12, 2).
        let t = Affine::reference(pt(10.0, 0.0), FRAC_PI_2, 2.0, true);
        assert!(close(t.apply(pt(1.0, 1.0)), pt(12.0, 2.0)));
    }

    #[test]
    fn composition_applies_inner_first() {
        let outer = Affine::reference(pt(100.0, 0.0), 0.0, 1.0, false);
        let inner = Affine::reference(pt(0.0, 0.0), FRAC_PI_2, 1.0, false);
        // inner rota (1, 0) → (0, 1); outer traslada → (100, 1).
        assert!(close(outer.then_inner(&inner).apply(pt(1.0, 0.0)), pt(100.0, 1.0)));
    }

    #[test]
    fn no_repetition_yields_single_zero_offset() {
        assert_eq!(offsets(0, |_| unreachable!()), vec![pt(0.0, 0.0)]);
        assert_eq!(offsets(2, |i| pt(i as f64, 0.0)), vec![pt(0.0, 0.0), pt(1.0, 0.0)]);
    }
}
