//! Abrir cosas: un archivo, un diff entre versiones, un `.raw`; recargar y
//! recibir lo que terminó de cargar. Lo que se ve y su contexto de diff se
//! fijan juntos.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{friendly_error, short_hash, RikuGuiApp};
use crate::gui::content::{Content, DiffContext, DiffTab, LoadKind, SceneState};
use crate::gui::history::model::Request as HistoryRequest;
use crate::gui::loader::{Finished, LoadRequest};
use crate::gui::toast::ToastKind;
use crate::gui::tr;
#[cfg(feature = "spice")]
use crate::gui::wave_view::{self, WaveView};

impl RikuGuiApp {
    pub(super) fn open_path(&mut self, path: &Path) {
        self.loader.cancel();
        self.selected_path = Some(path.to_path_buf());
        self.error = None;
        // Un archivo suelto no es parte de un diff.
        self.content = Content::Home;
        self.diff = None;
        self.remember_recent(path);
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

        // Cualquier formato con un backend registrado (ruta neutra async).
        if self.load_via_backend(path, None) {
            self.status = tr!("status.loading", file = path.display());
        } else if self.error.is_none() {
            self.status = tr!("status.unsupported", file = path.display());
            self.notify(ToastKind::Warning, tr!("toast.unsupported", name = name, exts = self.openable_text()));
        }
    }

    /// Intenta cargar `path` via alguno de los backends registrados; `entry`
    /// elige una sub-vista (celda GDS) o `None` para la de por defecto.
    /// Retorna `true` si algún backend aceptó el archivo (la carga queda en vuelo).
    pub(super) fn load_via_backend(&mut self, path: &Path, entry: Option<String>) -> bool {
        #[cfg(feature = "spice")]
        {
            if wave_view::is_raw(path) {
                return self.open_raw(path);
            }
            if self.content.wave().is_some() {
                self.content = Content::Home;
            }
        }
        let content = match std::fs::read(path) {
            Ok(c) => c,
            Err(e) => {
                self.fail(&tr!("error.read", file = path.display()), e);
                return false;
            }
        };
        let path_str = path.to_string_lossy().to_string();

        let backend = self.backends.iter()
            .find(|b| b.accepts(&content, Some(&path_str)))
            .cloned();
        let Some(backend) = backend else { return false };

        self.loader.start(LoadRequest {
            backend,
            source: Arc::new(content),
            path: path_str,
            entry,
            kind: LoadKind::Single,
            refit: true,
            diff: None,
        });
        true
    }

