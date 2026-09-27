//! Lectura de una versión de un archivo (un blob de Git o el disco) para
//! compararla.
//!
//! Distingue "no existe en esa versión" (se compara contra vacío: todo
//! añadido o eliminado) de "existe pero no se puede comparar" (demasiado
//! grande o ilegible): lo segundo no debe llegar al módulo como vacío, o el
//! diff diría que se borró todo.

use std::path::Path;

use crate::core::domain::git_types::{GitError, LARGE_BLOB_THRESHOLD};
use crate::core::domain::ports::GitRepository;

/// Una versión de un archivo.
#[derive(Debug, PartialEq, Eq)]
pub enum Blob {
    Bytes(Vec<u8>),
    /// No existe en esa versión.
    Missing,
    /// Existe pero no se puede comparar; el texto dice por qué.
    Skipped(String),
}

impl Blob {
    /// El contenido para el módulo: vacío si no existe.
    pub fn bytes(&self) -> &[u8] {
        match self {
            Blob::Bytes(b) => b,
            _ => &[],
        }
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, Blob::Missing)
    }

    pub fn skipped(&self) -> Option<&str> {
        match self {
            Blob::Skipped(why) => Some(why),
            _ => None,
        }
    }
}

fn too_large(path: &str, size: u64) -> String {
    format!(
        "{path}: {} MB, más que el límite de {} MB; no se compara",
        size / (1024 * 1024),
        LARGE_BLOB_THRESHOLD / (1024 * 1024)
    )
}

/// `path` en `commit`. Un error de Git que no es del archivo (commit
/// inexistente, repo roto) se propaga.
pub fn read_blob<R: GitRepository + ?Sized>(repo: &R, commit: &str, path: &str) -> Result<Blob, GitError> {
    match repo.get_blob(commit, path) {
        Ok(bytes) => Ok(Blob::Bytes(bytes)),
        Err(GitError::BlobNotFound { .. }) => Ok(Blob::Missing),
        Err(GitError::LargeBlob { path, size }) => Ok(Blob::Skipped(too_large(&path, size as u64))),
        Err(e) => Err(e),
    }
}

/// Como [`read_blob`], pero todo error queda en el archivo (`log`, donde un
/// blob suelto no debe tumbar el historial entero).
pub fn read_blob_or_skip<R: GitRepository + ?Sized>(repo: &R, commit: &str, path: &str) -> Blob {
    read_blob(repo, commit, path).unwrap_or_else(|e| Blob::Skipped(format!("{path} en {commit}: {e}")))
}

/// `path` en el working tree, con el mismo límite de tamaño que Git.
pub fn read_disk(workdir: Option<&Path>, path: &str) -> Blob {
    let Some(root) = workdir else { return Blob::Missing };
    let full = root.join(path);
    match std::fs::metadata(&full) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Blob::Missing,
        Err(e) => return Blob::Skipped(format!("{path}: no se pudo leer: {e}")),
        Ok(m) if m.len() > LARGE_BLOB_THRESHOLD as u64 => return Blob::Skipped(too_large(path, m.len())),
        Ok(_) => {}
    }
    match std::fs::read(&full) {
        Ok(bytes) => Blob::Bytes(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Blob::Missing,
        Err(e) => Blob::Skipped(format!("{path}: no se pudo leer: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::git_types::{ChangedFile, CommitInfo};
    use std::collections::HashMap;

    /// Mock con respuestas configurables por (commit, path).
    struct MockRepo {
        responses: HashMap<(String, String), Result<Vec<u8>, GitError>>,
    }

    impl MockRepo {
        fn new() -> Self {
            Self {
                responses: HashMap::new(),
            }
        }
        fn set(&mut self, commit: &str, path: &str, result: Result<Vec<u8>, GitError>) {
            self.responses
                .insert((commit.to_string(), path.to_string()), result);
        }
    }

    impl GitRepository for MockRepo {
        fn get_blob(&self, commit: &str, path: &str) -> Result<Vec<u8>, GitError> {
            match self.responses.get(&(commit.to_string(), path.to_string())) {
                Some(Ok(bytes)) => Ok(bytes.clone()),
                Some(Err(GitError::BlobNotFound { commit, path })) => Err(GitError::BlobNotFound {
                    commit: commit.clone(),
                    path: path.clone(),
                }),
                Some(Err(GitError::LargeBlob { path, size })) => Err(GitError::LargeBlob {
                    path: path.clone(),
                    size: *size,
                }),
                Some(Err(GitError::CommitNotFound(s))) => Err(GitError::CommitNotFound(s.clone())),
                Some(Err(_)) | None => Err(GitError::CommitNotFound("mock-default".to_string())),
            }
        }
        fn get_commits(&self, _: Option<&str>) -> Result<Vec<CommitInfo>, GitError> {
            Ok(Vec::new())
        }
        fn get_changed_files(&self, _: &str, _: &str) -> Result<Vec<ChangedFile>, GitError> {
            Ok(Vec::new())
        }
    }

    fn blob_err(e: GitError) -> Blob {
        let mut repo = MockRepo::new();
        repo.set("HEAD", "a.gds", Err(e));
        read_blob(&repo, "HEAD", "a.gds").unwrap()
    }

    #[test]
    fn existe_falta_o_se_omite() {
        let mut repo = MockRepo::new();
        repo.set("HEAD", "a.sch", Ok(b"hello".to_vec()));
        assert_eq!(read_blob(&repo, "HEAD", "a.sch").unwrap(), Blob::Bytes(b"hello".to_vec()));

        let missing = blob_err(GitError::BlobNotFound { commit: "HEAD".into(), path: "a.gds".into() });
        assert!(missing.is_missing() && missing.bytes().is_empty());

        let big = blob_err(GitError::LargeBlob { path: "big.gds".into(), size: 99 * 1024 * 1024 });
        let why = big.skipped().expect("un blob grande se omite, no es vacío");
        assert!(why.contains("big.gds") && why.contains("99 MB"), "{why}");
    }

    #[test]
    fn otro_error_se_propaga_o_se_omite() {
        let mut repo = MockRepo::new();
        repo.set("HEAD", "x.sch", Err(GitError::CommitNotFound("HEAD".into())));
        assert!(matches!(read_blob(&repo, "HEAD", "x.sch"), Err(GitError::CommitNotFound(_))));
        let why = read_blob_or_skip(&repo, "HEAD", "x.sch");
        assert!(why.skipped().is_some_and(|w| w.contains("x.sch") && w.contains("HEAD")), "{why:?}");
    }

    #[test]
    fn disco() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.sch"), b"x").unwrap();
        assert_eq!(read_disk(Some(dir.path()), "a.sch"), Blob::Bytes(b"x".to_vec()));
        assert!(read_disk(Some(dir.path()), "no.sch").is_missing());
        assert!(read_disk(None, "a.sch").is_missing());
        // Un directorio con el nombre del archivo: existe pero no se lee.
        std::fs::create_dir(dir.path().join("d.sch")).unwrap();
        assert!(read_disk(Some(dir.path()), "d.sch").skipped().is_some());
    }
}
