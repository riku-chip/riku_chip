//! Orquestación de `riku log`.
//!
//! Recorre el historial Git y, por cada commit, computa un resumen semántico
//! (igual que `status` para el working tree, pero entre `parent..commit`).
//! Como `status`, no formatea — entrega `LogReport` y la capa CLI elige texto
//! o JSON.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::adapters::registry::get_driver_for;
use crate::core::analysis::blob_io;
use crate::core::analysis::envelope::Envelope;
use crate::core::analysis::summary::{DetailLevel, FileSummary, SummaryCategory};
use crate::core::domain::driver::DriverDiffReport;
use crate::core::domain::git_types::{
    ChangeStatus, CommitInfo, CommitWithParents, GitError, LogQuery,
};
use crate::core::domain::ports::GitRepository;
use crate::core::git::git_service::GitService;
use crate::core::path_matcher::PathMatcher;

// ─── Errores ─────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum LogError {
    #[error(transparent)]
    Git(#[from] GitError),
}

// ─── Schema ──────────────────────────────────────────────────────────────────

pub const LOG_SCHEMA: &str = "riku-log/v1";

pub type EnvelopedLogReport<'a> = Envelope<'a, LogReport>;

impl<'a> From<&'a LogReport> for EnvelopedLogReport<'a> {
    fn from(inner: &'a LogReport) -> Self {
        Envelope::new(LOG_SCHEMA, inner)
    }
}

// ─── Modelo de salida ────────────────────────────────────────────────────────

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

// ─── Entry points ────────────────────────────────────────────────────────────

/// Abre el repo desde path y aplica `LogOptions`.
pub fn analyze_with_options_path(
    repo_path: &Path,
    opts: &LogOptions,
) -> Result<LogReport, LogError> {
    let svc = GitService::open(repo_path)?;
    walk_with_summary(&svc, opts)
}

pub fn walk_with_summary<R: GitRepository + ?Sized>(
    repo: &R,
    opts: &LogOptions,
) -> Result<LogReport, LogError> {
    // Para mantener semántica de Git nativo cuando hay `paths`, recorremos
    // todo y filtramos por commit. Si en el futuro hace falta optimizar,
    // pasar el primer path al `LogQuery::file_path`.
    let query = LogQuery {
        file_path: None,
        limit: opts.limit,
        start: opts.start.as_deref(),
    };
    let raw = repo.get_commits_with_options(&query)?;
    let refs_map = repo.refs_by_oid().unwrap_or_default();

    let mut warnings = Vec::new();
    let mut commits = Vec::with_capacity(raw.len());
    for c in raw {
        let log_commit = build_log_commit(repo, c, &refs_map, opts, &mut warnings);
        // Si hay filtro de paths y este commit no tocó ninguno, lo omitimos.
        if !opts.paths.is_empty() && log_commit.files.is_empty() && !log_commit.is_merge {
            continue;
        }
        commits.push(log_commit);
    }

    Ok(LogReport { commits, warnings })
}

// ─── Construcción por commit ─────────────────────────────────────────────────

fn build_log_commit<R: GitRepository + ?Sized>(
    repo: &R,
    raw: CommitWithParents,
    refs_map: &std::collections::HashMap<String, Vec<String>>,
    opts: &LogOptions,
    warnings: &mut Vec<String>,
) -> LogCommit {
    let oid = raw.info.oid.clone();
    let refs = refs_map.get(&oid).cloned().unwrap_or_default();
    let is_merge = raw.parents.len() > 1;

    let files = if is_merge || raw.parents.is_empty() {
        // Root commit y merges: en v1 no se hace diff por archivo.
        Vec::new()
    } else {
        let parent = &raw.parents[0];
        diff_against_parent(repo, parent, &oid, opts, warnings)
    };

    LogCommit {
        info: raw.info,
        parents: raw.parents,
        refs,
        is_merge,
        files,
    }
}

