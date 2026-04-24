//! Abstracciones neutras para visores de layout y esquemático.
//!
//! Este crate define el **contrato común** que cumplen los backends específicos
//! (`xschem-viewer`, `gds-renderer`, …) para que los consumidores (`riku-gui`,
//! `riku` CLI) puedan integrarlos sin acoplarse al formato.
//!
//! # Tipos principales
//!
//! - [`DrawElement`] — primitivas de dibujo neutras (Line, Rect, Circle, Polygon, Text).
//! - [`BoundingBox`] — caja envolvente en coordenadas de mundo (`f64`).
//! - [`Scene`] / [`RenderableScene`] — escena renderizable, eager o perezosa.
//! - [`Viewport`] — pan + zoom isotrópico, agnóstico del sentido del eje Y.
//! - [`ViewerBackend`] — trait asíncrono que implementa cada formato concreto.
//! - [`ViewerError`] — errores unificados, incluyendo `Cancelled` y `Join`.
//!
//! Los tipos ricos específicos de un formato (p.ej. `MissingSymbol` de Xschem)
//! viven en el crate del backend correspondiente, no aquí.

pub mod backend;
pub mod bbox;
pub mod element;
pub mod error;
pub mod scene;
pub mod viewport;

pub use backend::{BackendInfo, ViewerBackend};
pub use bbox::BoundingBox;
pub use element::{DrawElement, HAlign, Layer, VAlign};
pub use error::{Result, ViewerError};
pub use scene::{RenderableScene, Scene, SceneHandle};
pub use viewport::{screen_to_world, world_to_screen, Viewport};

// Re-export del token para que los backends no necesiten depender explícitamente
// de `tokio-util` solo para el tipo.
pub use tokio_util::sync::CancellationToken;
