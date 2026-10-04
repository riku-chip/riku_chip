//! Recorrido del historial Git y construcción de `LogCommit` por commit.

use std::path::Path;

use riku_kernel::{FileChange, Registry};

use crate::core::analysis::diff_pair::{diff_pair, End, OnError, Version};
use crate::core::analysis::summary::{FileSummary, SummaryCategory};
use crate::core::analysis::{graph, parallel};
use crate::core::domain::git_types::{ChangeStatus, ChangedFile, CommitWithParents, LogQuery};
use crate::core::domain::ports::GitRepository;
use crate::core::git::git_service::GitService;
use crate::core::path_matcher::PathMatcher;

use super::types::{LogCommit, LogError, LogOptions, LogReport};

// ─── Entry points ────────────────────────────────────────────────────────────

/// Abre el repo desde path y aplica `LogOptions`.
pub fn analyze_with_options_path(repo_path: &Path, opts: &LogOptions, modules: &Registry) -> Result<LogReport, LogError> {
    let svc = GitService::open(repo_path)?;
    walk_with_summary(&svc, opts, modules)
}

pub fn walk_with_summary<R: GitRepository + ?Sized>(
    repo: &R,
    opts: &LogOptions,
    modules: &Registry,
) -> Result<LogReport, LogError> {
    // El filtro por `paths` va en el recorrido de Git, antes del límite.
    let query = LogQuery { paths: &opts.paths, limit: opts.limit, start: opts.start.as_deref(), topological: opts.graph };
    let raw = repo.get_commits_with_options(&query)?;
    // El DAG cargado (oid y padres), para el grafo.
    let dag: Vec<(String, Vec<String>)> =
        if opts.graph { raw.iter().map(|c| (c.info.oid.clone(), c.parents.clone())).collect() } else { Vec::new() };
    let refs_map = repo.refs_by_oid().unwrap_or_default();

    // Dos pasadas sobre los commits, cada una repartida entre los hilos con
    // una conexión a Git por hilo (ver `parallel`): primero qué archivos
    // cambió cada uno y cuánto pesan (barato: árboles de Git y cabeceras de
    // blobs), después los diffs, en tandas que caben en memoria.
    if opts.skip_summaries {
        let mut commits: Vec<LogCommit> = raw
            .into_iter()
            .map(|c| build_log_commit_without_files(Planned { raw: c, files: Vec::new(), warnings: Vec::new() }, &refs_map).0)
            .collect();
        if opts.graph {
            for (c, row) in commits.iter_mut().zip(graph::layout(&dag)) {
                c.graph = Some(row);
            }
        }
        return Ok(LogReport { commits, warnings: Vec::new() });
    }

    let free = vec![0; raw.len()];
    let planned: Vec<(Planned, u64)> = parallel::map_in_waves(
        repo.reopener(),
        raw,
        &free,
        |c| plan(repo, c, opts, modules),
        |r, c| plan(r, c, opts, modules),
        |c, e| {
            let warning = format!("commit {}: {e}", c.info.oid);
            (Planned { raw: c, files: Vec::new(), warnings: vec![warning] }, 0)
        },
    );
    let costs: Vec<u64> = planned.iter().map(|(_, cost)| *cost).collect();
    let planned: Vec<Planned> = planned.into_iter().map(|(p, _)| p).collect();
    let built: Vec<(LogCommit, Vec<String>)> = parallel::map_in_waves(
        repo.reopener(),
        planned,
        &costs,
        |p| build_log_commit(repo, p, &refs_map, opts, modules),
        |r, p| build_log_commit(r, p, &refs_map, opts, modules),
        |p, e| {
            let warning = format!("commit {}: {e}", p.raw.info.oid);
            let mut out = build_log_commit_without_files(p, &refs_map);
            out.1.push(warning);
            out
        },
    );

    let mut warnings = Vec::new();
    let mut commits = Vec::with_capacity(built.len());
    for (log_commit, w) in built {
        warnings.extend(w);
        // Tocó un archivo del filtro, pero ninguno con cambios que mostrar
        // (sin módulo, o sin cambios semánticos ni avisos). Los merges y el
        // commit inicial no llevan resumen: pasaron el filtro de Git, quedan.
        if !opts.paths.is_empty() && log_commit.files.is_empty() && log_commit.parents.len() == 1 {
            continue;
        }
        commits.push(log_commit);
    }

    if opts.graph {
        // Con `--paths` algunos commits no se muestran: sus hijos se conectan
        // al ancestro visible más cercano.
        let visible: std::collections::HashSet<String> = commits.iter().map(|c| c.info.oid.clone()).collect();
        let shown = if visible.len() == dag.len() { dag } else { graph::simplify(&dag, &visible) };
        for (c, row) in commits.iter_mut().zip(graph::layout(&shown)) {
            c.graph = Some(row);
        }
    }

    Ok(LogReport { commits, warnings })
}

