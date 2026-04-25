use std::path::{Path, PathBuf};

use git2::{DiffOptions, Oid, Repository};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::ports::RepoRoot;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub oid: String,
    pub short_id: String,
    pub message: String,
    pub author: String,
    pub timestamp: i64,
}

/// Como `CommitInfo`, pero con los OIDs de los padres para distinguir merge
/// commits (más de un padre) y enlaces de historia. Solo lo emite el método
/// `get_commits_with_options`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitWithParents {
    pub info: CommitInfo,
    /// OIDs (formato hex) de los padres. Vacío para el commit root.
    pub parents: Vec<String>,
}

/// Filtros opcionales para recorrido de historia.
#[derive(Debug, Default, Clone)]
pub struct LogQuery<'a> {
    /// Si está, solo se incluyen commits que tocan ese archivo.
    pub file_path: Option<&'a str>,
    /// Límite duro de commits devueltos. `None` = sin límite.
    pub limit: Option<usize>,
    /// Si está, comienza desde ese ref/oid en lugar de `HEAD`.
    pub start: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: ChangeStatus,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Added,
    Removed,
    Modified,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingChange {
    pub path: String,
    pub status: ChangeStatus,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub head_oid: String,
    pub head_short: String,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("no se encontro un repo Git desde {0}")]
    RepositoryNotFound(PathBuf),
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("commit no encontrado: {0}")]
    CommitNotFound(String),
    #[error("archivo no encontrado en commit {commit}: {path}")]
    BlobNotFound { commit: String, path: String },
    #[error("blob demasiado grande ({size} bytes) en {path}")]
    LargeBlob { path: String, size: usize },
}

pub const LARGE_BLOB_THRESHOLD: usize = 50 * 1024 * 1024;

pub struct GitService {
    repo: Repository,
}

impl GitService {
    pub fn open(repo_path: &Path) -> Result<Self, GitError> {
        let repo = Repository::open(repo_path)
            .or_else(|_| {
                let dot_git = repo_path.join(".git");
                if dot_git.is_dir() {
                    Repository::open(dot_git)
                } else {
                    Repository::discover(repo_path)
                }
            })
            .map_err(|_| GitError::RepositoryNotFound(repo_path.to_path_buf()))?;
        Ok(Self { repo })
    }

    pub fn get_blob(&self, commit_ish: &str, file_path: &str) -> Result<Vec<u8>, GitError> {
        let commit = self.resolve_commit(commit_ish)?;
        let blob_id = self.tree_entry_id(commit.tree()?, file_path)?;
        let blob = self.repo.find_blob(blob_id)?;
        let size = blob.size();
        if size > LARGE_BLOB_THRESHOLD {
            return Err(GitError::LargeBlob {
                path: file_path.to_string(),
                size,
            });
        }
        Ok(blob.content().to_vec())
    }

    pub fn get_commits(&self, file_path: Option<&str>) -> Result<Vec<CommitInfo>, GitError> {
        let head = self
            .repo
            .head()?
            .target()
            .ok_or_else(|| GitError::CommitNotFound("HEAD".to_string()))?;
        let mut walker = self.repo.revwalk()?;
        walker.push(head)?;
        walker.set_sorting(git2::Sort::TIME)?;

        let mut results = Vec::new();
        for oid in walker {
            let oid = oid?;
            let commit = self.repo.find_commit(oid)?;
            if let Some(file_path) = file_path {
                if !self.commit_touches(&commit, file_path)? {
                    continue;
                }
            }
            results.push(CommitInfo {
                oid: commit.id().to_string(),
                short_id: commit.id().to_string().chars().take(7).collect(),
                message: commit.message().unwrap_or("").trim().to_string(),
                author: commit.author().name().unwrap_or("").to_string(),
                timestamp: commit.author().when().seconds(),
            });
        }
        Ok(results)
    }

