//! Relleno de polígonos simples (convexos o cóncavos) con egui.
//!
//! `Shape::convex_polygon` triangula en abanico desde el primer vértice: con
//! polígonos cóncavos (L, U, anillos "keyhole" de GDS) pinta triángulos fuera
//! de la figura. Aquí se decide por polígono:
//!
//! - convexo → `convex_polygon` (camino rápido, la mayoría de un layout);
//! - cóncavo → triangulación earcut + `Mesh`, contorno aparte;
//! - earcut sin resultado → solo contorno (nunca relleno fuera de la figura).
//!
//! La triangulación se hace en coordenadas de mundo (`f64`) y los índices se
//! aplican a los puntos de pantalla: la transformación es afín, así que la
//! partición en triángulos es la misma (el reflejo de Y solo invierte el
//! sentido de giro, que earcut acepta en ambos casos).

use eframe::egui::{self, Color32, Pos2, Shape, Stroke};

/// ¿Todos los giros del polígono van en el mismo sentido? Ignora aristas
/// colineales y el vértice de cierre repetido. Menos de 3 puntos no forman
/// área: se reportan como no convexos para que el caller no los rellene.
pub fn is_convex(points: &[(f64, f64)]) -> bool {
    let pts = strip_closing_point(points);
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0_f64;
    for i in 0..n {
        let (ax, ay) = pts[i];
        let (bx, by) = pts[(i + 1) % n];
        let (cx, cy) = pts[(i + 2) % n];
        let cross = (bx - ax) * (cy - by) - (by - ay) * (cx - bx);
        if cross == 0.0 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    sign != 0.0
}

/// Índices de triángulos (tríos) que cubren el polígono. Vacío si el polígono
/// es degenerado o earcut no pudo triangularlo.
pub fn triangulate(points: &[(f64, f64)]) -> Vec<usize> {
    let pts = strip_closing_point(points);
    if pts.len() < 3 {
        return Vec::new();
    }
    let flat: Vec<f64> = pts.iter().flat_map(|&(x, y)| [x, y]).collect();
    earcutr::earcut(&flat, &[], 2).unwrap_or_default()
}

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

/// GDS (y muchos formatos) repiten el primer vértice al final para cerrar.
fn strip_closing_point(points: &[(f64, f64)]) -> &[(f64, f64)] {
    match (points.first(), points.last()) {
        (Some(a), Some(b)) if points.len() > 1 && a == b => &points[..points.len() - 1],
        _ => points,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Área total de los triángulos (fórmula del determinante).
    fn tri_area(points: &[(f64, f64)], idx: &[usize]) -> f64 {
        idx.chunks_exact(3)
            .map(|t| {
                let (a, b, c) = (points[t[0]], points[t[1]], points[t[2]]);
                ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() * 0.5
            })
            .sum()
    }

    fn shoelace(points: &[(f64, f64)]) -> f64 {
        let n = points.len();
        (0..n)
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % n]);
                a.0 * b.1 - b.0 * a.1
            })
            .sum::<f64>()
            .abs()
            * 0.5
    }

    const SQUARE: [(f64, f64); 4] = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    /// L de 3x3 menos la esquina superior derecha 2x2 → área 5.
    const L_SHAPE: [(f64, f64); 6] =
        [(0.0, 0.0), (3.0, 0.0), (3.0, 1.0), (1.0, 1.0), (1.0, 3.0), (0.0, 3.0)];

    #[test]
    fn convexity_detection() {
        assert!(is_convex(&SQUARE));
        assert!(!is_convex(&L_SHAPE));
        // Vértice colineal y cierre repetido no rompen la detección.
        let with_extras = [(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)];
        assert!(is_convex(&with_extras));
        // Sentido horario (como queda tras reflejar Y) también es convexo.
        let cw: Vec<_> = SQUARE.iter().rev().copied().collect();
        assert!(is_convex(&cw));
        assert!(!is_convex(&[(0.0, 0.0), (1.0, 1.0)]));
    }

    #[test]
    fn concave_triangulation_covers_exact_area() {
        let idx = triangulate(&L_SHAPE);
        assert_eq!(idx.len() % 3, 0);
        assert!((tri_area(&L_SHAPE, &idx) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn keyhole_ring_triangulates_to_ring_area() {
        // Anillo 10x10 con hueco 6x6 codificado como un solo polígono con
        // corte (así llegan los guard rings desde GDS). Área = 100 - 36.
        let ring = [
            (0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 2.0),
            (2.0, 2.0), (2.0, 8.0), (8.0, 8.0), (8.0, 2.0), (0.0, 2.0),
        ];
        let pts = &ring[..];
        let idx = triangulate(pts);
        assert!(!idx.is_empty());
        assert!((tri_area(pts, &idx) - 64.0).abs() < 1e-6, "área {}", tri_area(pts, &idx));
    }

    #[test]
    fn closing_point_is_ignored() {
        let mut closed = L_SHAPE.to_vec();
        closed.push(L_SHAPE[0]);
        let idx = triangulate(&closed);
        assert!((tri_area(&closed, &idx) - shoelace(&L_SHAPE)).abs() < 1e-9);
    }

    #[test]
    fn degenerate_polygon_has_no_triangles() {
        assert!(triangulate(&[(0.0, 0.0), (1.0, 1.0)]).is_empty());
        let line = [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)];
        assert!(tri_area(&line, &triangulate(&line)) < 1e-12);
    }
}