// ─── Construcción por commit ─────────────────────────────────────────────────

/// Un commit con los archivos que hay que comparar (ya filtrados por
/// `paths` y por módulo) y los avisos de la primera pasada.
struct Planned {
    raw: CommitWithParents,
    files: Vec<ChangedFile>,
    warnings: Vec<String>,
}

/// Primera pasada: qué archivos cambió el commit respecto a su primer padre
/// y el costo estimado de compararlos (por el tamaño de sus blobs).
fn plan<R: GitRepository + ?Sized>(repo: &R, raw: CommitWithParents, opts: &LogOptions, modules: &Registry) -> (Planned, u64) {
    let mut warnings = Vec::new();
    // Root commit y merges: en v1 no se hace diff por archivo.
    let files = match raw.parents.first() {
        Some(parent) if raw.parents.len() == 1 => match repo.get_changed_files(parent, &raw.info.oid) {
            Ok(list) => {
                let matcher = PathMatcher::new(&opts.paths);
                // Formatos sin módulo no se listan en log.
                list.into_iter().filter(|cf| matcher.matches(&cf.path) && modules.for_path(&cf.path).is_some()).collect()
            }
            Err(e) => {
                warnings.push(format!("commit {}: {e}", raw.info.oid));
                Vec::new()
            }
        },
        _ => Vec::new(),
    };
    // Los archivos de un commit se comparan de a uno: la memoria que ocupa
    // es la del más grande, no la suma (sumar armaba tandas más chicas).
    let cost = match raw.parents.first() {
        Some(parent) => files
            .iter()
            .map(|cf: &ChangedFile| {
                let before = (cf.status != ChangeStatus::Added).then(|| repo.blob_size(parent, cf.before_path())).flatten();
                let after = (cf.status != ChangeStatus::Removed).then(|| repo.blob_size(&raw.info.oid, &cf.path)).flatten();
                parallel::diff_cost(before, after)
            })
            .max()
            .unwrap_or(0),
        None => 0,
    };
    (Planned { raw, files, warnings }, cost)
}

