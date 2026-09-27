//! Los archivos de un commit como [`FileSource`]: un módulo que necesita
//! otros archivos de la misma versión (las sub-celdas de un `.mag`) los lee
//! del mismo commit, no del disco de hoy.

use std::sync::{Arc, Mutex};

use riku_kernel::{DiskFiles, FileSource};

use crate::core::domain::ports::{GitRepository, Reopener};

/// Archivos de un commit. Abre su propia conexión a Git la primera vez que
/// se le pide un archivo (así no cuesta nada a los formatos de un solo
/// archivo) y la usa desde cualquier hilo, de a uno.
pub struct GitFiles {
    reopen: Reopener,
    commit: String,
    repo: Mutex<Option<Box<dyn GitRepository + Send>>>,
}

impl GitFiles {
    pub fn new(reopen: Reopener, commit: impl Into<String>) -> Self {
        Self { reopen, commit: commit.into(), repo: Mutex::new(None) }
    }
}

impl FileSource for GitFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let mut repo = self.repo.lock().ok()?;
        if repo.is_none() {
            *repo = (self.reopen)().ok();
        }
        repo.as_ref()?.get_blob(&self.commit, path).ok()
    }

    fn describe(&self) -> String {
        format!("commit {}", self.commit.get(..7).unwrap_or(&self.commit))
    }
}

/// Fuente de los archivos de `commit` en `repo`; `None` si el repo no puede
/// abrir otra conexión (dobles de prueba).
pub fn commit_files<R: GitRepository + ?Sized>(repo: &R, commit: &str) -> Option<Arc<dyn FileSource>> {
    repo.reopener().map(|r| Arc::new(GitFiles::new(r, commit)) as Arc<dyn FileSource>)
}

/// Archivos del working tree (el disco bajo `workdir`).
pub fn workdir_files(workdir: Option<&std::path::Path>) -> Option<Arc<dyn FileSource>> {
    workdir.map(|w| Arc::new(DiskFiles::new(w)) as Arc<dyn FileSource>)
}
