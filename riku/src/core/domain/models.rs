//! Modelos de dominio de riku.
//!
//! Los tipos semánticos (componentes, schematic, diff) se re-exportan desde
//! `xschem_viewer::semantic` para que cualquier proyecto que use esa
//! librería consuma exactamente los mismos tipos — riku es un consumidor
//! más. Aquí solo se definen los tipos propios de riku (formatos soportados,
//! drivers, etc.) que no tienen sentido fuera de un VCS multi-formato.

use std::fmt;

use serde::{Deserialize, Serialize};

// ─── Re-exports desde xschem-viewer ──────────────────────────────────────────

pub use xschem_viewer::semantic::{
    ChangeKind, ComponentDiff, DiffReport,
    SemanticComponent as Component,
    SemanticSchematic as Schematic,
    SemanticWire as Wire,
};

// ─── Tipos propios de riku ───────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    Xschem,
    Qucs,
    #[serde(rename = "kicad_legacy")]
    KicadLegacy,
    Unknown,
}

impl Default for FileFormat {
    fn default() -> Self {
        Self::Unknown
    }
}

impl fmt::Display for FileFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xschem => write!(f, "xschem"),
            Self::Qucs => write!(f, "qucs"),
            Self::KicadLegacy => write!(f, "kicad_legacy"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriverKind {
    Xschem,
}

impl fmt::Display for DriverKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xschem => write!(f, "xschem"),
        }
    }
}
