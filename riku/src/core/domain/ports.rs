use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::core::domain::git_types::{
    BranchInfo, ChangedFile, CommitChanges, CommitInfo, CommitWithParents, GitError, LogQuery,
    WorkingChange,
};

/// Abre otra conexión al mismo repositorio. `git2::Repository` se puede
/// mover entre hilos pero no compartir: cada hilo que lee Git usa la suya.
pub type Reopener = Arc<dyn Fn() -> Result<Box<dyn GitRepository + Send>, GitError> + Send + Sync>;

pub trait GitRepository {
    fn get_blob(&self, commit_ish: &str, file_path: &str) -> Result<Vec<u8>, GitError>;

    fn get_commits(&self, file_path: Option<&str>) -> Result<Vec<CommitInfo>, GitError>;

    fn get_changed_files(
        &self,
        commit_a: &str,
        commit_b: &str,
    ) -> Result<Vec<ChangedFile>, GitError>;

    /// Cambios en working tree vs HEAD. Default `Ok(vec![])` para no romper
    /// implementaciones existentes (mocks de tests, futuros adaptadores).
    fn working_tree_changes(&self) -> Result<Vec<WorkingChange>, GitError> {
        Ok(Vec::new())
    }

    /// Información de la rama actual. Default `Ok(None)` para no forzar a
    /// cada adapter a implementarlo si no aplica (repo en estado inicial).
    fn current_branch(&self) -> Result<Option<BranchInfo>, GitError> {
        Ok(None)
    }

    /// Versión enriquecida de `get_commits` con filtros y padres por commit.
    /// Default delega a `get_commits` y sintetiza padres vacíos para no romper
    /// adapters existentes.
    fn get_commits_with_options(
        &self,
        query: &LogQuery<'_>,
    ) -> Result<Vec<CommitWithParents>, GitError> {
        let mut commits = self.get_commits(query.file_path)?;
        if let Some(limit) = query.limit {
            commits.truncate(limit);
        }
        Ok(commits
            .into_iter()
            .map(|info| CommitWithParents {
                info,
                parents: Vec::new(),
            })
            .collect())
    }

    /// Mapa `oid → [refs]` para anotar el log. Default vacío.
    fn refs_by_oid(&self) -> Result<HashMap<String, Vec<String>>, GitError> {
        Ok(HashMap::new())
    }

    /// Un commit, sus padres y los archivos que cambió respecto al primero
    /// (`riku show`). Default: error, para no forzar a los mocks.
    fn commit_changes(&self, commit_ish: &str) -> Result<CommitChanges, GitError> {
        Err(GitError::CommitNotFound(commit_ish.to_string()))
    }

    /// Tamaño en bytes de un blob sin leerlo (para planificar la memoria de
    /// los diffs en paralelo). `None` si no existe o no se sabe.
    fn blob_size(&self, _commit_ish: &str, _file_path: &str) -> Option<u64> {
        None
    }

    /// Cómo abrir otra conexión a este repositorio desde otro hilo. `None`
    /// (el default, p. ej. en los mocks de los tests): todo se hace en
    /// secuencia con esta.
    fn reopener(&self) -> Option<Reopener> {
        None
    }
}

pub trait RepoRoot {
    fn root(&self) -> Option<&Path>;
}
