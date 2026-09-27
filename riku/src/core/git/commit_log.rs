use std::path::Path;

use git2::{Commit, Repository};

use crate::core::domain::git_types::{CommitInfo, CommitWithParents, GitError, LogQuery};
use crate::core::git::helpers::{commit_info_from, resolve_commit};

pub(super) fn get_commits(
    repo: &Repository,
    file_path: Option<&str>,
) -> Result<Vec<CommitInfo>, GitError> {
    let head = repo
        .head()?
        .target()
        .ok_or_else(|| GitError::CommitNotFound("HEAD".to_string()))?;
    let mut walker = repo.revwalk()?;
    walker.push(head)?;
    walker.set_sorting(git2::Sort::TIME)?;

    let mut results = Vec::new();
    for oid in walker {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        if let Some(file_path) = file_path {
            if !commit_touches(repo, &commit, file_path)? {
                continue;
            }
        }
        results.push(commit_info_from(&commit));
    }
    Ok(results)
}

pub(super) fn get_commits_with_options(
    repo: &Repository,
    query: &LogQuery<'_>,
) -> Result<Vec<CommitWithParents>, GitError> {
    let start_oid = match query.start {
        Some(refish) => resolve_commit(repo, refish)?.id(),
        None => repo
            .head()?
            .target()
            .ok_or_else(|| GitError::CommitNotFound("HEAD".to_string()))?,
    };
    let mut walker = repo.revwalk()?;
    walker.push(start_oid)?;
    let sort = if query.topological { git2::Sort::TOPOLOGICAL | git2::Sort::TIME } else { git2::Sort::TIME };
    walker.set_sorting(sort)?;

    let limit = query.limit.unwrap_or(usize::MAX);
    let mut results = Vec::new();
    for oid in walker {
        if results.len() >= limit {
            break;
        }
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        if let Some(file_path) = query.file_path {
            if !commit_touches(repo, &commit, file_path)? {
                continue;
            }
        }
        let info = commit_info_from(&commit);
        let parents = (0..commit.parent_count())
            .filter_map(|i| commit.parent_id(i).ok())
            .map(|p| p.to_string())
            .collect();
        results.push(CommitWithParents { info, parents });
    }
    Ok(results)
}

fn commit_touches(
    repo: &Repository,
    commit: &Commit<'_>,
    file_path: &str,
) -> Result<bool, GitError> {
    if commit.parent_count() == 0 {
        return Ok(super::blob::tree_entry_id(repo, commit.tree()?, file_path).is_ok());
    }

    let parent = commit.parent(0)?;
    let tree_a = parent.tree()?;
    let tree_b = commit.tree()?;
    let diff = repo.diff_tree_to_tree(Some(&tree_a), Some(&tree_b), None)?;
    for delta in diff.deltas() {
        if delta
            .new_file()
            .path()
            .map(|p| p == Path::new(file_path))
            .unwrap_or(false)
            || delta
                .old_file()
                .path()
                .map(|p| p == Path::new(file_path))
                .unwrap_or(false)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
