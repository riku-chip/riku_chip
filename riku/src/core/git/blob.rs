use std::path::Path;

use git2::{Oid, Repository, Tree};

use crate::core::domain::git_types::{GitError, LARGE_BLOB_THRESHOLD};
use crate::core::git::helpers::resolve_commit;

pub(super) fn get_blob(repo: &Repository, commit_ish: &str, file_path: &str) -> Result<Vec<u8>, GitError> {
    let commit = resolve_commit(repo, commit_ish)?;
    let blob_id = tree_entry_id(repo, commit.tree()?, file_path)?;
    // El tamaño sale de la cabecera del objeto: un blob demasiado grande se
    // rechaza sin descomprimirlo entero.
    let (size, _) = repo.odb()?.read_header(blob_id)?;
    if size > LARGE_BLOB_THRESHOLD {
        return Err(GitError::LargeBlob { path: file_path.to_string(), size });
    }
    Ok(repo.find_blob(blob_id)?.content().to_vec())
}

/// Tamaño de un blob leyendo solo la cabecera del objeto.
pub(super) fn blob_size(repo: &Repository, commit_ish: &str, file_path: &str) -> Option<u64> {
    let commit = resolve_commit(repo, commit_ish).ok()?;
    let id = tree_entry_id(repo, commit.tree().ok()?, file_path).ok()?;
    let (size, _) = repo.odb().ok()?.read_header(id).ok()?;
    Some(size as u64)
}

pub(super) fn tree_entry_id(repo: &Repository, tree: Tree<'_>, file_path: &str) -> Result<Oid, GitError> {
    let mut node = tree;
    let mut parts = Path::new(file_path).components().peekable();
    while let Some(part) = parts.next() {
        let name = part.as_os_str().to_string_lossy();
        if parts.peek().is_some() {
            let oid = {
                let entry = node
                    .get_name(&name)
                    .ok_or_else(|| GitError::BlobNotFound { commit: "tree".to_string(), path: file_path.to_string() })?;
                entry.id()
            };
            node = repo.find_tree(oid)?;
        } else {
            let entry = node
                .get_name(&name)
                .ok_or_else(|| GitError::BlobNotFound { commit: "tree".to_string(), path: file_path.to_string() })?;
            return Ok(entry.id());
        }
    }
    Err(GitError::BlobNotFound { commit: "tree".to_string(), path: file_path.to_string() })
}