    pub fn get_changed_files(
        &self,
        commit_a: &str,
        commit_b: &str,
    ) -> Result<Vec<ChangedFile>, GitError> {
        let tree_a = self.resolve_commit(commit_a)?.tree()?;
        let tree_b = self.resolve_commit(commit_b)?.tree()?;
        let mut options = DiffOptions::new();
        let mut diff =
            self.repo
                .diff_tree_to_tree(Some(&tree_a), Some(&tree_b), Some(&mut options))?;
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

    /// Cambios en el working tree (incluyendo staged) respecto a HEAD.
    ///
    /// Combina índice y working tree en una sola lista — es lo que el usuario
    /// percibe como "qué he tocado". Para casos avanzados (staged vs unstaged)
    /// se pueden añadir métodos separados, pero la versión 1 los unifica.
    pub fn working_tree_changes(&self) -> Result<Vec<WorkingChange>, GitError> {
        let mut options = git2::StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true);
        let statuses = self.repo.statuses(Some(&mut options))?;

        let mut results = Vec::new();
        for entry in statuses.iter() {
            let st = entry.status();
            if st.is_ignored() {
                continue;
            }
            let path = match entry.path() {
                Some(p) => p.to_string(),
                None => continue,
            };
            let (status, old_path) = classify_status(st, entry.head_to_index(), entry.index_to_workdir());
            results.push(WorkingChange { path, status, old_path });
        }
        Ok(results)
    }

    /// Información de la rama actual y su relación con upstream (si existe).
    pub fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
        let head = match self.repo.head() {
            Ok(h) => h,
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch
                || e.code() == git2::ErrorCode::NotFound =>
            {
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };

        let head_oid = head
            .target()
            .ok_or_else(|| GitError::CommitNotFound("HEAD".to_string()))?;
        let head_oid_str = head_oid.to_string();
        let head_short: String = head_oid_str.chars().take(7).collect();

        let name = if head.is_branch() {
            head.shorthand().unwrap_or("HEAD").to_string()
        } else {
            "HEAD (detached)".to_string()
        };

        let (upstream, ahead, behind) = if head.is_branch() {
            self.upstream_relation(&head)?
        } else {
            (None, 0, 0)
        };

        Ok(Some(BranchInfo {
            name,
            head_oid: head_oid_str,
            head_short,
            upstream,
            ahead,
            behind,
        }))
    }

    /// Recorre historia con filtros opcionales (`LogQuery`) y emite commits con
    /// sus padres. Útil para distinguir merges en `riku log`.
    pub fn get_commits_with_options(
        &self,
        query: &LogQuery<'_>,
    ) -> Result<Vec<CommitWithParents>, GitError> {
        let start_oid = match query.start {
            Some(refish) => self.resolve_commit(refish)?.id(),
            None => self
                .repo
                .head()?
                .target()
                .ok_or_else(|| GitError::CommitNotFound("HEAD".to_string()))?,
        };
        let mut walker = self.repo.revwalk()?;
        walker.push(start_oid)?;
        walker.set_sorting(git2::Sort::TIME)?;

        let limit = query.limit.unwrap_or(usize::MAX);
        let mut results = Vec::new();
        for oid in walker {
            if results.len() >= limit {
                break;
            }
            let oid = oid?;
            let commit = self.repo.find_commit(oid)?;
            if let Some(file_path) = query.file_path {
                if !self.commit_touches(&commit, file_path)? {
                    continue;
                }
            }
            let info = CommitInfo {
                oid: commit.id().to_string(),
                short_id: commit.id().to_string().chars().take(7).collect(),
                message: commit.message().unwrap_or("").trim().to_string(),
                author: commit.author().name().unwrap_or("").to_string(),
                timestamp: commit.author().when().seconds(),
            };
            let parents = (0..commit.parent_count())
                .filter_map(|i| commit.parent_id(i).ok())
                .map(|p| p.to_string())
                .collect();
            results.push(CommitWithParents { info, parents });
        }
        Ok(results)
    }

