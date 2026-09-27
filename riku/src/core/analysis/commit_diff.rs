//! Diff de un archivo entre dos commits.
//!
//! El núcleo no elige formatos: el módulo sale del [`Registry`] que recibe
//! (por la extensión del archivo).

use std::path::Path;

use riku_kernel::{DiffOptions, Registry};
use thiserror::Error;

use crate::core::analysis::{blob_io, pipeline};
use crate::core::domain::git_types::GitError;
use crate::core::domain::models::{FileChange, FileFormat};
use crate::core::domain::ports::GitRepository;
use crate::core::git::git_service::GitService;

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error(transparent)]
    Git(#[from] GitError),
}

/// Abre el repo en `repo_path` y compara `file_path` entre los dos commits.
pub fn analyze_diff(
    repo_path: &Path,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
    modules: &Registry,
    opts: &DiffOptions,
) -> Result<FileChange, AnalyzeError> {
    let svc = GitService::open(repo_path)?;
    analyze_diff_with_repo(&svc, commit_a, commit_b, file_path, modules, opts)
}

/// Como [`analyze_diff`], con el repositorio inyectado (tests, composición).
/// Un archivo sin módulo da un `FileChange` vacío con un aviso.
pub fn analyze_diff_with_repo<R: GitRepository + ?Sized>(
    repo: &R,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
    modules: &Registry,
    opts: &DiffOptions,
) -> Result<FileChange, AnalyzeError> {
    let Some(module) = modules.for_path(file_path) else {
        let mut report = FileChange::new(FileFormat::Unknown);
        report.warnings.push(format!("{file_path}: no hay driver disponible para este formato."));
        return Ok(report);
    };

    let content_a = blob_io::read_blob(repo, commit_a, file_path)?;
    let content_b = blob_io::read_blob(repo, commit_b, file_path)?;
    let files = crate::core::git::files::between(repo, Some(commit_a), Some(commit_b));
    Ok(pipeline::diff_blobs(module.as_ref(), &content_a, &content_b, file_path, opts, &files))
}