fn diff_against_parent<R: GitRepository + ?Sized>(
    repo: &R,
    parent: &str,
    commit: &str,
    opts: &LogOptions,
    warnings: &mut Vec<String>,
) -> Vec<FileSummary> {
    let changed = match repo.get_changed_files(parent, commit) {
        Ok(list) => list,
        Err(e) => {
            warnings.push(format!("commit {commit}: {e}"));
            return Vec::new();
        }
    };

    let matcher = PathMatcher::new(&opts.paths);

    let mut files = Vec::new();
    for cf in changed {
        if !matcher.matches(&cf.path) {
            continue;
        }
        let driver = match get_driver_for(&cf.path) {
            Some(d) => d,
            None => continue, // formatos sin driver no se listan en log
        };

        let content_before = if cf.status == ChangeStatus::Added {
            Vec::new()
        } else {
            blob_io::read_blob_silent(repo, parent, &cf.path, warnings)
        };
        let content_after = if cf.status == ChangeStatus::Removed {
            Vec::new()
        } else {
            blob_io::read_blob_silent(repo, commit, &cf.path, warnings)
        };

        let report: DriverDiffReport = driver.diff(&content_before, &content_after, &cf.path);
        let summary = FileSummary::from_report_with(&report, &cf.path, opts.level);
        // Saltamos archivos sin cambio semántico ni cosmético detectado, para
        // no inflar el log con ruido de driver.
        if matches!(summary.category, SummaryCategory::Unchanged) {
            continue;
        }
        files.push(summary);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::git_types::{BranchInfo, ChangedFile, WorkingChange};

    struct MockRepo {
        commits: Vec<CommitWithParents>,
        blobs: std::collections::HashMap<(String, String), Vec<u8>>,
        changed: std::collections::HashMap<(String, String), Vec<ChangedFile>>,
        refs: std::collections::HashMap<String, Vec<String>>,
    }

    impl GitRepository for MockRepo {
        fn get_blob(&self, commit_ish: &str, file_path: &str) -> Result<Vec<u8>, GitError> {
            self.blobs
                .get(&(commit_ish.to_string(), file_path.to_string()))
                .cloned()
                .ok_or_else(|| GitError::BlobNotFound {
                    commit: commit_ish.to_string(),
                    path: file_path.to_string(),
                })
        }
        fn get_commits(&self, _file_path: Option<&str>) -> Result<Vec<CommitInfo>, GitError> {
            Ok(self.commits.iter().map(|c| c.info.clone()).collect())
        }
        fn get_changed_files(&self, a: &str, b: &str) -> Result<Vec<ChangedFile>, GitError> {
            Ok(self
                .changed
                .get(&(a.to_string(), b.to_string()))
                .cloned()
                .unwrap_or_default())
        }
        fn working_tree_changes(&self) -> Result<Vec<WorkingChange>, GitError> {
            Ok(Vec::new())
        }
        fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
            Ok(None)
        }
        fn get_commits_with_options(
            &self,
            _query: &LogQuery<'_>,
        ) -> Result<Vec<CommitWithParents>, GitError> {
            Ok(self.commits.clone())
        }
        fn refs_by_oid(&self) -> Result<std::collections::HashMap<String, Vec<String>>, GitError> {
            Ok(self.refs.clone())
        }
    }

    fn ci(oid: &str, parents: &[&str]) -> CommitWithParents {
        CommitWithParents {
            info: CommitInfo {
                oid: oid.to_string(),
                short_id: oid.chars().take(7).collect(),
                message: format!("commit {oid}"),
                author: "tester".to_string(),
                timestamp: 0,
            },
            parents: parents.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn merge_no_lleva_files_pero_se_marca() {
        let repo = MockRepo {
            commits: vec![ci("abc", &["p1", "p2"])],
            blobs: Default::default(),
            changed: Default::default(),
            refs: Default::default(),
        };
        let report = walk_with_summary(&repo, &LogOptions::default()).unwrap();
        assert_eq!(report.commits.len(), 1);
        assert!(report.commits[0].is_merge);
        assert!(report.commits[0].files.is_empty());
    }

    #[test]
    fn root_commit_no_lleva_files() {
        let repo = MockRepo {
            commits: vec![ci("root", &[])],
            blobs: Default::default(),
            changed: Default::default(),
            refs: Default::default(),
        };
        let report = walk_with_summary(&repo, &LogOptions::default()).unwrap();
        assert!(!report.commits[0].is_merge);
        assert!(report.commits[0].files.is_empty());
    }

    #[test]
    fn refs_se_anotan_por_oid() {
        let mut refs = std::collections::HashMap::new();
        refs.insert(
            "abc1234".to_string(),
            vec!["main".to_string(), "HEAD".to_string()],
        );
        let repo = MockRepo {
            commits: vec![ci("abc1234", &["parent"])],
            blobs: Default::default(),
            changed: Default::default(),
            refs,
        };
        let report = walk_with_summary(&repo, &LogOptions::default()).unwrap();
        assert!(report.commits[0].refs.contains(&"main".to_string()));
        assert!(report.commits[0].refs.contains(&"HEAD".to_string()));
    }

    #[test]
    fn paths_filtra_commits_que_no_tocan_match() {
        // Mock con un commit sin file changed → con filtro paths se omite.
        let repo = MockRepo {
            commits: vec![ci("abc", &["parent"])],
            blobs: Default::default(),
            changed: Default::default(), // sin entradas → 0 cambios
            refs: Default::default(),
        };
        let opts = LogOptions {
            paths: vec!["*.sch".to_string()],
            ..Default::default()
        };
        let report = walk_with_summary(&repo, &opts).unwrap();
        assert!(report.commits.is_empty());
    }

    #[test]
    fn json_envelope_lleva_schema() {
        let report = LogReport {
            commits: vec![],
            warnings: vec![],
        };
        let env = EnvelopedLogReport::from(&report);
        let v: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(v["schema"], "riku-log/v1");
        assert!(v.get("commits").is_some());
    }

}
