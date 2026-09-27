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

use std::sync::Arc;

use riku_kernel::{DiffFiles, DiffOptions, FileSource, Registry};

use crate::core::analysis::blob_io::Blob;
use crate::core::analysis::diff_pair::{self, diff_pair, AnalyzeError, End, OnError, Version};
use crate::core::analysis::parallel;
use crate::core::analysis::show::ShowFile;
use crate::core::domain::git_types::{ChangeStatus, GitError};
use crate::core::domain::models::{FileChange, FileFormat};
use crate::core::domain::ports::{GitRepository, RepoRoot};

pub use crate::core::analysis::diff_pair::WORKTREE;

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

    pub fn version(&self) -> Version<'_> {
        match self {
            Side::Rev(r) => Version::Rev(r),
            Side::WorkTree => Version::WorkTree,
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

/// `path` en un lado.
pub(crate) fn read_side<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    side: &Side,
    path: &str,
) -> Result<Blob, AnalyzeError> {
    diff_pair::read(repo, workdir, End::new(side.version(), path), OnError::Propagate)
}

/// Los otros archivos de una versión tal como la nombra el visor (ver
/// [`Version::from_token`]). Magic busca ahí las sub-celdas.
pub fn token_files<R: GitRepository + RepoRoot + ?Sized>(repo: &R, token: &str) -> Option<Arc<dyn FileSource>> {
    diff_pair::version_files(repo, repo.root(), Version::from_token(token))
}

pub(crate) fn sources<R: GitRepository + ?Sized>(repo: &R, workdir: Option<&Path>, from: &Side, to: &Side) -> DiffFiles {
    diff_pair::sources(repo, workdir, from.version(), to.version())
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
    let (before, after) = (End::new(from.version(), path), End::new(to.version(), path));
    diff_pair(repo, workdir, module.as_ref(), before, after, opts, OnError::Propagate)
}

/// Un archivo que cambió, con qué le pasó y su ruta anterior.
type Entry = (String, (ChangeStatus, Option<String>));

/// Todos los archivos que cambiaron entre `from` y `to`, en paralelo como
/// `show` (una conexión a Git por hilo, en tandas que caben en memoria).
pub fn analyze_all<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    from: &Side,
    to: &Side,
    modules: &Registry,
    opts: &DiffOptions,
) -> Result<DiffSetReport, AnalyzeError> {
    let entries: Vec<Entry> = changed_paths(repo, workdir, from, to)?.into_iter().collect();
    let size = |side: &Side, path: &str| match side {
        Side::Rev(r) => repo.blob_size(r, path),
        Side::WorkTree => workdir.and_then(|w| std::fs::metadata(w.join(path)).ok()).map(|m| m.len()),
    };
    let costs: Vec<u64> = entries
        .iter()
        .map(|(path, (_, old))| match modules.for_path(path) {
            Some(_) => parallel::diff_cost(size(from, old.as_deref().unwrap_or(path)), size(to, path)),
            None => 0,
        })
        .collect();
    let files = parallel::map_in_waves(
        repo.reopener(),
        entries,
        &costs,
        |entry| diff_entry(repo, workdir, from, to, entry, modules, opts),
        |r, entry| diff_entry(r, workdir, from, to, entry, modules, opts),
        |_, e| Err(AnalyzeError::Git(GitError::Git(git2::Error::from_str(&e.to_string())))),
    )
    .into_iter()
    .collect::<Result<Vec<ShowFile>, AnalyzeError>>()?;
    Ok(DiffSetReport { from: from.clone(), to: to.clone(), files })
}

/// El diff de un archivo de [`analyze_all`]; `None` si ningún módulo lo reconoce.
fn diff_entry<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    from: &Side,
    to: &Side,
    (path, (status, old_path)): Entry,
    modules: &Registry,
    opts: &DiffOptions,
) -> Result<ShowFile, AnalyzeError> {
    let change = match modules.for_path(&path) {
        Some(module) => {
            let before = End::new(from.version(), old_path.as_deref().unwrap_or(&path));
            let after = End::new(to.version(), &path);
            Some(diff_pair(repo, workdir, module.as_ref(), before, after, opts, OnError::Propagate)?)
        }
        None => None,
    };
    Ok(ShowFile { path, status: Some(status), old_path, change })
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
            for path in candidates {
                let before = read_side(repo, workdir, &Side::Rev(a.clone()), &path)?;
                let disk = read_side(repo, workdir, &Side::WorkTree, &path)?;
                let (before, after) = if matches!(from, Side::WorkTree) { (disk, before) } else { (before, disk) };
                // Un lado que no se pudo leer cuenta como modificado: el
                // diff lo dirá como error.
                let status = match (&before, &after) {
                    (Blob::Missing, Blob::Missing) => continue,
                    (Blob::Missing, _) => ChangeStatus::Added,
                    (_, Blob::Missing) => ChangeStatus::Removed,
                    (Blob::Bytes(x), Blob::Bytes(y)) if x == y => continue,
                    _ => ChangeStatus::Modified,
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
