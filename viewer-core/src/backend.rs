//! Trait asíncrono que cualquier backend de visor debe implementar.
//!
//! El contrato es intencionalmente estrecho:
//!
//! - `info()` — identifica al backend (nombre, formatos, versión).
//! - `accepts()` — decide si el backend puede procesar un blob dado.
//! - `load()` — parsea y construye una [`SceneHandle`] lista para renderizar.
//!
//! `async_trait` es obligatorio porque necesitamos `Box<dyn ViewerBackend>` y
//! Rust aún no soporta `async fn` en traits con objetos dinámicos de forma nativa.

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use std::sync::Arc;

use crate::error::{Result, ViewerError};
use crate::files::{DiffFiles, FileSource};
use crate::scene::SceneHandle;

/// Metadatos de un backend — devueltos por `info()` para UI y diagnóstico.
#[derive(Debug, Clone)]
pub struct BackendInfo {
    /// Identificador corto (`"xschem"`, `"gds"`).
    pub name: &'static str,
    /// Versión del backend (útil para doctor/diagnóstico).
    pub version: &'static str,
    /// Extensiones aceptadas, sin punto (`["sch", "sym"]`).
    pub extensions: &'static [&'static str],
}

/// Trait principal. Un backend típico lo implementa como unit struct sin estado;
/// cualquier cache compartida debe vivir dentro de `Arc<...>` interno.
#[async_trait]
pub trait ViewerBackend: Send + Sync {
    fn info(&self) -> BackendInfo;

    /// ¿Puede este backend procesar el contenido dado? La decisión puede basarse
    /// en firma mágica, cabecera de texto o la extensión del path si se provee.
    fn accepts(&self, content: &[u8], path_hint: Option<&str>) -> bool;

    /// Parsea y construye una escena renderizable.
    ///
    /// `token` permite cancelación cooperativa — los backends que hagan trabajo
    /// CPU-bound pesado deben poll-earlo en puntos razonables (entre fases de
    /// parseo, entre celdas, etc.) y retornar [`ViewerError::Cancelled`] cuando
    /// corresponda.
    ///
    /// Se espera que las implementaciones ejecuten el trabajo pesado dentro de
    /// `tokio::task::spawn_blocking` y propaguen el `JoinError` como
    /// [`ViewerError::Join`].
    async fn load(&self, content: Vec<u8>, path_hint: Option<String>, token: CancellationToken) -> Result<SceneHandle>;

    /// Carga una sub-vista concreta del archivo (ver [`crate::scene::ViewEntry`]).
    /// `None` = la que el backend elige por defecto, igual que [`Self::load`].
    ///
    /// Los backends sin sub-vistas no necesitan implementarlo: por defecto
    /// ignora `entry` y delega en `load`.
    async fn load_entry(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        token: CancellationToken,
    ) -> Result<SceneHandle> {
        let _ = entry;
        self.load(content, path_hint, token).await
    }

    /// Escena de diff entre dos versiones del archivo: geometría resaltada y
    /// lista de cambios (`RenderableScene::changes`). `before` vacío = el
    /// archivo no existía en esa versión. `entry` elige la sub-vista.
    ///
    /// Por defecto no soportado: cada backend decide si puede comparar.
    async fn load_diff(
        &self,
        before: Vec<u8>,
        after: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        token: CancellationToken,
    ) -> Result<SceneHandle> {
        let _ = (before, after, path_hint, entry, token);
        Err(ViewerError::Unsupported(format!("{}: diff no soportado", self.info().name)))
    }

    /// Como [`Self::load_entry`], con acceso a los otros archivos de la misma
    /// versión (formatos repartidos en varios archivos, como Magic).
    ///
    /// Por defecto ignora `files` y delega en `load_entry`: un backend de un
    /// solo archivo no necesita implementarlo.
    async fn load_with(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        files: Option<Arc<dyn FileSource>>,
        token: CancellationToken,
    ) -> Result<SceneHandle> {
        let _ = files;
        self.load_entry(content, path_hint, entry, token).await
    }

    /// Como [`Self::load_diff`], con los otros archivos de cada versión.
    /// Por defecto ignora `files` y delega en `load_diff`.
    async fn load_diff_with(
        &self,
        before: Vec<u8>,
        after: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        files: DiffFiles,
        token: CancellationToken,
    ) -> Result<SceneHandle> {
        let _ = files;
        self.load_diff(before, after, path_hint, entry, token).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::scene::Scene;

    struct Plain;

    #[async_trait]
    impl ViewerBackend for Plain {
        fn info(&self) -> BackendInfo {
            BackendInfo { name: "plain", version: "0", extensions: &[] }
        }
        fn accepts(&self, _: &[u8], _: Option<&str>) -> bool {
            true
        }
        async fn load(&self, _: Vec<u8>, _: Option<String>, _: CancellationToken) -> Result<SceneHandle> {
            Ok(Arc::new(Scene::new()))
        }
    }

    #[tokio::test]
    async fn default_load_entry_delegates_to_load() {
        let s = Plain.load_entry(Vec::new(), None, Some("X".into()), CancellationToken::new()).await.expect("load_entry");
        assert!(s.entries().is_empty());
        assert!(s.current_entry().is_none());
        assert!(s.changes().is_empty());
    }

    #[tokio::test]
    async fn default_load_diff_is_unsupported() {
        let r = Plain.load_diff(Vec::new(), Vec::new(), None, None, CancellationToken::new()).await;
        assert!(matches!(r, Err(ViewerError::Unsupported(_))));
    }

    #[tokio::test]
    async fn the_variants_with_files_delegate_by_default() {
        let files: Arc<dyn FileSource> = Arc::new(crate::files::DiskFiles::new("."));
        let s = Plain.load_with(Vec::new(), None, None, Some(files.clone()), CancellationToken::new()).await;
        assert!(s.is_ok());
        let both = DiffFiles::new(Some(files.clone()), Some(files));
        let r = Plain.load_diff_with(Vec::new(), Vec::new(), None, None, both, CancellationToken::new()).await;
        assert!(matches!(r, Err(ViewerError::Unsupported(_))));
    }
}
