use std::fmt;

use serde::{Deserialize, Serialize};

/// Formato de un archivo de diseño.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    Xschem,
    /// Layouts: GDSII y OASIS.
    Gds,
    /// Resultados de simulación SPICE (`.raw` de ngspice): formas de onda.
    Waveform,
    #[default]
    Unknown,
}

impl fmt::Display for FileFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Xschem => "xschem",
            Self::Gds => "gds",
            Self::Waveform => "waveform",
            Self::Unknown => "unknown",
        })
    }
}