    /// Diff de un archivo entre dos commits via el backend de su formato. El
    /// archivo puede no existir en el commit "antes" (archivo nuevo); un
    /// commit que no existe es un error, no "sin cambios". El contexto del
    /// diff se muestra cuando llega la escena.
    pub(super) fn load_backend_diff(
        &mut self,
        repo: &Path,
        commit_a: &str,
        commit_b: &str,
        file: &Path,
        entry: Option<String>,
        from_history: bool,
    ) -> Result<(), String> {
        use crate::core::analysis::blob_io::Blob;
        use crate::core::analysis::diff_pair::{self, End, OnError, Version};
        use crate::core::domain::ports::{GitRepository, RepoRoot};
        use crate::core::git::git_service::GitService;

        self.loader.cancel();
        let svc = GitService::open(repo).map_err(|e| e.to_string())?;
        let file_str = file.to_string_lossy().to_string();
        // Si el archivo se renombró entre A y B, en A tiene la ruta vieja.
        let worktree = diff_pair::WORKTREE;
        let old_path = (!commit_a.is_empty() && commit_b != worktree)
            .then(|| svc.get_changed_files(commit_a, commit_b).ok())
            .flatten()
            .and_then(|list| list.into_iter().find(|c| c.path == file_str))
            .and_then(|c| c.old_path);
        // Si falta de un lado se compara contra vacío: todo cuenta como
        // añadido (archivo nuevo, o `commit_a` vacío: el commit inicial) o
        // eliminado (borrado en `commit_b`). Con `:worktree` (`riku diff A
        // archivo -f visual`), B es el disco. La misma lectura que la CLI.
        let workdir = svc.root().map(Path::to_path_buf);
        let read = |token: &str, path: &str| -> Result<Vec<u8>, String> {
            let end = End::new(Version::from_token(token), path);
            match diff_pair::read(&svc, workdir.as_deref(), end, OnError::Propagate) {
                Ok(Blob::Bytes(b)) => Ok(b),
                Ok(Blob::Missing) => Ok(Vec::new()),
                Ok(Blob::Skipped(why)) => Err(why),
                Err(e) => Err(format!("{token}: {e}")),
            }
        };
        let after = read(commit_b, &file_str)?;
        let before = read(commit_a, old_path.as_deref().unwrap_or(&file_str))?;
        let diff = DiffContext {
            commit_a: commit_a.to_string(),
            commit_b: commit_b.to_string(),
            file: file.to_path_buf(),
            from_history,
        };

        #[cfg(feature = "spice")]
        if wave_view::is_raw(file) {
            let view = WaveView::compare_bytes(
                &before,
                &after,
                (commit_a, short_hash(commit_a)),
                (commit_b, short_hash(commit_b)),
                &self.wave_exprs,
            )?;
            self.status = view.summary();
            self.selected_path = Some(file.to_path_buf());
            self.content = Content::Wave(view);
            self.diff = Some(diff);
            return Ok(());
        }

        let backend = self.backends.iter()
            .find(|b| b.accepts(&after, Some(&file_str)))
            .cloned()
            .ok_or_else(|| tr!("error.no_viewer", file = file_str))?;

        // Una vista de ondas abierta taparía el diff pedido.
        #[cfg(feature = "spice")]
        if self.content.wave().is_some() {
            self.content = Content::Home;
        }
        self.selected_path = Some(file.to_path_buf());
        // Otros archivos de cada commit, para los formatos que los necesitan.
        // Contra el disco, las sub-celdas también salen del disco.
        let files = diff_pair::sources(&svc, workdir.as_deref(), Version::from_token(commit_a), Version::from_token(commit_b));
        self.loader.start(LoadRequest {
            backend,
            source: Arc::new(after),
            path: file_str,
            entry,
            kind: LoadKind::Diff { before: Arc::new(before), files, tab: DiffTab::Diff },
            refit: true,
            diff: Some(diff),
        });
        Ok(())
    }

    /// Otra carga del archivo de la escena actual (otra celda u otra pestaña
    /// del diff), sin releer el disco. La escena actual sigue visible hasta
    /// que llega la nueva.
    pub(super) fn reload_scene(&mut self, entry: Option<String>, kind: Option<LoadKind>, refit: bool) {
        let Some(bs) = self.content.scene() else { return };
        let req = LoadRequest {
            backend: bs.backend.clone(),
            source: bs.source.clone(),
            path: bs.path.clone(),
            entry,
            kind: kind.unwrap_or_else(|| bs.kind.clone()),
            refit,
            diff: self.diff.clone(),
        };
        self.error = None;
        self.loader.start(req);
    }

    /// Carga otra celda del archivo abierto.
    pub(super) fn select_entry(&mut self, id: &str) {
        self.status = tr!("status.loading_cell", cell = id);
        self.reload_scene(Some(id.to_string()), None, true);
    }

    /// Cambia la pestaña del diff (Diff / Before / After) sobre la misma
    /// celda, conservando la vista para comparar la misma zona.
    pub(super) fn select_diff_tab(&mut self, tab: DiffTab) {
        let Some(bs) = self.content.scene() else { return };
        let LoadKind::Diff { before, files, .. } = &bs.kind else { return };
        let kind = LoadKind::Diff { before: before.clone(), files: files.clone(), tab };
        let entry = bs.scene.current_entry().map(str::to_string);
        self.reload_scene(entry, Some(kind), false);
    }

    /// Abre en el lienzo lo que pidió el panel History.
    pub(super) fn handle_history_request(&mut self, req: HistoryRequest) {
        let HistoryRequest::OpenDiff { parent, commit, path } = req;
        let Some(repo) = self.history.repo().map(Path::to_path_buf) else { return };
        let parent = parent.unwrap_or_default();
        let file = PathBuf::from(&path);
        self.selected_path = Some(repo.join(&path));
        match self.load_backend_diff(&repo, &parent, &commit, &file, None, true) {
            Ok(()) => self.status = format!("Diff {} → {} · {path}", short_hash(&parent), short_hash(&commit)),
            Err(e) => self.fail(&tr!("error.diff"), e),
        }
    }

