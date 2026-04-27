//! Orquestación de `riku log`.
//!
//! Recorre el historial Git y, por cada commit, computa un resumen semántico
//! (igual que `status` para el working tree, pero entre `parent..commit`).
//! Como `status`, no formatea — entrega `LogReport` y la capa CLI elige texto
//! o JSON.

mod types;
mod walk;

pub use types::{EnvelopedLogReport, LogCommit, LogError, LogOptions, LogReport, LOG_SCHEMA};
pub use walk::{analyze_with_options_path, walk_with_summary};
