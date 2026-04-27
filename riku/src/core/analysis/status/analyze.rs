//! Entry points y pipeline por archivo de `riku status`.

use std::io;
use std::path::Path;

use crate::adapters::registry::get_driver_for;
use crate::core::analysis::blob_io;
use crate::core::analysis::pipeline;
use crate::core::analysis::summary::{DetailLevel, FileSummary};
use crate::core::domain::git_types::{ChangeStatus, WorkingChange};
use crate::core::domain::ports::{GitRepository, RepoRoot};
use crate::core::git::git_service::GitService;
use crate::core::path_matcher::PathMatcher;

use super::types::{StatusError, StatusOptions, StatusReport};

// ─── Entry points ────────────────────────────────────────────────────────────

/// Abre el repo desde path y aplica `StatusOptions`.
pub fn analyze_with_options_path(
    repo_path: &Path,
    opts: &StatusOptions,
) -> Result<StatusReport, StatusError> {
    let svc = GitService::open(repo_path)?;
    let workdir = svc.root().map(|p| p.to_path_buf());
    analyze_with_options(&svc, workdir.as_deref(), opts)
}

/// Versión inyectable con repo (para tests y composición).
pub fn analyze_with_options<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    opts: &StatusOptions,
) -> Result<StatusReport, StatusError> {
    let branch = repo.current_branch()?;
    let changes = repo.working_tree_changes()?;

    let matcher = PathMatcher::new(&opts.paths);

    let mut files = Vec::new();
    let mut warnings = Vec::new();

    for change in changes {
        if !matcher.matches(&change.path) {
            continue;
        }
        let summary = summarize_change(repo, workdir, &change, opts.level, &mut warnings);
        files.push(summary);
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(StatusReport {
        branch,
        files,
        warnings,
    })
}

// ─── Resumen por archivo ─────────────────────────────────────────────────────

fn summarize_change<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    change: &WorkingChange,
    level: DetailLevel,
    warnings: &mut Vec<String>,
) -> FileSummary {
    let driver = match get_driver_for(&change.path) {
        Some(d) => d,
        None => return FileSummary::unknown(&change.path),
    };

    // Contenido "antes": HEAD si el archivo existía allí; vacío si nuevo.
    let content_before = match change.status {
        ChangeStatus::Added => Vec::new(),
        _ => match blob_io::read_blob_lenient(repo, "HEAD", change.path.as_str(), warnings) {
            Ok(bytes) => bytes.unwrap_or_default(),
            Err(e) => return FileSummary::error(&change.path, e.to_string()),
        },
    };

    // Contenido "después": working tree desde disco (a menos que el archivo
    // esté eliminado, en cuyo caso es vacío).
    let content_after = match change.status {
        ChangeStatus::Removed => Vec::new(),
        _ => match read_workdir(workdir, &change.path) {
            Ok(bytes) => bytes,
            Err(e) => {
                warnings.push(format!("{}: no se pudo leer: {e}", change.path));
                Vec::new()
            }
        },
    };

    pipeline::summarize(&*driver, &content_before, &content_after, &change.path, level)
}