    /// Abre un `.raw` en la vista de formas de onda. `true` si se pudo.
    #[cfg(feature = "spice")]
    pub(super) fn open_raw(&mut self, path: &Path) -> bool {
        self.loader.cancel();
        match wave_view::read_raw(path) {
            Ok(file) => {
                let view = WaveView::single(file, path.to_path_buf(), &self.wave_exprs);
                self.status = view.summary();
                self.content = Content::Wave(view);
                self.diff = None;
                true
            }
            Err(e) => {
                self.fail(&tr!("error.read", file = path.display()), e);
                false
            }
        }
    }

    /// Atiende lo que pidió la vista de formas de onda (comparar con otro archivo).
    #[cfg(feature = "spice")]
    pub(super) fn handle_wave_request(&mut self) {
        let Some(view) = self.content.wave_mut() else { return };
        let Some(req) = view.request.take() else { return };
        let Some(path) = view.path.clone() else { return };
        match req {
            wave_view::Request::CompareWith(other) => match (wave_view::read_raw(&other), wave_view::read_raw(&path)) {
                (Ok(a), Ok(b)) => {
                    let label = |p: &Path| p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let view = WaveView::compare(a, b, label(&other), label(&path), Some(path.clone()), &self.wave_exprs);
                    self.status = view.summary();
                    self.content = Content::Wave(view);
                }
                (Err(e), _) | (_, Err(e)) => self.fail(&tr!("error.compare"), e),
            },
            wave_view::Request::StopComparing => {
                self.open_raw(&path);
            }
        }
    }

    /// Relee el archivo del disco conservando la sub-vista actual.
    pub(super) fn reload_backend(&mut self) {
        #[cfg(feature = "spice")]
        if let Some(path) = self.content.wave().filter(|w| !w.is_diff()).and_then(|w| w.path.clone()) {
            self.open_raw(&path);
            return;
        }
        let Some(bs) = self.content.scene() else { return };
        // En diff los bytes vienen de git, no del disco.
        if matches!(bs.kind, LoadKind::Diff { .. }) {
            return;
        }
        let path = PathBuf::from(&bs.path);
        let entry = bs.scene.current_entry().map(str::to_string);
        self.error = None;
        if self.load_via_backend(&path, entry) {
            self.status = tr!("status.reloading", file = path.display());
        }
    }

    /// Lo que terminó de cargar desde el último cuadro: se muestra junto con
    /// su contexto de diff. Se llama desde `ui()`.
    pub(super) fn poll_pending_load(&mut self) {
        match self.loader.poll() {
            Some(Finished::Loaded(loaded)) => {
                let name = loaded.backend.info().name;
                let what = match &loaded.kind {
                    LoadKind::Single => tr!("status.opened", name = name),
                    LoadKind::Diff { tab, .. } => tab.label().to_string(),
                };
                self.status = match loaded.scene.current_entry() {
                    Some(entry) => format!("{what} · {entry}"),
                    None => what,
                };
                // Confirmar solo lo que el usuario pidió cargar (archivo o
                // celda nuevos), no cada cambio de pestaña del diff.
                if loaded.refit {
                    let shown = loaded.scene.current_entry().map(str::to_string).unwrap_or_else(|| {
                        Path::new(&loaded.path).file_name().unwrap_or_default().to_string_lossy().to_string()
                    });
                    self.notify(ToastKind::Success, tr!("toast.loaded", what = shown));
                }
                let diff = loaded.diff.clone();
                let prev = self.content.take_scene();
                self.content = Content::Scene(SceneState::from_loaded(loaded, prev));
                self.diff = diff;
            }
            Some(Finished::Failed { path, error }) => {
                let name = Path::new(&path).file_name().unwrap_or_default().to_string_lossy().to_string();
                // Un archivo que no abre no debe ofrecerse como reciente.
                self.recent.retain(|r| *r != path);
                self.fail(&tr!("error.open", file = name), friendly_error(&error));
            }
            None => {}
        }
    }
}
