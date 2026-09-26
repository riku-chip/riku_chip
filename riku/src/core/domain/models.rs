//! Modelos de dominio de riku.
//!
//! Los tipos de cambio (`FileChange`, `Change`, `Element`, `ChangeKind`…) y
//! `FileFormat` son del núcleo (`riku-kernel`): el dominio ya no depende de
//! los tipos de ningún formato. Aquí solo quedan los tipos propios del VCS.

use std::fmt;

use serde::{Deserialize, Serialize};

pub use riku_kernel::{Bounds, Change, ChangeKind, Detail, Element, FileChange, FileFormat, Value, Via};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriverKind {
    Xschem,
    Gds,
}

impl fmt::Display for DriverKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Xschem => write!(f, "xschem"),
            Self::Gds => write!(f, "gds"),
        }
    }
}
