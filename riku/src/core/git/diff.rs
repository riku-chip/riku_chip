use git2::{Diff, DiffOptions, Repository};

use crate::core::domain::git_types::{ChangeStatus, ChangedFile, CommitChanges, CommitWithParents, GitError};
use crate::core::git::helpers::{commit_info_from, resolve_commit};

pub(super) fn get_changed_files(
    repo: &Repository,
    commit_a: &str,
    commit_b: &str,
) -> Result<Vec<ChangedFile>, GitError> {
    let tree_a = resolve_commit(repo, commit_a)?.tree()?;
    let tree_b = resolve_commit(repo, commit_b)?.tree()?;
    let mut options = DiffOptions::new();
    let diff =
        repo.diff_tree_to_tree(Some(&tree_a), Some(&tree_b), Some(&mut options))?;
    changed_files(diff)
}

/// Un commit, sus padres y los archivos que cambió respecto al primero. El
/// commit inicial se compara contra un árbol vacío: todo aparece añadido.
pub(super) fn commit_changes(repo: &Repository, commit_ish: &str) -> Result<CommitChanges, GitError> {
    let commit = resolve_commit(repo, commit_ish)?;
    let parent_tree = match commit.parent(0) {
        Ok(parent) => Some(parent.tree()?),
        Err(_) => None,
    };
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), None)?;
    Ok(CommitChanges {
        commit: CommitWithParents {
            info: commit_info_from(&commit),
            parents: commit.parent_ids().map(|oid| oid.to_string()).collect(),
        },
        files: changed_files(diff)?,
    })
}

fn changed_files(mut diff: Diff<'_>) -> Result<Vec<ChangedFile>, GitError> {
    let mut find_options = git2::DiffFindOptions::new();
    diff.find_similar(Some(&mut find_options))?;

    let mut results = Vec::new();
    for delta in diff.deltas() {
        let status = match delta.status() {
            git2::Delta::Added => ChangeStatus::Added,
            git2::Delta::Deleted => ChangeStatus::Removed,
            git2::Delta::Modified => ChangeStatus::Modified,
            git2::Delta::Renamed => ChangeStatus::Renamed,
            _ => ChangeStatus::Modified,
        };
        let new_path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .ok_or_else(|| GitError::CommitNotFound("delta path missing".to_string()))?;
        results.push(ChangedFile {
            path: new_path.to_string_lossy().to_string(),
            status,
            old_path: delta
                .old_file()
                .path()
                .map(|p| p.to_string_lossy().to_string()),
        });
    }
    Ok(results)
}
