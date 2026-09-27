//! Geometría del grafo del panel History: dónde va cada nodo, cada línea y
//! cada curva de una fila. Función pura (sin egui), así se prueba sin
//! ventana; la vista solo la pinta.
//!
//! Un tramo va del centro del nodo de una fila al centro de la siguiente. Si
//! cambia de columna es una curva Bézier cúbica que sale y llega vertical
//! (los puntos de control están a media fila, en la columna de salida y en
//! la de llegada): las ramas se abren y se juntan suaves, como en el Git
//! Graph de VS Code.

use crate::core::analysis::graph::GraphRow;

/// Medidas del grafo, en puntos de egui (grilla de 4/8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub row_h: f32,
    pub col_w: f32,
    pub node_r: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self { row_h: 24.0, col_w: 14.0, node_r: 4.0 }
    }
}

impl Metrics {
    /// Centro de la columna `col` de una fila cuyo borde superior está en `top`.
    pub fn center(&self, x0: f32, top: f32, col: usize) -> [f32; 2] {
        [x0 + (col as f32 + 0.5) * self.col_w, top + self.row_h * 0.5]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Line { from: [f32; 2], to: [f32; 2], lane: usize },
    Curve { points: [[f32; 2]; 4], lane: usize },
    Node { center: [f32; 2], radius: f32, lane: usize, hollow: bool },
}

impl Prim {
    pub fn lane(&self) -> usize {
        match self {
            Prim::Line { lane, .. } | Prim::Curve { lane, .. } | Prim::Node { lane, .. } => *lane,
        }
    }
}

/// Los tramos que salen de una fila hacia la siguiente (se dibujan antes que
/// los nodos, para que los nodos queden encima).
pub fn edges(row: &GraphRow, m: &Metrics, x0: f32, top: f32) -> Vec<Prim> {
    row.edges
        .iter()
        .map(|&(a, b, lane)| {
            let from = m.center(x0, top, a);
            let to = m.center(x0, top + m.row_h, b);
            if a == b {
                Prim::Line { from, to, lane }
            } else {
                let mid = from[1] + m.row_h * 0.5;
                Prim::Curve { points: [from, [from[0], mid], [to[0], mid], to], lane }
            }
        })
        .collect()
}

/// El nodo de la fila (hueco si es un merge).
pub fn node(row: &GraphRow, m: &Metrics, x0: f32, top: f32, merge: bool) -> Prim {
    Prim::Node { center: m.center(x0, top, row.column), radius: m.node_r, lane: row.lane, hollow: merge }
}

/// Columnas que ocupa una fila (nodo, ramas que pasan y tramos), para
/// reservar el ancho del grafo.
pub fn width(row: &GraphRow) -> usize {
    std::iter::once(row.column)
        .chain(row.passing.iter().map(|p| p.0))
        .chain(row.edges.iter().flat_map(|e| [e.0, e.1]))
        .max()
        .map_or(1, |c| c + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::analysis::graph::{layout, tests::dag};

    #[test]
    fn edges_start_and_end_at_node_centers() {
        let d = dag("m:a f, a:b, f:b, b:");
        let rows = layout(&d);
        let m = Metrics::default();
        // Fila 0 (merge): una línea recta hacia abajo y una curva a la columna 1.
        let prims = edges(&rows[0], &m, 10.0, 0.0);
        let n0 = node(&rows[0], &m, 10.0, 0.0, true);
        let Prim::Node { center, hollow, .. } = n0 else { panic!() };
        assert!(hollow);
        assert_eq!(center, [17.0, 12.0]);
        assert_eq!(prims[0], Prim::Line { from: [17.0, 12.0], to: [17.0, 36.0], lane: 0 });
        let Prim::Curve { points, lane } = &prims[1] else { panic!("{prims:?}") };
        assert_eq!(*lane, 1);
        assert_eq!(points[0], [17.0, 12.0]);
        assert_eq!(points[3], [31.0, 36.0]);
        // Sale y llega vertical: los controles comparten x con sus extremos.
        assert_eq!(points[1][0], points[0][0]);
        assert_eq!(points[2][0], points[3][0]);
        // La fila de la rama lateral (columna 1) termina en el nodo de b.
        let Prim::Curve { points, .. } = &edges(&rows[2], &m, 10.0, 48.0)[1] else { panic!() };
        assert_eq!(points[3], m.center(10.0, 72.0, 0));
    }

    #[test]
    fn width_counts_every_column_the_row_touches() {
        let rows = layout(&dag("m:a f, a:b, f:b, b:"));
        assert_eq!(rows.iter().map(width).collect::<Vec<_>>(), vec![2, 2, 2, 1]);
    }
}
