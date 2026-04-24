//! Bounding box neutral en coordenadas de mundo.
//!
//! Usa `f64` para cubrir GDS (coordenadas en unidades de base que pueden superar
//! `i32::MAX`) sin pérdida de precisión. El sentido del eje Y no se asume aquí
//! — es responsabilidad del viewport.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl BoundingBox {
    /// Caja vacía (acumulador). `is_empty()` retorna true hasta el primer expand.
    pub fn empty() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    pub fn point(x: f64, y: f64) -> Self {
        Self { min_x: x, min_y: y, max_x: x, max_y: y }
    }

    pub fn from_points(p1: (f64, f64), p2: (f64, f64)) -> Self {
        Self {
            min_x: p1.0.min(p2.0),
            min_y: p1.1.min(p2.1),
            max_x: p1.0.max(p2.0),
            max_y: p1.1.max(p2.1),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y
    }

    pub fn width(&self) -> f64 {
        (self.max_x - self.min_x).max(0.0)
    }

    pub fn height(&self) -> f64 {
        (self.max_y - self.min_y).max(0.0)
    }

    pub fn center(&self) -> (f64, f64) {
        ((self.min_x + self.max_x) * 0.5, (self.min_y + self.max_y) * 0.5)
    }

    pub fn expand_point(&mut self, x: f64, y: f64) {
        if x < self.min_x { self.min_x = x; }
        if y < self.min_y { self.min_y = y; }
        if x > self.max_x { self.max_x = x; }
        if y > self.max_y { self.max_y = y; }
    }

    pub fn expand(&mut self, other: &BoundingBox) {
        if other.is_empty() { return; }
        if other.min_x < self.min_x { self.min_x = other.min_x; }
        if other.min_y < self.min_y { self.min_y = other.min_y; }
        if other.max_x > self.max_x { self.max_x = other.max_x; }
        if other.max_y > self.max_y { self.max_y = other.max_y; }
    }

    pub fn inflate(&self, margin: f64) -> Self {
        Self {
            min_x: self.min_x - margin,
            min_y: self.min_y - margin,
            max_x: self.max_x + margin,
            max_y: self.max_y + margin,
        }
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}
