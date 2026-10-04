use git2::Repository;

use crate::core::domain::git_types::{ChangeStatus, GitError, WorkingChange};

/// Cambios en el working tree (incluyendo staged) respecto a HEAD.
///
/// Combina índice y working tree en una sola lista — es lo que el usuario
/// percibe como "qué he tocado". Para casos avanzados (staged vs unstaged)
/// se pueden añadir métodos separados, pero la versión 1 los unifica.
pub(super) fn working_tree_changes(repo: &Repository) -> Result<Vec<WorkingChange>, GitError> {
    let mut options = git2::StatusOptions::new();
    options.include_untracked(true).recurse_untracked_dirs(true).renames_head_to_index(true).renames_index_to_workdir(true);
    let statuses = repo.statuses(Some(&mut options))?;

    let mut results = Vec::new();
    for entry in statuses.iter() {
        let st = entry.status();
        if st.is_ignored() {
            continue;
        }
        let (status, old_path) = classify_status(st, entry.head_to_index(), entry.index_to_workdir());
        // En un renombre, `entry.path()` es la ruta vieja: la nueva está en
        // el delta (el último que hay: índice → disco, o HEAD → índice).
        let new_path = old_path.as_ref().and_then(|_| {
            entry
                .index_to_workdir()
                .or(entry.head_to_index())
                .and_then(|d| d.new_file().path().map(|p| p.to_string_lossy().to_string()))
        });
        let path = match new_path.or_else(|| entry.path().ok().map(str::to_string)) {
            Some(p) => p,
            None => continue,
        };
        results.push(WorkingChange { path, status, old_path });
    }
    Ok(results)
}

fn classify_status(
    st: git2::Status,
    head_to_index: Option<git2::DiffDelta<'_>>,
    index_to_workdir: Option<git2::DiffDelta<'_>>,
) -> (ChangeStatus, Option<String>) {
    let renamed = st.contains(git2::Status::INDEX_RENAMED) || st.contains(git2::Status::WT_RENAMED);
    let added = st.contains(git2::Status::INDEX_NEW) || st.contains(git2::Status::WT_NEW);
    let removed = st.contains(git2::Status::INDEX_DELETED) || st.contains(git2::Status::WT_DELETED);

    let old_path = if renamed {
        head_to_index
            .as_ref()
            .or(index_to_workdir.as_ref())
            .and_then(|d| d.old_file().path())
            .map(|p| p.to_string_lossy().to_string())
    } else {
        None
    };

    let status = if renamed {
        ChangeStatus::Renamed
    } else if removed {
        ChangeStatus::Removed
    } else if added {
        ChangeStatus::Added
    } else {
        ChangeStatus::Modified
    };
    (status, old_path)
}
