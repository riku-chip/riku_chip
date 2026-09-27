//! Un archivo entre dos versiones: el flujo que comparten `diff`, `show`,
//! `status`, `log` y el visor. Leer cada lado (un commit, el disco o nada),
//! armar los otros archivos de cada versión (sub-celdas de Magic) y pasar
//! por el módulo, con una sola regla para los errores.

use std::path::Path;
use std::sync::Arc;

use riku_kernel::{DiffFiles, DiffOptions, FileChange, FileSource, FormatModule};
use thiserror::Error;

use crate::core::analysis::blob_io::{self, Blob};
use crate::core::analysis::pipeline;
use crate::core::domain::git_types::GitError;
use crate::core::domain::ports::GitRepository;
use crate::core::git::files;

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error(transparent)]
    Git(#[from] GitError),
}

/// Nombre que usa el visor (y la CLI al lanzarlo) para "el working tree"
/// en lugar de un commit.
pub const WORKTREE: &str = ":worktree";

/// Una versión de un archivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version<'a> {
    /// Un commit (hash, rama, tag, `HEAD~2`…).
    Rev(&'a str),
    /// El disco.
    WorkTree,
    /// Ninguna: el archivo es nuevo o se borró, o el "antes" del commit
    /// inicial. Se compara contra vacío.
    Absent,
}

impl<'a> Version<'a> {
    /// Como la nombra el visor: `""` es ninguna, [`WORKTREE`] el disco y lo
    /// demás un commit.
    pub fn from_token(token: &'a str) -> Self {
        match token {
            "" => Version::Absent,
            WORKTREE => Version::WorkTree,
            rev => Version::Rev(rev),
        }
    }

    /// Para mostrar: el commit tal como se pidió, `worktree` o `∅`.
    pub fn label(&self) -> &'a str {
        match self {
            Version::Rev(r) => r,
            Version::WorkTree => "worktree",
            Version::Absent => "∅",
        }
    }
}

/// Qué hacer con un error de Git que no es del archivo (commit que no
/// existe, repo roto).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnError {
    /// Se propaga: `diff` y `show` fallan con el error.
    Propagate,
    /// Queda en el archivo: `status` y `log` no deben caerse por un blob.
    InFile,
}

/// Un lado de la comparación: qué versión y con qué ruta (la vieja, si el
/// archivo se renombró).
#[derive(Clone, Copy, Debug)]
pub struct End<'a> {
    pub version: Version<'a>,
    pub path: &'a str,
}

impl<'a> End<'a> {
    pub fn new(version: Version<'a>, path: &'a str) -> Self {
        Self { version, path }
    }
}

/// `path` en una versión.
pub fn read<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    end: End<'_>,
    on_error: OnError,
) -> Result<Blob, AnalyzeError> {
    Ok(match end.version {
        Version::Absent => Blob::Missing,
        Version::WorkTree => blob_io::read_disk(workdir, end.path),
        Version::Rev(rev) => match on_error {
            OnError::Propagate => blob_io::read_blob(repo, rev, end.path)?,
            OnError::InFile => blob_io::read_blob_or_skip(repo, rev, end.path),
        },
    })
}

/// Los otros archivos de una versión, donde Magic busca las sub-celdas.
pub fn version_files<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    version: Version<'_>,
) -> Option<Arc<dyn FileSource>> {
    match version {
        Version::Absent => None,
        Version::WorkTree => files::workdir_files(workdir),
        Version::Rev(rev) => files::commit_files(repo, rev),
    }
}

/// [`version_files`] de los dos lados.
pub fn sources<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    before: Version<'_>,
    after: Version<'_>,
) -> DiffFiles {
    DiffFiles::new(version_files(repo, workdir, before), version_files(repo, workdir, after))
}

/// El diff de un archivo entre dos versiones. El módulo lo elige quien
/// llama (cada comando decide qué hacer con un formato sin módulo). Un lado
/// que existe pero no se pudo leer deja el archivo con error, sin llamar al
/// módulo.
pub fn diff_pair<R: GitRepository + ?Sized>(
    repo: &R,
    workdir: Option<&Path>,
    module: &dyn FormatModule,
    before: End<'_>,
    after: End<'_>,
    opts: &DiffOptions,
    on_error: OnError,
) -> Result<FileChange, AnalyzeError> {
    let a = read(repo, workdir, before, on_error)?;
    let b = read(repo, workdir, after, on_error)?;
    let files = sources(repo, workdir, before.version, after.version);
    let mut report = pipeline::diff_blobs(module, &a, &b, after.path, opts, &files);
    // Un archivo pedido que no está en ninguna de las dos versiones (una
    // ruta mal escrita) no es "sin cambios".
    if a.is_missing() && b.is_missing() && before.version != Version::Absent && after.version != Version::Absent {
        let path = after.path;
        report.warnings.push(format!("{path}: no existe en {} ni en {}", before.version.label(), after.version.label()));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_del_visor() {
        assert_eq!(Version::from_token(""), Version::Absent);
        assert_eq!(Version::from_token(WORKTREE), Version::WorkTree);
        assert_eq!(Version::from_token("HEAD~1"), Version::Rev("HEAD~1"));
    }
}