    /// Mapa `oid → [ref names]` de las refs locales (ramas + tags) que apuntan
    /// a algún commit. Útil para anotar el log con etiquetas.
    ///
    /// Las ramas remotas se incluyen con prefijo `remotes/origin/...` para
    /// poder distinguirlas. HEAD aparece como entrada propia si está resuelto.
    pub fn refs_by_oid(&self) -> Result<std::collections::HashMap<String, Vec<String>>, GitError> {
        let mut map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        // HEAD primero, para que aparezca al inicio en el orden de inserción.
        if let Ok(head) = self.repo.head() {
            if let Some(oid) = head.target() {
                map.entry(oid.to_string()).or_default().push("HEAD".to_string());
            }
        }

        let refs = self.repo.references()?;
        for r in refs.flatten() {
            let target = match r.target() {
                Some(o) => o,
                None => continue,
            };
            let name = match r.shorthand() {
                Some(n) => n.to_string(),
                None => continue,
            };
            map.entry(target.to_string()).or_default().push(name);
        }
        Ok(map)
    }

    fn upstream_relation(
        &self,
        head: &git2::Reference<'_>,
    ) -> Result<(Option<String>, usize, usize), GitError> {
        let branch_name = match head.shorthand() {
            Some(n) => n,
            None => return Ok((None, 0, 0)),
        };
        let branch = match self.repo.find_branch(branch_name, git2::BranchType::Local) {
            Ok(b) => b,
            Err(_) => return Ok((None, 0, 0)),
        };
        let upstream = match branch.upstream() {
            Ok(u) => u,
            Err(_) => return Ok((None, 0, 0)),
        };
        let upstream_name = upstream
            .name()
            .ok()
            .flatten()
            .map(|s| s.to_string());
        let local_oid = head.target().unwrap_or_else(git2::Oid::zero);
        let upstream_oid = upstream
            .get()
            .target()
            .unwrap_or_else(git2::Oid::zero);
        let (ahead, behind) = self
            .repo
            .graph_ahead_behind(local_oid, upstream_oid)
            .unwrap_or((0, 0));
        Ok((upstream_name, ahead, behind))
    }

    fn resolve_commit(&self, commit_ish: &str) -> Result<git2::Commit<'_>, GitError> {
        let obj = self.repo.revparse_single(commit_ish)?;
        let commit = obj.peel_to_commit()?;
        Ok(commit)
    }

    fn tree_entry_id(&self, tree: git2::Tree<'_>, file_path: &str) -> Result<Oid, GitError> {
        let mut node = tree;
        let mut parts = Path::new(file_path).components().peekable();
        while let Some(part) = parts.next() {
            let name = part.as_os_str().to_string_lossy();
            if parts.peek().is_some() {
                let oid = {
                    let entry = node.get_name(&name).ok_or_else(|| GitError::BlobNotFound {
                        commit: "tree".to_string(),
                        path: file_path.to_string(),
                    })?;
                    entry.id()
                };
                node = self.repo.find_tree(oid)?;
            } else {
                let entry = node.get_name(&name).ok_or_else(|| GitError::BlobNotFound {
                    commit: "tree".to_string(),
                    path: file_path.to_string(),
                })?;
                return Ok(entry.id());
            }
        }
        Err(GitError::BlobNotFound {
            commit: "tree".to_string(),
            path: file_path.to_string(),
        })
    }

    fn commit_touches(&self, commit: &git2::Commit<'_>, file_path: &str) -> Result<bool, GitError> {
        if commit.parent_count() == 0 {
            return Ok(self.tree_entry_id(commit.tree()?, file_path).is_ok());
        }

        let parent = commit.parent(0)?;
        let tree_a = parent.tree()?;
        let tree_b = commit.tree()?;
        let diff = self
            .repo
            .diff_tree_to_tree(Some(&tree_a), Some(&tree_b), None)?;
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
}

impl RepoRoot for GitService {
    fn root(&self) -> Option<&Path> {
        self.repo.workdir()
    }
}

fn classify_status(
    st: git2::Status,
    head_to_index: Option<git2::DiffDelta<'_>>,
    index_to_workdir: Option<git2::DiffDelta<'_>>,
) -> (ChangeStatus, Option<String>) {
    let renamed = st.contains(git2::Status::INDEX_RENAMED)
        || st.contains(git2::Status::WT_RENAMED);
    let added = st.contains(git2::Status::INDEX_NEW)
        || st.contains(git2::Status::WT_NEW);
    let removed = st.contains(git2::Status::INDEX_DELETED)
        || st.contains(git2::Status::WT_DELETED);

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
