use std::fmt;

use serde::{Deserialize, Serialize};

/// Formato de un archivo de diseño.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    Xschem,
    Qucs,
    #[serde(rename = "kicad_legacy")]
    KicadLegacy,
    /// Layouts: GDSII y OASIS.
    Gds,
    #[default]
    Unknown,
}

impl fmt::Display for FileFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Xschem => "xschem",
            Self::Qucs => "qucs",
            Self::KicadLegacy => "kicad_legacy",
            Self::Gds => "gds",
            Self::Unknown => "unknown",
        })
    }
}
