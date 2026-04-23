use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

impl fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added => write!(f, "added"),
            Self::Removed => write!(f, "removed"),
            Self::Modified => write!(f, "modified"),
        }
    }
}

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub symbol: String,
    pub params: BTreeMap<String, String>,
    /// Conectividad pin → net (vacío si no se pudo resolver).
    /// Clave: nombre del pin (e.g. "DRAIN"), valor: nombre de net (e.g. "Vout").
    pub pins: BTreeMap<String, String>,
    pub x: f64,
    pub y: f64,
    pub rotation: i32,
    pub mirror: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Schematic {
    pub components: BTreeMap<String, Component>,
    pub wires: Vec<Wire>,
    pub nets: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentDiff {
    pub name: String,
    pub kind: ChangeKind,
    pub cosmetic: bool,
    /// true si la posición/rotación/mirror también cambió, independientemente de si hubo cambio semántico
    #[serde(default)]
    pub position_changed: bool,
    pub before: Option<BTreeMap<String, String>>,
    pub after: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DiffReport {
    pub components: Vec<ComponentDiff>,
    pub nets_added: Vec<String>,
    pub nets_removed: Vec<String>,
    pub is_move_all: bool,
}

impl DiffReport {
    pub fn is_empty(&self) -> bool {
        self.components.iter().all(|change| change.cosmetic)
            && self.nets_added.is_empty()
            && self.nets_removed.is_empty()
    }
}
