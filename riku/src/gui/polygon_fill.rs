//! Relleno de polígonos simples (convexos o cóncavos) con egui, para escenas
//! sin índice (ver `viewer_core::index`), que triangulan en cada cuadro.
//!
//! `Shape::convex_polygon` triangula en abanico desde el primer vértice: con
//! polígonos cóncavos (L, U, anillos "keyhole" de GDS) pinta triángulos fuera
//! de la figura. Aquí se decide por polígono:
//!
//! - convexo → `convex_polygon` (camino rápido, la mayoría de un layout);
//! - cóncavo → triangulación earcut + `Mesh`, contorno aparte;
//! - earcut sin resultado → solo contorno (nunca relleno fuera de la figura).

use eframe::egui::{self, Color32, Pos2, Shape, Stroke};
use viewer_core::fill::{is_convex, triangulate};

/// Pinta `world` (ya proyectado a `screen`, mismo orden) con relleno y contorno.
pub fn paint_filled_polygon(
    painter: &egui::Painter,
    world: &[(f64, f64)],
    screen: Vec<Pos2>,
    fill: Color32,
    stroke: Stroke,
) {
    // Capas de solo contorno (implantes, marcadores…): no triangular.
    if fill.a() == 0 {
        painter.add(Shape::closed_line(screen, stroke));
        return;
    }
    if is_convex(world) {
        painter.add(Shape::convex_polygon(screen, fill, stroke));
        return;
    }

    let indices = triangulate(world);
    if !indices.is_empty() {
        let mut mesh = egui::Mesh::default();
        for p in &screen {
            mesh.colored_vertex(*p, fill);
        }
        for tri in indices.chunks_exact(3) {
            mesh.add_triangle(tri[0] as u32, tri[1] as u32, tri[2] as u32);
        }
        painter.add(Shape::mesh(mesh));
    }
    painter.add(Shape::closed_line(screen, stroke));
}
