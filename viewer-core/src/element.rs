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
}
