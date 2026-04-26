//! Orquestación de `riku status`.
//!
//! Composición de `GitRepository` (working tree + HEAD) con `RikuDriver` para
//! producir una lista de `FileSummary` clasificados.
//!
//! Este módulo no formatea — entrega `StatusReport` y la capa CLI decide cómo
//! presentarlo (texto, JSON, ...).

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::adapters::registry::get_driver_for;
use crate::core::analysis::blob_io;
use crate::core::analysis::summary::{DetailLevel, FileSummary, SummaryCategory};
use crate::core::domain::git_types::{BranchInfo, ChangeStatus, GitError, WorkingChange};
use crate::core::domain::ports::{GitRepository, RepoRoot};
use crate::core::git::git_service::GitService;
use crate::core::path_matcher::PathMatcher;

// ─── Errores ─────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum StatusError {
    #[error(transparent)]
    Git(#[from] GitError),
}

// ─── Modelo de salida ────────────────────────────────────────────────────────

/// Identificador del schema JSON de `riku status`. Versionado a propósito:
/// cambios incompatibles bumpan el sufijo (`v1` → `v2`); cambios compatibles
/// (campos nuevos opcionales) no.
pub const STATUS_SCHEMA: &str = "riku-status/v1";

/// Wrapper público para serialización con `schema` siempre presente.
///
/// Usar `EnvelopedStatusReport::from(report)` antes de pasar a `serde_json`.
#[derive(Clone, Debug, Serialize)]
pub struct EnvelopedStatusReport<'a> {
    pub schema: &'static str,
    #[serde(flatten)]
    pub inner: &'a StatusReport,
}

impl<'a> From<&'a StatusReport> for EnvelopedStatusReport<'a> {
    fn from(inner: &'a StatusReport) -> Self {
        Self {
            schema: STATUS_SCHEMA,
            inner,
        }
    }
}

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

// ─── Entry points ────────────────────────────────────────────────────────────

/// Helper que abre el repo y delega en `analyze_with_options` con defaults.
pub fn analyze(repo_path: &Path) -> Result<StatusReport, StatusError> {
    analyze_with_options_path(repo_path, &StatusOptions::default())
}

/// Helper que abre el repo desde path y aplica `StatusOptions`.
pub fn analyze_with_options_path(
    repo_path: &Path,
    opts: &StatusOptions,
) -> Result<StatusReport, StatusError> {
    let svc = GitService::open(repo_path)?;
    let workdir = svc.root().map(|p| p.to_path_buf());
    analyze_with_options(&svc, workdir.as_deref(), opts)
}

/// Versión inyectable sin opciones (compatibilidad con Fase 1 / tests).
pub fn analyze_with_repo<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
) -> Result<StatusReport, StatusError> {
    analyze_with_options(repo, workdir, &StatusOptions::default())
}

/// Versión inyectable con opciones.
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

    let report = driver.diff(&content_before, &content_after, &change.path);
    FileSummary::from_report_with(&report, &change.path, level)
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
    use super::*;
    use crate::core::domain::git_types::{ChangedFile, CommitInfo};

    /// Repo mock que solo provee working_tree_changes y get_blob — suficiente
    /// para ejercitar `analyze_with_repo` sin tocar disco real.
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
        let report = analyze_with_repo(&repo, None).unwrap();
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
        let report = analyze_with_repo(&repo, None).unwrap();
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
        let report = analyze_with_repo(&repo, None).unwrap();
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
