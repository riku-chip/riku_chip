//! Viewport 2D: pan + zoom isotrópico.
//!
//! No asume sentido del eje Y. El eje se escoge en el sitio de integración (por
//! ejemplo `riku-gui` aplica Y-down al pasar a egui). Las funciones libres
//! `world_to_screen` / `screen_to_world` operan en el mismo sistema que el pan.

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

/// Mundo → pantalla, usando el viewport dado.
pub fn world_to_screen(vp: &Viewport, x: f64, y: f64) -> (f64, f64) {
    (x * vp.scale + vp.pan_x, y * vp.scale + vp.pan_y)
}

/// Pantalla → mundo, inverso de `world_to_screen`.
pub fn screen_to_world(vp: &Viewport, sx: f64, sy: f64) -> (f64, f64) {
    ((sx - vp.pan_x) / vp.scale, (sy - vp.pan_y) / vp.scale)
}
