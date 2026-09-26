//! Contrato entre el núcleo y los drivers de formato.
//!
//! Un driver compara dos versiones de un archivo y devuelve un
//! [`FileChange`] con cambios tipados (ver `riku-kernel`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::domain::models::{DriverKind, FileChange, FileFormat};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverInfo {
    pub name: DriverKind,
    pub available: bool,
    pub version: String,
    pub extensions: Vec<String>,
}

pub trait RikuDriver: Send + Sync {
    fn info(&self) -> DriverInfo;

    fn diff(&self, content_a: &[u8], content_b: &[u8], path_hint: &str) -> FileChange;

    /// Formato que maneja el driver.
    fn format(&self) -> FileFormat;

    /// `true` si el contenido es de este formato, por su firma (sin mirar la
    /// extensión). El núcleo no conoce las firmas: pregunta a cada driver.
    fn detect(&self, content: &[u8]) -> bool;

    fn can_handle(&self, filename: &str) -> bool {
        let suffix = Path::new(filename)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let suffix = if suffix.is_empty() {
            String::new()
        } else {
            format!(".{suffix}")
        };
        self.info().extensions.iter().any(|ext| ext == &suffix)
    }
}
