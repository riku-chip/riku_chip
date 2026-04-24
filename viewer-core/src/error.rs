//! Errores neutros para cualquier backend de visor.
//!
//! Los errores específicos de cada backend (parse de Xschem, GDS, ...) deben
//! convertirse a estas variantes en el límite del adaptador. Así `riku-gui` y
//! `riku` CLI solo necesitan conocer `ViewerError`.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ViewerError {
    /// Error al parsear el contenido del archivo.
    #[error("parse error: {0}")]
    Parse(String),

    /// Error de I/O al leer o escribir un recurso.
    #[error("io error: {0}")]
    Io(String),

    /// El backend no soporta la operación solicitada.
    #[error("unsupported operation: {0}")]
    Unsupported(String),

    /// La tarea async fue cancelada cooperativamente vía `CancellationToken`.
    #[error("operation cancelled")]
    Cancelled,

    /// Una tarea `spawn_blocking` o `spawn` falló al hacer `join`.
    /// Contiene el mensaje formateado de `tokio::task::JoinError`.
    #[error("task join error: {0}")]
    Join(String),

    /// Error específico del backend que no encaja en las variantes anteriores.
    #[error("backend error: {0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, ViewerError>;

impl From<tokio::task::JoinError> for ViewerError {
    fn from(e: tokio::task::JoinError) -> Self {
        ViewerError::Join(e.to_string())
    }
}

impl From<std::io::Error> for ViewerError {
    fn from(e: std::io::Error) -> Self {
        ViewerError::Io(e.to_string())
    }
}
