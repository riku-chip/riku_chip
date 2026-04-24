//! Trait asíncrono que cualquier backend de visor debe implementar.
//!
//! El contrato es intencionalmente estrecho:
//!
//! - `info()` — identifica al backend (nombre, formatos, versión).
//! - `accepts()` — decide si el backend puede procesar un blob dado.
//! - `load()` — parsea y construye una [`SceneHandle`] lista para renderizar.
//!
//! `async_trait` es obligatorio porque necesitamos `Box<dyn ViewerBackend>` y
//! Rust aún no soporta `async fn` en traits con objetos dinámicos de forma nativa.

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::error::Result;
use crate::scene::SceneHandle;

/// Metadatos de un backend — devueltos por `info()` para UI y diagnóstico.
#[derive(Debug, Clone)]
pub struct BackendInfo {
    /// Identificador corto (`"xschem"`, `"gds"`).
    pub name: &'static str,
    /// Versión del backend (útil para doctor/diagnóstico).
    pub version: &'static str,
    /// Extensiones aceptadas, sin punto (`["sch", "sym"]`).
    pub extensions: &'static [&'static str],
}

/// Trait principal. Un backend típico lo implementa como unit struct sin estado;
/// cualquier cache compartida debe vivir dentro de `Arc<...>` interno.
#[async_trait]
pub trait ViewerBackend: Send + Sync {
    fn info(&self) -> BackendInfo;

    /// ¿Puede este backend procesar el contenido dado? La decisión puede basarse
    /// en firma mágica, cabecera de texto o la extensión del path si se provee.
    fn accepts(&self, content: &[u8], path_hint: Option<&str>) -> bool;

    /// Parsea y construye una escena renderizable.
    ///
    /// `token` permite cancelación cooperativa — los backends que hagan trabajo
    /// CPU-bound pesado deben poll-earlo en puntos razonables (entre fases de
    /// parseo, entre celdas, etc.) y retornar [`ViewerError::Cancelled`] cuando
    /// corresponda.
    ///
    /// Se espera que las implementaciones ejecuten el trabajo pesado dentro de
    /// `tokio::task::spawn_blocking` y propaguen el `JoinError` como
    /// [`ViewerError::Join`].
    async fn load(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        token: CancellationToken,
    ) -> Result<SceneHandle>;
}
