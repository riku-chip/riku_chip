//! DTOs y errores del subsistema Git.
//!
//! Estos tipos son datos puros (sin lógica) que viven en el dominio para que
//! `domain::ports::GitRepository` los pueda usar en su contrato sin depender
//! de la infraestructura concreta de `git/git_service.rs`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub oid: String,
    pub short_id: String,
    pub message: String,
    pub author: String,
    pub timestamp: i64,
}

/// Como `CommitInfo`, pero con los OIDs de los padres para distinguir merge
/// commits (más de un padre) y enlaces de historia. Solo lo emite el método
/// `get_commits_with_options`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitWithParents {
    pub info: CommitInfo,
    /// OIDs (formato hex) de los padres. Vacío para el commit root.
    pub parents: Vec<String>,
}

/// Filtros opcionales para recorrido de historia.
#[derive(Debug, Default, Clone)]
pub struct LogQuery<'a> {
    /// Si está, solo se incluyen commits que tocan ese archivo.
    pub file_path: Option<&'a str>,
    /// Límite duro de commits devueltos. `None` = sin límite.
    pub limit: Option<usize>,
    /// Si está, comienza desde ese ref/oid en lugar de `HEAD`.
    pub start: Option<&'a str>,
    /// Orden topológico (cada hijo antes que sus padres, empates por fecha),
    /// como `git log --graph`. Sin él, solo por fecha.
    pub topological: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: ChangeStatus,
    pub old_path: Option<String>,
}

/// Un commit y los archivos que cambió respecto a su primer padre (o a un
/// árbol vacío si es el commit inicial). Lo emite `commit_changes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitChanges {
    pub commit: CommitWithParents,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Added,
    Removed,
    Modified,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingChange {
    pub path: String,
    pub status: ChangeStatus,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub head_oid: String,
    pub head_short: String,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("no se encontro un repo Git desde {0}")]
    RepositoryNotFound(PathBuf),
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("commit no encontrado: {0}")]
    CommitNotFound(String),
    #[error("archivo no encontrado en commit {commit}: {path}")]
    BlobNotFound { commit: String, path: String },
    #[error("blob demasiado grande ({size} bytes) en {path}")]
    LargeBlob { path: String, size: usize },
}

pub const LARGE_BLOB_THRESHOLD: usize = 50 * 1024 * 1024;
