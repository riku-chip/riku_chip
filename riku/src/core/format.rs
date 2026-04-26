//! Detección de formato de archivo.
//!
//! Inspecciona el header del contenido para decidir a qué driver enrutar.
//! Solo lee los primeros bytes — es seguro contra archivos grandes o binarios.

use crate::core::domain::models::FileFormat;

/// Identifica el formato del contenido por heurísticas de cabecera.
pub fn detect_format(content: &[u8]) -> FileFormat {
    let header = String::from_utf8_lossy(&content[..content.len().min(240)]);
    if header.contains("xschem version=") {
        FileFormat::Xschem
    } else if header.contains("<Qucs Schematic") {
        FileFormat::Qucs
    } else if header.contains("EESchema Schematic File Version") {
        FileFormat::KicadLegacy
    } else {
        FileFormat::Unknown
    }
}