/// Segunda pasada: el resumen semántico de cada archivo del commit.
fn build_log_commit<R: GitRepository + ?Sized>(
    repo: &R,
    planned: Planned,
    refs_map: &std::collections::HashMap<String, Vec<String>>,
    opts: &LogOptions,
    modules: &Registry,
) -> (LogCommit, Vec<String>) {
    let Planned { raw, files: changed, warnings } = planned;
    let commit = raw.info.oid.clone();
    let mut files = Vec::new();
    if let Some(parent) = raw.parents.first().filter(|_| raw.parents.len() == 1) {
        for cf in changed {
            let Some(module) = modules.for_path(&cf.path) else { continue };
            let before = if cf.status == ChangeStatus::Added { Version::Absent } else { Version::Rev(parent) };
            let after = if cf.status == ChangeStatus::Removed { Version::Absent } else { Version::Rev(&commit) };
            let (before, after) = (End::new(before, cf.before_path()), End::new(after, &cf.path));
            // `InFile`: un blob que no se lee queda como error del archivo,
            // no tumba el historial.
            let report = diff_pair(repo, None, module.as_ref(), before, after, &opts.diff, OnError::InFile)
                .unwrap_or_else(|e| FileChange::failed(module.info().format, e.to_string()));
            let summary = FileSummary::from_report_with(&report, &cf.path, opts.level);
            // Se saltan los archivos sin cambio semántico ni cosmético, para
            // no inflar el log; con avisos no, que el aviso es la noticia.
            // Tampoco los de otro formato con la misma extensión: como los
            // que no tienen módulo.
            if matches!(summary.category, SummaryCategory::Unchanged) && summary.warnings.is_empty()
                || matches!(summary.category, SummaryCategory::Unknown)
            {
                continue;
            }
            files.push(summary);
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
    }
    let (mut out, _) = build_log_commit_without_files(Planned { raw, files: Vec::new(), warnings: Vec::new() }, refs_map);
    out.files = files;
    (out, warnings)
}

/// El `LogCommit` sin resumen por archivo (merges, commit inicial, o si no se
/// pudo abrir una conexión a Git).
fn build_log_commit_without_files(
    planned: Planned,
    refs_map: &std::collections::HashMap<String, Vec<String>>,
) -> (LogCommit, Vec<String>) {
    let Planned { raw, warnings, .. } = planned;
    let refs = refs_map.get(&raw.info.oid).cloned().unwrap_or_default();
    let is_merge = raw.parents.len() > 1;
    (
        LogCommit { info: raw.info, parents: raw.parents, refs, is_merge, files: Vec::new(), graph: None, lvs: Vec::new() },
        warnings,
    )
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::types::EnvelopedLogReport;
    use super::*;
    use crate::core::domain::git_types::{BranchInfo, ChangedFile, CommitChanges, CommitInfo, GitError, WorkingChange};

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
                .ok_or_else(|| GitError::BlobNotFound { commit: commit_ish.to_string(), path: file_path.to_string() })
        }
        fn get_changed_files(&self, a: &str, b: &str) -> Result<Vec<ChangedFile>, GitError> {
            Ok(self.changed.get(&(a.to_string(), b.to_string())).cloned().unwrap_or_default())
        }
        fn working_tree_changes(&self) -> Result<Vec<WorkingChange>, GitError> {
            Ok(Vec::new())
        }
        fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
            Ok(None)
        }
        fn get_commits_with_options(&self, _query: &LogQuery<'_>) -> Result<Vec<CommitWithParents>, GitError> {
            Ok(self.commits.clone())
        }
        fn refs_by_oid(&self) -> Result<std::collections::HashMap<String, Vec<String>>, GitError> {
            Ok(self.refs.clone())
        }
        fn commit_changes(&self, _: &str) -> Result<CommitChanges, GitError> {
            unimplemented!("log no usa commit_changes")
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
        let report = walk_with_summary(&repo, &LogOptions::default(), &crate::modules::registry()).unwrap();
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
        let report = walk_with_summary(&repo, &LogOptions::default(), &crate::modules::registry()).unwrap();
        assert!(!report.commits[0].is_merge);
        assert!(report.commits[0].files.is_empty());
    }

    #[test]
    fn refs_se_anotan_por_oid() {
        let mut refs = std::collections::HashMap::new();
        refs.insert("abc1234".to_string(), vec!["main".to_string(), "HEAD".to_string()]);
        let repo =
            MockRepo { commits: vec![ci("abc1234", &["parent"])], blobs: Default::default(), changed: Default::default(), refs };
        let report = walk_with_summary(&repo, &LogOptions::default(), &crate::modules::registry()).unwrap();
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
        let opts = LogOptions { paths: vec!["*.sch".to_string()], ..Default::default() };
        let report = walk_with_summary(&repo, &opts, &crate::modules::registry()).unwrap();
        assert!(report.commits.is_empty());
    }

    #[test]
    fn json_envelope_lleva_schema() {
        let report = LogReport { commits: vec![], warnings: vec![] };
        let env = EnvelopedLogReport::from(&report);
        let v: serde_json::Value = serde_json::to_value(&env).unwrap();
        assert_eq!(v["schema"], "riku-log/v2");
        assert!(v.get("commits").is_some());
    }
}
