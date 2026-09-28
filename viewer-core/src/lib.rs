//! Abstracciones neutras para visores de layout y esquemático.
//!
//! Este crate define el **contrato común** que cumplen los backends específicos
//! (`xschem-viewer`, `riku-mod-layout`, …) para que los consumidores (`riku-gui`,
//! `riku` CLI) puedan integrarlos sin acoplarse al formato.
//!
//! # Tipos principales
//!
//! - [`DrawElement`] — primitivas de dibujo neutras (Line, Rect, Circle, Polygon, Text).
//! - [`BoundingBox`] — caja envolvente en coordenadas de mundo (`f64`).
//! - [`Scene`] / [`RenderableScene`] — escena renderizable, eager o perezosa.
//! - [`Viewport`] — pan + zoom isotrópico, agnóstico del sentido del eje Y.
//! - [`YAxis`] — sentido del eje Y del mundo de una escena (Xschem Y-down, GDS Y-up).
//! - [`LayerPaint`] / [`Rgba`] — estilo por capa que una escena puede proveer.
//! - [`ChangeItem`] — cambio listable de una escena de diff.
//! - [`ViewerBackend`] — trait asíncrono que implementa cada formato concreto.
//! - [`FileSource`] / [`DiffFiles`] — otros archivos de la misma versión
//!   (commit o disco), para formatos repartidos en varios archivos.
//! - [`ViewerError`] — errores unificados, incluyendo `Cancelled` y `Join`.
//!
//! Los tipos ricos específicos de un formato (p.ej. `MissingSymbol` de Xschem)
//! viven en el crate del backend correspondiente, no aquí.

pub mod backend;
pub mod bbox;
pub mod diff;
pub mod element;
pub mod error;
pub mod files;
pub mod fill;
pub mod index;
pub mod paint;
pub mod scene;
pub mod viewport;

pub use backend::{BackendInfo, ViewerBackend};
pub use bbox::BoundingBox;
pub use diff::{Annotation, AnnotationShape, ChangeItem, ChangeKind};
pub use element::{DrawElement, HAlign, Layer, VAlign};
pub use error::{Result, ViewerError};
pub use files::{DiffFiles, DiskFiles, FileSource};
pub use index::{CoverageLayer, CoverageView, Fill, LodQuery, SceneIndex, Visible};
pub use paint::{LayerPaint, Rgba};
pub use scene::{NetHit, NetProbe, RenderableScene, Scene, SceneHandle, TextStyle, ViewEntry};
pub use viewport::{screen_to_world, world_to_screen, Viewport, YAxis};

// Re-export del token para que los backends no necesiten depender explícitamente
// de `tokio-util` solo para el tipo.
pub use tokio_util::sync::CancellationToken;
