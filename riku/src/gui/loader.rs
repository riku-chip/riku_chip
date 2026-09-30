//! Carga de escenas en segundo plano via `ViewerBackend`: una sola en vuelo
//! (la nueva cancela la anterior), y lo que llega se consulta por cuadro.

use std::sync::Arc;

use poll_promise::Promise;
use tokio::runtime::Runtime;
use viewer_core::{backend::ViewerBackend, error::ViewerError, scene::SceneHandle, CancellationToken};

use crate::gui::content::{DiffContext, DiffTab, LoadKind};

/// Qué cargar.
pub(crate) struct LoadRequest {
    pub backend: Arc<dyn ViewerBackend>,
    pub source: Arc<Vec<u8>>,
    pub path: String,
    /// Sub-vista (celda) o `None` para la de por defecto.
    pub entry: Option<String>,
    pub kind: LoadKind,
    /// Re-encuadrar al llegar (nueva celda). Al cambiar de pestaña del diff
    /// se conserva la vista para comparar la misma zona.
    pub refit: bool,
    /// El diff del que viene: se muestra junto con la escena cuando llega.
    pub diff: Option<DiffContext>,
}

/// Una carga que terminó bien, lista para mostrarse.
pub(crate) struct LoadedScene {
    pub scene: SceneHandle,
    pub backend: Arc<dyn ViewerBackend>,
    pub source: Arc<Vec<u8>>,
    pub path: String,
    pub kind: LoadKind,
    pub refit: bool,
    pub diff: Option<DiffContext>,
}

/// Una carga que terminó: la escena, o el error y el archivo que no abrió.
/// Una cancelada no se informa.
pub(crate) enum Finished {
    Loaded(LoadedScene),
    Failed { path: String, error: ViewerError },
}

pub(crate) struct Loader {
    /// Runtime Tokio compartido. Se queda vivo mientras la app vive.
    runtime: Arc<Runtime>,
    pending: Option<Promise<Result<LoadedScene, ViewerError>>>,
    token: Option<CancellationToken>,
    /// Archivo de la carga en vuelo (para nombrarlo si falla).
    path: Option<String>,
}

impl Loader {
    pub(crate) fn new() -> Self {
        // Runtime multi-hilo: spawn_blocking (parseo pesado) no bloquea al
        // scheduler principal. Dos workers son suficientes para una GUI.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");
        Self { runtime: Arc::new(runtime), pending: None, token: None, path: None }
    }

    /// Hay una carga en vuelo.
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Descarta la carga en vuelo: cuando llegue no debe pisar lo que se
    /// abrió después (una vista de ondas, un error, otra carga).
    pub(crate) fn cancel(&mut self) {
        if let Some(tok) = self.token.take() {
            tok.cancel();
        }
        self.pending = None;
        self.path = None;
    }

    /// Arranca una carga; la anterior, si había, se cancela.
    pub(crate) fn start(&mut self, req: LoadRequest) {
        self.cancel();
        let token = CancellationToken::new();
        self.token = Some(token.clone());
        self.path = Some(req.path.clone());
        let LoadRequest { backend, source, path, entry, kind, refit, diff } = req;

        let _guard = self.runtime.enter();
        let fut = async move {
            let hint = Some(path.clone());
            let result = match &kind {
                LoadKind::Single => backend.load_entry(source.as_ref().clone(), hint, entry, token).await,
                LoadKind::Diff { files, tab: DiffTab::After, .. } => {
                    backend.load_with(source.as_ref().clone(), hint, entry, files.after.clone(), token).await
                }
                LoadKind::Diff { before, files, tab: DiffTab::Before, .. } => {
                    backend.load_with(before.as_ref().clone(), hint, entry, files.before.clone(), token).await
                }
                LoadKind::Diff { before, files, tab: DiffTab::Diff, .. } => {
                    backend
                        .load_diff_with(before.as_ref().clone(), source.as_ref().clone(), hint, entry, files.clone(), token)
                        .await
                }
            };
            result.map(|scene| LoadedScene { scene, backend, source, path, kind, refit, diff })
        };
        self.pending = Some(Promise::spawn_async(fut));
    }

    /// Una carga aparte (un lado de la vista de LVS): no cancela ni la
    /// cancela la principal; quien la pide la consulta.
    #[cfg_attr(not(all(feature = "xschem", feature = "layout")), allow(dead_code))]
    pub(crate) fn load_detached(
        &self,
        backend: Arc<dyn ViewerBackend>,
        source: Arc<Vec<u8>>,
        path: String,
        entry: Option<String>,
    ) -> Promise<Result<LoadedScene, ViewerError>> {
        let _guard = self.runtime.enter();
        Promise::spawn_async(async move {
            let scene = backend.load_entry(source.as_ref().clone(), Some(path.clone()), entry, CancellationToken::new()).await?;
            Ok(LoadedScene { scene, backend, source, path, kind: LoadKind::Single, refit: true, diff: None })
        })
    }

    /// Lo que terminó desde el último cuadro, si algo terminó.
    pub(crate) fn poll(&mut self) -> Option<Finished> {
        self.pending.as_ref()?.ready()?;
        let result = self.pending.take()?.block_and_take();
        self.token = None;
        let path = self.path.take().unwrap_or_default();
        match result {
            Ok(loaded) => Some(Finished::Loaded(loaded)),
            // Una cancelada viene de nosotros mismos (se abrió otra cosa).
            Err(ViewerError::Cancelled) => None,
            Err(error) => Some(Finished::Failed { path, error }),
        }
    }
}
