use std::path::Path;

use thiserror::Error;

use crate::core::domain::driver::DriverDiffReport;
use crate::core::domain::error::RikuError;
use crate::core::domain::models::FileFormat;
use crate::core::domain::ports::GitRepository;
use crate::core::git::git_service::{GitError, GitService};
use crate::adapters::registry::get_driver_for;

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Riku(#[from] RikuError),
}

pub fn analyze_diff(
    repo_path: &Path,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
) -> Result<DriverDiffReport, AnalyzeError> {
    let svc = GitService::open(repo_path)?;
    analyze_diff_with_repo(&svc, commit_a, commit_b, file_path)
}

pub fn analyze_diff_with_repo<R: GitRepository + ?Sized>(
    repo: &R,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
) -> Result<DriverDiffReport, AnalyzeError> {
    let driver = match get_driver_for(file_path) {
        Some(driver) => driver,
        None => {
            let mut report = DriverDiffReport {
                file_type: FileFormat::Unknown,
                ..Default::default()
            };
            report.warnings.push(format!(
                "{file_path}: no hay driver disponible para este formato."
            ));
            return Ok(report);
        }
    };

    let mut warnings = Vec::new();
    let content_a = match repo.get_blob(commit_a, file_path) {
        Ok(bytes) => bytes,
        Err(GitError::LargeBlob { path, size }) => {
            warnings.push(format!(
                "{path} ({size} bytes) es demasiado grande; usando diff vacio."
            ));
            Vec::new()
        }
        Err(GitError::BlobNotFound { .. }) => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    let content_b = match repo.get_blob(commit_b, file_path) {
        Ok(bytes) => bytes,
        Err(GitError::LargeBlob { path, size }) => {
            warnings.push(format!(
                "{path} ({size} bytes) es demasiado grande; usando diff vacio."
            ));
            Vec::new()
        }
        Err(GitError::BlobNotFound { .. }) => Vec::new(),
        Err(err) => return Err(err.into()),
    };

    let mut report = driver.diff(&content_a, &content_b, file_path);
    report.warnings.extend(warnings);
    Ok(report)
}
