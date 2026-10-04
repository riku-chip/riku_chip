use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::core::domain::git_types::{
    BranchInfo, ChangedFile, CommitChanges, CommitWithParents, GitError, LogQuery, WorkingChange,
};

/// Abre otra conexión al mismo repositorio. `git2::Repository` se puede
/// mover entre hilos pero no compartir: cada hilo que lee Git usa la suya.
pub type Reopener = Arc<dyn Fn() -> Result<Box<dyn GitRepository + Send>, GitError> + Send + Sync>;

/// Lo que el núcleo le pide a Git. Sin implementaciones por defecto que
/// devuelvan "vacío": un adaptador nuevo que olvide un método no compila (en
/// vez de decir "sin cambios"). Los dos que sí tienen default responden
/// "no sé", que es una respuesta válida.
pub trait GitRepository {
    fn get_blob(&self, commit_ish: &str, file_path: &str) -> Result<Vec<u8>, GitError>;

    fn get_changed_files(&self, commit_a: &str, commit_b: &str) -> Result<Vec<ChangedFile>, GitError>;

    /// Cambios en el working tree respecto a HEAD.
    fn working_tree_changes(&self) -> Result<Vec<WorkingChange>, GitError>;

    /// La rama actual; `None` con HEAD desacoplado o sin commits.
    fn current_branch(&self) -> Result<Option<BranchInfo>, GitError>;

    /// Commits con sus padres, filtrados y limitados según `query`.
    fn get_commits_with_options(&self, query: &LogQuery<'_>) -> Result<Vec<CommitWithParents>, GitError>;

    /// Mapa `oid → [refs]` para anotar el log.
    fn refs_by_oid(&self) -> Result<HashMap<String, Vec<String>>, GitError>;

    /// Un commit, sus padres y los archivos que cambió respecto al primero
    /// (`riku show`).
    fn commit_changes(&self, commit_ish: &str) -> Result<CommitChanges, GitError>;

    /// Tamaño en bytes de un blob sin leerlo (para planificar la memoria de
    /// los diffs en paralelo). `None` si no existe o no se sabe: todo va en
    /// una tanda.
    fn blob_size(&self, _commit_ish: &str, _file_path: &str) -> Option<u64> {
        None
    }

    /// Cómo abrir otra conexión a este repositorio desde otro hilo. `None`
    /// (p. ej. en los mocks de los tests): todo se hace en secuencia con esta.
    fn reopener(&self) -> Option<Reopener> {
        None
    }
}

pub trait RepoRoot {
    fn root(&self) -> Option<&Path>;
}
