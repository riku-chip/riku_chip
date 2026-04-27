//! Modelo público de `riku status`: errores, schema/envelope, reporte y opciones.
//!
//! La orquestación que llena estos tipos vive en [`super::analyze`].

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::analysis::envelope::Envelope;
use crate::core::analysis::summary::{DetailLevel, FileSummary, SummaryCategory};
use crate::core::domain::git_types::{BranchInfo, GitError};

// ─── Errores ─────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum StatusError {
    #[error(transparent)]
    Git(#[from] GitError),
}

// ─── Schema / envelope ───────────────────────────────────────────────────────

/// Identificador del schema JSON de `riku status`. Versionado a propósito:
/// cambios incompatibles bumpan el sufijo (`v1` → `v2`); cambios compatibles
/// (campos nuevos opcionales) no.
pub const STATUS_SCHEMA: &str = "riku-status/v1";

/// Wrapper público para serialización con `schema` siempre presente.
///
/// Usar `EnvelopedStatusReport::from(report)` antes de pasar a `serde_json`.
pub type EnvelopedStatusReport<'a> = Envelope<'a, StatusReport>;

impl<'a> From<&'a StatusReport> for EnvelopedStatusReport<'a> {
    fn from(inner: &'a StatusReport) -> Self {
        Envelope::new(STATUS_SCHEMA, inner)
    }
}

// ─── Reporte ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusReport {
    pub branch: Option<BranchInfo>,
    pub files: Vec<FileSummary>,
    /// Mensajes informativos no fatales (blob omitido por tamaño, etc.).
    pub warnings: Vec<String>,
}

impl StatusReport {
    pub fn has_semantic_changes(&self) -> bool {
        self.files
            .iter()
            .any(|f| matches!(f.category, SummaryCategory::Semantic))
    }

    pub fn count_by_category(&self, cat: SummaryCategory) -> usize {
        self.files.iter().filter(|f| f.category == cat).count()
    }
}

// ─── Opciones ────────────────────────────────────────────────────────────────

/// Configuración para `analyze_with_options`.
///
/// `paths` admite globs simples estilo gitignore (`amp_*.sch`, `**/*.sch`).
/// Vacío significa "no filtrar".
#[derive(Clone, Debug, Default)]
pub struct StatusOptions {
    pub level: DetailLevel,
    pub paths: Vec<String>,
}
