use thiserror::Error;

use crate::core::git::git_service::GitError;

#[derive(Debug, Error)]
pub enum RikuError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("formato no soportado: {0}")]
    UnsupportedFormat(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("render error: {0}")]
    Render(String),
}
