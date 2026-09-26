//! Primitivas de dibujo neutras — mínimo común a esquemáticos y layouts.
//!
//! Cada backend (xschem, gds, ...) produce estos elementos desde su representación
//! interna, aplicando transformaciones y resolución de alineación. Variantes
//! específicas (como `MissingSymbol` de Xschem) viven en tipos extendidos del
//! backend, no aquí.

use serde::{Deserialize, Serialize};

use crate::bbox::BoundingBox;

/// Capa de dibujo. `u16` cubre holgadamente el rango estándar de GDS/Xschem.
pub type Layer = u16;

/// Alineación horizontal del texto relativa a su punto ancla `(x, y)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HAlign {
    Start,
    Middle,
    End,
}

/// Alineación vertical del texto relativa a su punto ancla `(x, y)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

/// Primitiva de dibujo neutral. Los backends traducen sus estructuras a este
/// enum en el límite del adaptador.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DrawElement {
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        layer: Layer,
    },
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        layer: Layer,
        filled: bool,
    },
    Circle {
        cx: f64,
        cy: f64,
        r: f64,
        layer: Layer,
        filled: bool,
    },
    Polygon {
        points: Vec<(f64, f64)>,
        layer: Layer,
        filled: bool,
    },
    /// Texto con ancla libre. El ángulo está en **grados** (convención matemática:
    /// positivo = antihorario en eje Y-up, o sea horario en Y-down). Los backends
    /// con rotación discreta (0/90/180/270) deben convertir al poblar.
    Text {
        x: f64,
        y: f64,
        content: String,
        /// Altura visual del glifo en unidades de mundo.
        size: f64,
        angle_deg: f64,
        h_align: HAlign,
        v_align: VAlign,
        layer: Layer,
    },
}

impl DrawElement {
    pub fn layer(&self) -> Layer {
        match self {
            Self::Line { layer, .. }
            | Self::Rect { layer, .. }
            | Self::Circle { layer, .. }
            | Self::Polygon { layer, .. }
            | Self::Text { layer, .. } => *layer,
        }
    }

    /// Bounding box neutral del primitivo. Para `Text`, solo el ancla — medir
    /// el glifo real requiere métricas de fuente que no viven en viewer-core.
    pub fn bounding_box(&self) -> BoundingBox {
        match self {
            Self::Line { x1, y1, x2, y2, .. } => {
                BoundingBox::from_points((*x1, *y1), (*x2, *y2))
            }
            Self::Rect { x, y, w, h, .. } => {
                BoundingBox::from_points((*x, *y), (*x + *w, *y + *h))
            }
            Self::Circle { cx, cy, r, .. } => BoundingBox {
                min_x: *cx - *r,
                min_y: *cy - *r,
                max_x: *cx + *r,
                max_y: *cy + *r,
            },
            Self::Polygon { points, .. } => {
                let mut bb = BoundingBox::empty();
                for (x, y) in points {
                    bb.expand_point(*x, *y);
                }
                bb
            }
            Self::Text { x, y, .. } => BoundingBox::point(*x, *y),
        }
    }

    /// ¿El punto `(px, py)` cae dentro del primitivo? Solo tiene sentido para
    /// primitivos con área (`Rect`, `Circle`, `Polygon`), rellenos o no: un
    /// contorno también "ocupa" su interior para el usuario que apunta. Líneas
    /// y textos no se pueden señalar así (retornan `false`).
    ///
    /// Polígonos con regla par-impar: correcta para simples y cóncavos, y para
    /// los anillos "keyhole" de GDS (el hueco queda fuera).
    pub fn contains_point(&self, px: f64, py: f64) -> bool {
        match self {
            Self::Rect { x, y, w, h, .. } => {
                BoundingBox::from_points((*x, *y), (*x + *w, *y + *h)).contains(px, py)
            }
            Self::Circle { cx, cy, r, .. } => (px - cx).powi(2) + (py - cy).powi(2) <= r * r,
            Self::Polygon { points, .. } => {
                if points.len() < 3 {
                    return false;
                }
                let mut inside = false;
                let mut j = points.len() - 1;
                for i in 0..points.len() {
                    let (xi, yi) = points[i];
                    let (xj, yj) = points[j];
                    if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
                        inside = !inside;
                    }
                    j = i;
                }
                inside
            }
            Self::Line { .. } | Self::Text { .. } => false,
        }
    }

    /// Área en unidades de mundo al cuadrado, para primitivos con área.
    pub fn area(&self) -> Option<f64> {
        match self {
            Self::Rect { w, h, .. } => Some((w * h).abs()),
            Self::Circle { r, .. } => Some(std::f64::consts::PI * r * r),
            Self::Polygon { points, .. } if points.len() >= 3 => {
                let n = points.len();
                let twice: f64 = (0..n)
                    .map(|i| {
                        let ((x1, y1), (x2, y2)) = (points[i], points[(i + 1) % n]);
                        x1 * y2 - x2 * y1
                    })
                    .fold(0.0, |acc, v| acc + v);
                Some(twice.abs() * 0.5)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poly(points: &[(f64, f64)]) -> DrawElement {
        DrawElement::Polygon { points: points.to_vec(), layer: 0, filled: true }
    }

    #[test]
    fn concave_polygon_hit_test_and_area() {
        // L de 3x3 menos la esquina superior derecha 2x2 → área 5.
        let l = poly(&[(0.0, 0.0), (3.0, 0.0), (3.0, 1.0), (1.0, 1.0), (1.0, 3.0), (0.0, 3.0)]);
        assert!(l.contains_point(0.5, 2.5));
        assert!(l.contains_point(2.5, 0.5));
        assert!(!l.contains_point(2.0, 2.0), "la esquina recortada queda fuera");
        assert_eq!(l.area(), Some(5.0));
    }

    #[test]
    fn keyhole_ring_excludes_hole() {
        // Anillo 10x10 con hueco 6x6 codificado con corte (como en GDS).
        let ring = poly(&[
            (0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 2.0),
            (2.0, 2.0), (2.0, 8.0), (8.0, 8.0), (8.0, 2.0), (0.0, 2.0),
        ]);
        assert!(ring.contains_point(1.0, 5.0));
        assert!(!ring.contains_point(5.0, 5.0), "el hueco no es parte del polígono");
        assert!((ring.area().unwrap() - 64.0).abs() < 1e-9);
    }

    #[test]
    fn rect_circle_and_non_area_primitives() {
        let r = DrawElement::Rect { x: 0.0, y: 0.0, w: 2.0, h: 1.0, layer: 0, filled: false };
        assert!(r.contains_point(1.0, 0.5) && !r.contains_point(3.0, 0.5));
        assert_eq!(r.area(), Some(2.0));
        let c = DrawElement::Circle { cx: 0.0, cy: 0.0, r: 1.0, layer: 0, filled: true };
        assert!(c.contains_point(0.5, 0.5) && !c.contains_point(1.0, 1.0));
        let line = DrawElement::Line { x1: 0.0, y1: 0.0, x2: 1.0, y2: 1.0, layer: 0 };
        assert!(!line.contains_point(0.5, 0.5) && line.area().is_none());
    }
}
