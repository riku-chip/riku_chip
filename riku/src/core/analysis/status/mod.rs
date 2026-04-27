//! Orquestación de `riku status`.
//!
//! Composición de `GitRepository` (working tree + HEAD) con `RikuDriver` para
//! producir una lista de `FileSummary` clasificados.
//!
//! Este módulo no formatea — entrega `StatusReport` y la capa CLI decide cómo
//! presentarlo (texto, JSON, ...).

mod analyze;
mod types;

pub use analyze::{analyze_with_options, analyze_with_options_path};
pub use types::{
    EnvelopedStatusReport, StatusError, StatusOptions, StatusReport, STATUS_SCHEMA,
};
