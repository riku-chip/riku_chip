use std::path::Path;

use thiserror::Error;

use crate::adapters::registry::get_driver_for;
use crate::core::analysis::blob_io;
use crate::core::domain::driver::DriverDiffReport;
use crate::core::domain::git_types::GitError;
use crate::core::domain::models::FileFormat;
use crate::core::domain::ports::GitRepository;
use crate::core::git::git_service::GitService;

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error(transparent)]
    Git(#[from] GitError),
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
    let content_a = blob_io::read_blob_lenient(repo, commit_a, file_path, &mut warnings)?
        .unwrap_or_default();
    let content_b = blob_io::read_blob_lenient(repo, commit_b, file_path, &mut warnings)?
        .unwrap_or_default();

    let mut report = driver.diff(&content_a, &content_b, file_path);
    report.warnings.extend(warnings);
    Ok(report)
}
