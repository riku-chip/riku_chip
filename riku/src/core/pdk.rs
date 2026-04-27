//! Detección del PDK desde variables de entorno.
//!
//! Fuente única de verdad para resolver `$PDK_ROOT/$PDK/libs.tech/xschem`.
//! Consumido por `cli::doctor` (diagnóstico) y `adapters::xschem_driver`
//! (opciones de render y string de estado en `DriverInfo`).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdkStatus {
    /// `PDK_ROOT` o `PDK` no están en el entorno.
    NotConfigured,
    /// Ambos configurados, pero la ruta `<PDK_ROOT>/<PDK>/libs.tech/xschem`
    /// no existe en disco.
    Misconfigured(PathBuf),
    /// Ruta encontrada.
    Found(PathBuf),
}

pub fn pdk_status() -> PdkStatus {
    let (Some(root), Some(name)) = (std::env::var("PDK_ROOT").ok(), std::env::var("PDK").ok())
    else {
        return PdkStatus::NotConfigured;
    };
    let path = Path::new(&root).join(&name).join("libs.tech/xschem");
    if path.exists() {
        PdkStatus::Found(path)
    } else {
        PdkStatus::Misconfigured(path)
    }
}

/// Ruta de símbolos del PDK lista para inyectar al renderer, o `None` si el
/// PDK no está configurado o su ruta no existe. Atajo para los consumidores
/// que tratan "no configurado" y "mal configurado" igual.
pub fn pdk_symbol_path() -> Option<PathBuf> {
    match pdk_status() {
        PdkStatus::Found(p) => Some(p),
        _ => None,
    }
}
