//! Modelo público de `riku log`: errores, schema/envelope, reporte y opciones.
//!
//! La orquestación que llena estos tipos vive en [`super::walk`].

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::analysis::envelope::Envelope;
use crate::core::analysis::summary::{DetailLevel, FileSummary, SummaryCategory};
use crate::core::domain::git_types::{CommitInfo, GitError};

// ─── Errores ─────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum LogError {
    #[error(transparent)]
    Git(#[from] GitError),
}

// ─── Schema / envelope ───────────────────────────────────────────────────────

pub const LOG_SCHEMA: &str = "riku-log/v1";

pub type EnvelopedLogReport<'a> = Envelope<'a, LogReport>;

impl<'a> From<&'a LogReport> for EnvelopedLogReport<'a> {
    fn from(inner: &'a LogReport) -> Self {
        Envelope::new(LOG_SCHEMA, inner)
    }
}

// ─── Reporte ─────────────────────────────────────────────────────────────────

/// Un commit anotado con su resumen semántico por archivo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogCommit {
    #[serde(flatten)]
    pub info: CommitInfo,
    /// OIDs de los padres (1 normal, 0 root, 2+ merge).
    pub parents: Vec<String>,
    /// Refs que apuntan exactamente a este commit (rama, tag, HEAD).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub refs: Vec<String>,
    /// `true` si tiene más de un padre. Implica `files` vacío en v1.
    pub is_merge: bool,
    /// Resumen por archivo. Vacío en commits root o merge en v1.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub files: Vec<FileSummary>,
}

impl LogCommit {
    pub fn has_semantic_changes(&self) -> bool {
        self.files
            .iter()
            .any(|f| matches!(f.category, SummaryCategory::Semantic))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogReport {
    pub commits: Vec<LogCommit>,
    pub warnings: Vec<String>,
}

// ─── Opciones ────────────────────────────────────────────────────────────────

/// Configuración para `walk_with_summary` y variantes.
#[derive(Clone, Debug, Default)]
pub struct LogOptions {
    pub level: DetailLevel,
    /// Filtra commits que tocan al menos uno de estos paths exactos.
    pub paths: Vec<String>,
    pub limit: Option<usize>,
    /// Ref/oid de inicio. `None` = HEAD.
    pub start: Option<String>,
}
