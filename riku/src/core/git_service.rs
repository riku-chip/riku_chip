use std::path::{Path, PathBuf};

use git2::{DiffOptions, Oid, Repository};
use thiserror::Error;

use crate::core::ports::RepoRoot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub oid: String,
    pub short_id: String,
    pub message: String,
    pub author: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: ChangeStatus,
    pub old_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeStatus {
    Added,
    Removed,
    Modified,
    Renamed,
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
