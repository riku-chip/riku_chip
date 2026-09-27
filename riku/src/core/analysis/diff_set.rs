//! `riku diff` entre dos versiones cualesquiera: un commit o el working
//! tree (el disco), de un archivo o de todos los que cambiaron, como
//! `git diff`:
//!
//! | Forma | Compara |
//! |---|---|
//! | `riku diff` | working tree contra `HEAD`, todos los archivos |
//! | `riku diff ARCHIVO` | ese archivo, working tree contra `HEAD` |
//! | `riku diff A` | working tree contra `A`, todos |
//! | `riku diff A ARCHIVO` | ese archivo, working tree contra `A` |
//! | `riku diff A B` | `B` contra `A`, todos |
//! | `riku diff A B ARCHIVO` | ese archivo, `B` contra `A` |

use std::collections::BTreeMap;
use std::path::Path;

use riku_kernel::{DiffFiles, DiffOptions, Registry};

use crate::core::analysis::blob_io;
use crate::core::analysis::commit_diff::AnalyzeError;
use crate::core::analysis::show::ShowFile;
use crate::core::domain::git_types::ChangeStatus;
use crate::core::domain::models::{FileChange, FileFormat};
use crate::core::domain::ports::GitRepository;
use crate::core::git::files;

/// Nombre que usa el visor (y la CLI al lanzarlo) para "el working tree"
/// en lugar de un commit.
pub const WORKTREE: &str = ":worktree";

/// Una de las dos versiones que se comparan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Side {
    /// Un commit (hash, rama, tag, `HEAD~2`…).
    Rev(String),
    /// Los archivos en disco.
    WorkTree,
}

impl Side {
    /// Para mostrar y para el JSON: el commit tal como se pidió, o `worktree`.
    pub fn label(&self) -> &str {
        match self {
            Side::Rev(r) => r,
            Side::WorkTree => "worktree",
        }
    }

    /// Para pasarle al visor (`--commit-b :worktree`).
    pub fn token(&self) -> &str {
        match self {
            Side::Rev(r) => r,
            Side::WorkTree => WORKTREE,
        }
    }
}

/// Resultado de comparar todos los archivos que cambiaron.
#[derive(Debug)]
pub struct DiffSetReport {
    pub from: Side,
    pub to: Side,
    pub files: Vec<ShowFile>,
}

impl DiffSetReport {
    pub fn has_functional_changes(&self) -> bool {
        self.files.iter().filter_map(|f| f.change.as_ref()).any(|c| c.functional().next().is_some())
    }
}

/// Contenido de `path` en un lado; `None` si no existe ahí.
fn read_side<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    side: &Side,
    path: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<u8>>, AnalyzeError> {
    match side {
        Side::Rev(r) => Ok(blob_io::read_blob_lenient(repo, r, path, warnings)?),
        Side::WorkTree => Ok(workdir.and_then(|w| std::fs::read(w.join(path)).ok())),
    }
}

fn sources<R: GitRepository + ?Sized>(repo: &R, workdir: Option<&Path>, from: &Side, to: &Side) -> DiffFiles {
    let one = |s: &Side| match s {
        Side::Rev(r) => files::commit_files(repo, r),
        Side::WorkTree => files::workdir_files(workdir),
    };
    DiffFiles::new(one(from), one(to))
}

/// Un archivo entre dos lados. Un archivo sin módulo da un `FileChange`
/// vacío con un aviso (como antes).
pub fn analyze_file<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    from: &Side,
    to: &Side,
    path: &str,
    modules: &Registry,
    opts: &DiffOptions,
) -> Result<FileChange, AnalyzeError> {
    let Some(module) = modules.for_path(path) else {
        let mut report = FileChange::new(FileFormat::Unknown);
        report.warnings.push(format!("{path}: no hay módulo de Riku para este formato."));
        return Ok(report);
    };
    let mut warnings = Vec::new();
    let before = read_side(repo, workdir, from, path, &mut warnings)?;
    let after = read_side(repo, workdir, to, path, &mut warnings)?;
    if before.is_none() && after.is_none() {
        warnings.push(format!("{path}: no existe en {} ni en {}", from.label(), to.label()));
    }
    let files = sources(repo, workdir, from, to);
    let mut report = module.diff_with(&before.unwrap_or_default(), &after.unwrap_or_default(), path, opts, &files);
    report.warnings.extend(warnings);
    Ok(report)
}

/// Todos los archivos que cambiaron entre `from` y `to`.
pub fn analyze_all<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    from: &Side,
    to: &Side,
    modules: &Registry,
    opts: &DiffOptions,
) -> Result<DiffSetReport, AnalyzeError> {
    let entries = changed_paths(repo, workdir, from, to)?;
    let mut out = Vec::with_capacity(entries.len());
    for (path, (status, old_path)) in entries {
        let change = match modules.for_path(&path) {
            Some(_) => Some(analyze_file(repo, workdir, from, to, &path, modules, opts)?),
            None => None,
        };
        out.push(ShowFile { path, status: Some(status), old_path, change });
    }
    Ok(DiffSetReport { from: from.clone(), to: to.clone(), files: out })
}

/// Rutas que cambiaron, con qué les pasó (ordenadas).
fn changed_paths<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    from: &Side,
    to: &Side,
) -> Result<BTreeMap<String, (ChangeStatus, Option<String>)>, AnalyzeError> {
    let mut map = BTreeMap::new();
    match (from, to) {
        (Side::Rev(a), Side::Rev(b)) => {
            for c in repo.get_changed_files(a, b)? {
                map.insert(c.path, (c.status, c.old_path));
            }
        }
        (Side::Rev(a), Side::WorkTree) | (Side::WorkTree, Side::Rev(a)) => {
            // Lo que cambió de `a` a HEAD más lo que cambió en disco desde
            // HEAD; después se descarta lo que quedó igual a `a`.
            let mut candidates: Vec<String> = repo.working_tree_changes()?.into_iter().map(|c| c.path).collect();
            if !is_head(a) {
                for c in repo.get_changed_files(a, "HEAD")? {
                    candidates.extend(c.old_path);
                    candidates.push(c.path);
                }
            }
            candidates.sort();
            candidates.dedup();
            let mut ignored = Vec::new();
            for path in candidates {
                let before = read_side(repo, workdir, &Side::Rev(a.clone()), &path, &mut ignored)?;
                let disk = read_side(repo, workdir, &Side::WorkTree, &path, &mut ignored)?;
                let (before, after) = if matches!(from, Side::WorkTree) { (disk, before) } else { (before, disk) };
                let status = match (&before, &after) {
                    (None, Some(_)) => ChangeStatus::Added,
                    (Some(_), None) => ChangeStatus::Removed,
                    (Some(x), Some(y)) if x != y => ChangeStatus::Modified,
                    _ => continue,
                };
                map.insert(path, (status, None));
            }
        }
        (Side::WorkTree, Side::WorkTree) => {}
    }
    Ok(map)
}

fn is_head(rev: &str) -> bool {
    rev.eq_ignore_ascii_case("HEAD") || rev == "@"
}
