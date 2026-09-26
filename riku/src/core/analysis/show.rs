//! `riku show`: los cambios semánticos de un commit respecto a su primer
//! padre, archivo por archivo (como `git show`, con el diff de cada módulo).
//!
//! El commit inicial se compara contra vacío: sus archivos aparecen añadidos.
//! Un merge se compara contra su primer padre.

use riku_kernel::{DiffOptions, Registry};

use crate::core::analysis::blob_io;
use crate::core::analysis::commit_diff::AnalyzeError;
use crate::core::domain::git_types::{ChangeStatus, CommitWithParents};
use crate::core::domain::models::FileChange;
use crate::core::domain::ports::GitRepository;

#[derive(Debug)]
pub struct ShowReport {
    pub commit: CommitWithParents,
    pub files: Vec<ShowFile>,
}

#[derive(Debug)]
pub struct ShowFile {
    pub path: String,
    /// Qué le hizo git al archivo. `None` si se pidió un archivo que el
    /// commit no tocó.
    pub status: Option<ChangeStatus>,
    /// Ruta anterior, si git lo detectó como renombrado.
    pub old_path: Option<String>,
    /// Diff del módulo del formato; `None` si ningún módulo lo reconoce.
    pub change: Option<FileChange>,
}

impl ShowReport {
    /// Primer padre (contra el que se compara). `None` en el commit inicial.
    pub fn parent(&self) -> Option<&str> {
        self.commit.parents.first().map(String::as_str)
    }

    /// `true` si algún archivo tiene un cambio funcional (no cosmético).
    pub fn has_functional_changes(&self) -> bool {
        self.files.iter().filter_map(|f| f.change.as_ref()).any(|c| c.functional().next().is_some())
    }
}

/// Cambios de `commit`. Con `file`, solo ese archivo (aunque el commit no lo
/// haya tocado: entonces sale sin cambios); sin él, todos los que cambió.
pub fn analyze_show<R: GitRepository + ?Sized>(
    repo: &R,
    commit: &str,
    file: Option<&str>,
    modules: &Registry,
    opts: &DiffOptions,
) -> Result<ShowReport, AnalyzeError> {
    let changes = repo.commit_changes(commit)?;
    let entries: Vec<(String, Option<ChangeStatus>, Option<String>)> = match file {
        Some(f) => {
            let touched = changes.files.iter().find(|c| c.path == f || c.old_path.as_deref() == Some(f));
            vec![match touched {
                Some(c) => (c.path.clone(), Some(c.status.clone()), c.old_path.clone()),
                None => (f.to_string(), None, None),
            }]
        }
        None => changes.files.iter().map(|c| (c.path.clone(), Some(c.status.clone()), c.old_path.clone())).collect(),
    };

    let oid = changes.commit.info.oid.clone();
    let parent = changes.commit.parents.first().cloned();
    let mut files = Vec::with_capacity(entries.len());
    for (path, status, old_path) in entries {
        let change = match modules.for_path(&path) {
            None => None,
            Some(module) => {
                let mut warnings = Vec::new();
                let before_path = old_path.as_deref().unwrap_or(&path);
                let before = match &parent {
                    Some(p) => blob_io::read_blob_lenient(repo, p, before_path, &mut warnings)?.unwrap_or_default(),
                    None => Vec::new(),
                };
                let after = blob_io::read_blob_lenient(repo, &oid, &path, &mut warnings)?.unwrap_or_default();
                let mut report = module.diff(&before, &after, &path, opts);
                report.warnings.extend(warnings);
                Some(report)
            }
        };
        // `old_path` solo interesa si de verdad cambió de nombre.
        let old_path = old_path.filter(|o| *o != path);
        files.push(ShowFile { path, status, old_path, change });
    }
    Ok(ShowReport { commit: changes.commit, files })
}
