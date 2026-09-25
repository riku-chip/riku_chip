//! Viewport 2D: pan + zoom isotrópico.
//!
//! No asume sentido del eje Y. El eje se escoge en el sitio de integración (por
//! ejemplo `riku-gui` aplica Y-down al pasar a egui). Las funciones libres
//! `world_to_screen` / `screen_to_world` operan en el mismo sistema que el pan.
//! Para escenas Y-up, el consumidor pasa las coordenadas por [`YAxis::flip_y`]
//! antes de proyectarlas.

use serde::{Deserialize, Serialize};

use crate::bbox::BoundingBox;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub pan_x: f64,
    pub pan_y: f64,
    /// Factor de escala: pixeles por unidad de mundo. Siempre > 0.
    pub scale: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self { pan_x: 0.0, pan_y: 0.0, scale: 1.0 }
    }
}

impl Viewport {
    /// Ajusta pan y zoom para que `bbox` quepa en `(width_px, height_px)` con
    /// un margen del 10% alrededor. Si la bbox está vacía, no cambia nada.
    pub fn fit_to(&mut self, bbox: &BoundingBox, width_px: f64, height_px: f64) {
        if bbox.is_empty() || width_px <= 0.0 || height_px <= 0.0 {
            return;
        }
        let w = bbox.width().max(1e-9);
        let h = bbox.height().max(1e-9);
        let sx = width_px / w;
        let sy = height_px / h;
        self.scale = sx.min(sy) * 0.9; // 10% margen total
        let (cx, cy) = bbox.center();
        self.pan_x = width_px * 0.5 - cx * self.scale;
        self.pan_y = height_px * 0.5 - cy * self.scale;
    }

    /// Zoom centrado en un punto de pantalla (suele ser la posición del cursor),
    /// preservando el punto de mundo que estaba bajo ese pixel.
    pub fn zoom_at(&mut self, factor: f64, cursor_sx: f64, cursor_sy: f64) {
        if factor <= 0.0 || !factor.is_finite() {
            return;
        }
        let world_x = (cursor_sx - self.pan_x) / self.scale;
        let world_y = (cursor_sy - self.pan_y) / self.scale;
        self.scale *= factor;
        self.pan_x = cursor_sx - world_x * self.scale;
        self.pan_y = cursor_sy - world_y * self.scale;
    }

    pub fn pan_by_screen(&mut self, dpx: f64, dpy: f64) {
        self.pan_x += dpx;
        self.pan_y += dpy;
    }

    pub fn pan_by_world(&mut self, dx: f64, dy: f64) {
        self.pan_x += dx * self.scale;
        self.pan_y += dy * self.scale;
    }
}

/// Sentido del eje Y en las coordenadas de mundo de una escena.
///
/// El `Viewport` trabaja en un espacio "de vista" con Y hacia abajo (como las
/// pantallas). Una escena Y-up (GDS) se refleja al entrar a ese espacio; una
/// escena Y-down (Xschem) pasa tal cual. El reflejo es una involución: la
/// misma función sirve para ir y volver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum YAxis {
    /// Y crece hacia abajo (esquemáticos, coordenadas de pantalla).
    #[default]
    Down,
    /// Y crece hacia arriba (layouts GDS, convención matemática).
    Up,
}

impl YAxis {
    /// Mundo ↔ vista para una coordenada Y.
    pub fn flip_y(self, y: f64) -> f64 {
        match self {
            Self::Down => y,
            Self::Up => -y,
        }
    }

    /// Mundo ↔ vista para una bbox (reordena min/max al reflejar).
    pub fn flip_bbox(self, bb: &BoundingBox) -> BoundingBox {
        match self {
            Self::Down => *bb,
            Self::Up if bb.is_empty() => *bb,
            Self::Up => BoundingBox { min_y: -bb.max_y, max_y: -bb.min_y, ..*bb },
        }
    }
}

/// Mundo → pantalla, usando el viewport dado.
pub fn world_to_screen(vp: &Viewport, x: f64, y: f64) -> (f64, f64) {
    (x * vp.scale + vp.pan_x, y * vp.scale + vp.pan_y)
}

/// Pantalla → mundo, inverso de `world_to_screen`.
pub fn screen_to_world(vp: &Viewport, sx: f64, sy: f64) -> (f64, f64) {
    ((sx - vp.pan_x) / vp.scale, (sy - vp.pan_y) / vp.scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn y_down_is_identity() {
        let bb = BoundingBox::from_points((0.0, 1.0), (4.0, 3.0));
        assert_eq!(YAxis::Down.flip_y(2.5), 2.5);
        assert_eq!(YAxis::Down.flip_bbox(&bb), bb);
    }

    #[test]
    fn y_up_flip_is_involution_and_keeps_bbox_ordered() {
        let bb = BoundingBox::from_points((0.0, 1.0), (4.0, 3.0));
        let v = YAxis::Up.flip_bbox(&bb);
        assert_eq!((v.min_y, v.max_y), (-3.0, -1.0));
        assert_eq!((v.min_x, v.max_x), (0.0, 4.0));
        assert_eq!(YAxis::Up.flip_bbox(&v), bb);
        assert_eq!(YAxis::Up.flip_y(YAxis::Up.flip_y(7.0)), 7.0);
    }

    #[test]
    fn y_up_flip_keeps_empty_bbox_empty() {
        assert!(YAxis::Up.flip_bbox(&BoundingBox::empty()).is_empty());
    }

    #[test]
    fn fit_to_centers_bbox_in_local_rect() {
        let mut vp = Viewport::default();
        let bb = BoundingBox::from_points((0.0, 0.0), (10.0, 10.0));
        vp.fit_to(&bb, 200.0, 100.0);
        let (cx, cy) = world_to_screen(&vp, 5.0, 5.0);
        assert!((cx - 100.0).abs() < 1e-9 && (cy - 50.0).abs() < 1e-9);
    }
}
