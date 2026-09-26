use std::collections::HashMap;
use std::path::Path;

use git2::Repository;

use crate::core::domain::git_types::{
    BranchInfo, ChangedFile, CommitChanges, CommitInfo, CommitWithParents, GitError, LogQuery,
    WorkingChange,
};
use crate::core::domain::ports::{GitRepository, RepoRoot};
use crate::core::git::{blob, branch, commit_log, diff, working_tree};

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
}

impl RepoRoot for GitService {
    fn root(&self) -> Option<&Path> {
        self.repo.workdir()
    }
}

impl GitRepository for GitService {
    fn get_blob(&self, commit_ish: &str, file_path: &str) -> Result<Vec<u8>, GitError> {
        blob::get_blob(&self.repo, commit_ish, file_path)
    }

    fn get_commits(&self, file_path: Option<&str>) -> Result<Vec<CommitInfo>, GitError> {
        commit_log::get_commits(&self.repo, file_path)
    }

    fn get_changed_files(
        &self,
        commit_a: &str,
        commit_b: &str,
    ) -> Result<Vec<ChangedFile>, GitError> {
        diff::get_changed_files(&self.repo, commit_a, commit_b)
    }

    fn working_tree_changes(&self) -> Result<Vec<WorkingChange>, GitError> {
        working_tree::working_tree_changes(&self.repo)
    }

    fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
        branch::current_branch(&self.repo)
    }

    fn get_commits_with_options(
        &self,
        query: &LogQuery<'_>,
    ) -> Result<Vec<CommitWithParents>, GitError> {
        commit_log::get_commits_with_options(&self.repo, query)
    }

    fn refs_by_oid(&self) -> Result<HashMap<String, Vec<String>>, GitError> {
        branch::refs_by_oid(&self.repo)
    }

    fn commit_changes(&self, commit_ish: &str) -> Result<CommitChanges, GitError> {
        diff::commit_changes(&self.repo, commit_ish)
    }
}
