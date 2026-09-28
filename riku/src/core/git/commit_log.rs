use std::path::Path;

use git2::{Commit, Repository};

use crate::core::domain::git_types::{CommitWithParents, GitError, LogQuery};
use crate::core::git::helpers::{commit_info_from, resolve_commit};
use crate::core::path_matcher::PathMatcher;

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
    let matcher = PathMatcher::new(query.paths);
    let mut results = Vec::new();
    for oid in walker {
        if results.len() >= limit {
            break;
        }
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        // El filtro va antes del límite: `-n 20 --paths x` son los 20 más
        // recientes que tocan x, no los 20 más recientes filtrados.
        if !query.paths.is_empty() && !commit_touches(repo, &commit, |p| p.to_str().is_some_and(|p| matcher.matches(p)))? {
            continue;
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

/// `true` si el commit cambió, respecto a su primer padre (o a nada, si es
/// el inicial), algún archivo que cumple `wanted`. El diff de árboles de Git
/// salta los subárboles iguales por su oid: cuesta lo que cambió, no el
/// tamaño del repo.
fn commit_touches(repo: &Repository, commit: &Commit<'_>, wanted: impl Fn(&Path) -> bool) -> Result<bool, GitError> {
    let tree_a = match commit.parent_count() {
        0 => None,
        _ => Some(commit.parent(0)?.tree()?),
    };
    let tree_b = commit.tree()?;
    let diff = repo.diff_tree_to_tree(tree_a.as_ref(), Some(&tree_b), None)?;
    Ok(diff
        .deltas()
        .any(|d| d.new_file().path().is_some_and(&wanted) || d.old_file().path().is_some_and(&wanted)))
}
