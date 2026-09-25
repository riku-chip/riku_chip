//! Estilo de pintado por capa, neutro respecto al toolkit de UI.
//!
//! Un backend puede adjuntar a su escena una tabla `Layer → LayerPaint` para
//! que el visor genérico pinte con los colores del formato (p.ej. paleta PDK
//! de un GDS). Si la escena no provee paint para una capa, el consumidor usa
//! su paleta neutral.

use serde::{Deserialize, Serialize};

/// Color RGBA de 8 bits por canal. `a = 255` es opaco.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }
}

/// Cómo pintar los elementos de una capa.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerPaint {
    /// Nombre legible (p.ej. `"met1 68/20"`), útil para leyendas y tooltips.
    pub name: String,
    /// Relleno de primitivas `filled`. Suele ser semitransparente para que las
    /// capas superpuestas sigan viéndose.
    pub fill: Rgba,
    /// Contorno (y color de líneas/texto de la capa).
    pub stroke: Rgba,
}