fn read_workdir(workdir: Option<&Path>, rel_path: &str) -> io::Result<Vec<u8>> {
    match workdir {
        Some(base) => std::fs::read(base.join(rel_path)),
        None => Ok(Vec::new()),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::types::EnvelopedStatusReport;
    use super::*;
    use crate::core::analysis::summary::SummaryCategory;
    use crate::core::domain::git_types::{BranchInfo, ChangedFile, CommitInfo, GitError};

    /// Repo mock que solo provee working_tree_changes y get_blob — suficiente
    /// para ejercitar `analyze_with_options` sin tocar disco real.
    struct MockRepo {
        changes: Vec<WorkingChange>,
        head_blobs: std::collections::HashMap<String, Vec<u8>>,
        branch: Option<BranchInfo>,
    }

    impl GitRepository for MockRepo {
        fn get_blob(&self, _commit_ish: &str, file_path: &str) -> Result<Vec<u8>, GitError> {
            self.head_blobs
                .get(file_path)
                .cloned()
                .ok_or_else(|| GitError::BlobNotFound {
                    commit: "HEAD".to_string(),
                    path: file_path.to_string(),
                })
        }
        fn get_commits(&self, _file_path: Option<&str>) -> Result<Vec<CommitInfo>, GitError> {
            Ok(Vec::new())
        }
        fn get_changed_files(&self, _: &str, _: &str) -> Result<Vec<ChangedFile>, GitError> {
            Ok(Vec::new())
        }
        fn working_tree_changes(&self) -> Result<Vec<WorkingChange>, GitError> {
            Ok(self.changes.clone())
        }
        fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
            Ok(self.branch.clone())
        }
    }

    #[test]
    fn archivo_sin_driver_se_marca_unknown() {
        let repo = MockRepo {
            changes: vec![WorkingChange {
                path: "Makefile".to_string(),
                status: ChangeStatus::Modified,
                old_path: None,
            }],
            head_blobs: Default::default(),
            branch: None,
        };
        let report = analyze_with_options(&repo, None, &StatusOptions::default()).unwrap();
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].category, SummaryCategory::Unknown);
        assert!(!report.has_semantic_changes());
    }

    #[test]
    fn lista_se_ordena_por_path() {
        let repo = MockRepo {
            changes: vec![
                WorkingChange {
                    path: "z.txt".into(),
                    status: ChangeStatus::Modified,
                    old_path: None,
                },
                WorkingChange {
                    path: "a.txt".into(),
                    status: ChangeStatus::Modified,
                    old_path: None,
                },
            ],
            head_blobs: Default::default(),
            branch: None,
        };
        let report = analyze_with_options(&repo, None, &StatusOptions::default()).unwrap();
        assert_eq!(report.files[0].path, "a.txt");
        assert_eq!(report.files[1].path, "z.txt");
    }

    #[test]
    fn paths_filtra_por_glob() {
        let repo = MockRepo {
            changes: vec![
                WorkingChange {
                    path: "amp_ota.sch".into(),
                    status: ChangeStatus::Modified,
                    old_path: None,
                },
                WorkingChange {
                    path: "filtro.sch".into(),
                    status: ChangeStatus::Modified,
                    old_path: None,
                },
                WorkingChange {
                    path: "Makefile".into(),
                    status: ChangeStatus::Modified,
                    old_path: None,
                },
            ],
            head_blobs: Default::default(),
            branch: None,
        };
        let opts = StatusOptions {
            level: DetailLevel::Resumen,
            paths: vec!["amp_*.sch".to_string()],
        };
        let report = analyze_with_options(&repo, None, &opts).unwrap();
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].path, "amp_ota.sch");
    }

    #[test]
    fn rama_se_propaga_al_reporte() {
        let repo = MockRepo {
            changes: vec![],
            head_blobs: Default::default(),
            branch: Some(BranchInfo {
                name: "feature-amp".into(),
                head_oid: "0".repeat(40),
                head_short: "0000000".into(),
                upstream: None,
                ahead: 0,
                behind: 0,
            }),
        };
        let report = analyze_with_options(&repo, None, &StatusOptions::default()).unwrap();
        assert_eq!(
            report.branch.as_ref().map(|b| b.name.as_str()),
            Some("feature-amp")
        );
    }

    // ── Tests del contrato JSON (schema riku-status/v1) ──────────────────

    fn fixture_report() -> StatusReport {
        StatusReport {
            branch: Some(BranchInfo {
                name: "feature-amp".into(),
                head_oid: "abcdef0123456789abcdef0123456789abcdef01".into(),
                head_short: "abcdef0".into(),
                upstream: Some("origin/feature-amp".into()),
                ahead: 3,
                behind: 0,
            }),
            files: vec![FileSummary::unknown("Makefile")],
            warnings: vec![],
        }
    }

    #[test]
    fn json_envelope_lleva_schema_versionado() {
        let report = fixture_report();
        let env = EnvelopedStatusReport::from(&report);
        let s = serde_json::to_string(&env).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["schema"], "riku-status/v1");
    }

    #[test]
    fn json_contiene_claves_principales() {
        let report = fixture_report();
        let env = EnvelopedStatusReport::from(&report);
        let v: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert!(v.get("schema").is_some());
        assert!(v.get("branch").is_some());
        assert!(v.get("files").is_some());
        assert!(v.get("warnings").is_some());
        let branch = &v["branch"];
        assert_eq!(branch["name"], "feature-amp");
        assert_eq!(branch["head_short"], "abcdef0");
        assert_eq!(branch["upstream"], "origin/feature-amp");
        assert_eq!(branch["ahead"], 3);
    }

    #[test]
    fn json_omite_campos_vacios_para_estabilidad() {
        // Un FileSummary sin details/full_report/errors no debe inflar la salida
        // con campos null/vacíos — `skip_serializing_if` los oculta.
        let report = fixture_report();
        let v = serde_json::to_value(EnvelopedStatusReport::from(&report)).unwrap();
        let file = &v["files"][0];
        assert!(
            file.get("details").is_none(),
            "details vacío no debe aparecer"
        );
        assert!(
            file.get("full_report").is_none(),
            "full_report ausente no debe aparecer"
        );
        assert!(
            file.get("errors").is_none(),
            "errors vacío no debe aparecer"
        );
    }
}
