//! Lo que se pide desde la pantalla de inicio y la barra: abrir otra
//! carpeta, los cambios sin commitear, comparar, el diagnóstico y exportar
//! la imagen de lo que se ve. Todo con el núcleo de la CLI (`status`,
//! `diff_pair`, `doctor`, `export`); las tareas largas, en un hilo.

use std::path::{Path, PathBuf};

use eframe::egui;
use poll_promise::Promise;

use super::{short_hash, RikuGuiApp, MAX_RECENT};
use crate::core::analysis::diff_pair::WORKTREE;
use crate::core::analysis::status::{self, StatusOptions, StatusReport};
use crate::gui::change_set::ChangeSet;
use crate::gui::content::{Content, LoadKind};
use crate::gui::dialogs::{CompareDialog, CompareOutcome, DoctorDialog};
use crate::gui::folder_picker::{FolderPicker, Picked};
use crate::gui::history::HistoryPanel;
use crate::gui::home::{self, HomeAction, HomeInput, StatusView};
use crate::gui::project::{is_openable, ProjectEntry};
use crate::gui::toast::ToastKind;
use crate::gui::tr;

/// Los cambios sin commitear del repo abierto.
pub(super) enum RepoStatus {
    /// Hay que calcularlo cuando se vea el inicio (no antes: con layouts
    /// grandes cuesta, y quien abre un archivo no lo necesita).
    Stale,
    NoRepo,
    Loading(Promise<Result<StatusReport, String>>),
    Ready(StatusReport),
    Failed(String),
}

impl RikuGuiApp {
    /// Vuelve a calcular los cambios sin commitear (lo de `riku status`), en
    /// un hilo: con layouts grandes tarda.
    pub(super) fn refresh_status(&mut self, ctx: &egui::Context) {
        let Some(repo) = self.history.repo().map(Path::to_path_buf) else {
            self.repo_status = RepoStatus::NoRepo;
            return;
        };
        let ctx = ctx.clone();
        let job = Promise::spawn_thread("riku-status", move || {
            let diff = crate::core::config::options_for(&repo, Default::default())?;
            let opts = StatusOptions { diff, ..Default::default() };
            let out = status::analyze_with_options_path(&repo, &opts, &crate::modules::registry()).map_err(|e| e.to_string());
            ctx.request_repaint();
            out
        });
        self.repo_status = RepoStatus::Loading(job);
    }

    /// Recibe lo que terminó en segundo plano (status, exportar).
    pub(super) fn poll_jobs(&mut self, ctx: &egui::Context) {
        if let RepoStatus::Loading(job) = &self.repo_status {
            if job.ready().is_some() {
                let RepoStatus::Loading(job) = std::mem::replace(&mut self.repo_status, RepoStatus::NoRepo) else {
                    unreachable!()
                };
                self.repo_status = match job.block_and_take() {
                    Ok(r) => RepoStatus::Ready(r),
                    Err(e) => RepoStatus::Failed(e),
                };
            }
        }
        if self.export_job.as_ref().is_some_and(|j| j.ready().is_some()) {
            match self.export_job.take().map(Promise::block_and_take) {
                Some(Ok(path)) => {
                    let shown = path.display().to_string();
                    ctx.copy_text(shown.clone());
                    self.notify(ToastKind::Success, tr!("toast.exported", path = shown));
                }
                Some(Err(e)) => self.fail(&tr!("error.export"), e),
                None => {}
            }
        }
    }

    /// Abre otra carpeta: árbol, repo e History nuevos, y vuelve al inicio.
    pub(super) fn open_folder(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if !path.is_dir() {
            self.fail(&tr!("error.open_folder"), path.display());
            return;
        }
        self.loader.cancel();
        self.project_root = path.clone();
        self.refresh_tree();
        let height = self.history.height;
        self.history = HistoryPanel::new(&path, height);
        self.go_home();
        let p = path.to_string_lossy().to_string();
        self.recent_dirs.retain(|d| *d != p);
        self.recent_dirs.insert(0, p);
        self.recent_dirs.truncate(MAX_RECENT);
        self.repo_status = RepoStatus::Stale;
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().to_string());
        self.status = tr!("status.folder", name = name);
    }

    /// Abre la lista de lo que cambió de `from` a `to` (diff de todo el repo).
    pub(super) fn open_change_set(&mut self, repo: &Path, from: &str, to: &str, ctx: &egui::Context) {
        use crate::core::domain::ports::RepoRoot;
        let root = crate::core::git::git_service::GitService::open(repo)
            .ok()
            .and_then(|s| s.root().map(Path::to_path_buf))
            .unwrap_or_else(|| repo.to_path_buf());
        self.change_set = Some(ChangeSet::start(root, from.to_string(), to.to_string(), ctx));
        self.status = tr!("status.change_set", a = short_hash(from), b = short_hash(to));
    }

    /// Un archivo de la lista: su diff entre las dos versiones de la lista.
    pub(super) fn open_change_set_file(&mut self, path: &str) {
        let Some(cs) = &self.change_set else { return };
        let (repo, a, b) = (cs.repo.clone(), cs.from.clone(), cs.to.clone());
        match self.load_backend_diff(&repo, &a, &b, Path::new(path), None, false) {
            Ok(()) => self.status = tr!("status.diff", from = short_hash(&a), to = short_hash(&b), path = path),
            Err(e) => self.fail(&tr!("error.diff"), e),
        }
    }

    /// Vuelve a la pantalla de inicio (cierra lo que se estaba viendo).
    pub(super) fn go_home(&mut self) {
        self.change_set = None;
        self.loader.cancel();
        self.content = Content::Home;
        self.diff = None;
        self.selected_path = None;
        self.error = None;
    }

    /// La pantalla de inicio y lo que se pidió en ella.
    pub(super) fn show_home(&mut self, ui: &mut egui::Ui) {
        if matches!(self.repo_status, RepoStatus::Stale) {
            self.refresh_status(&ui.ctx().clone());
        }
        let status = match &self.repo_status {
            RepoStatus::Stale | RepoStatus::NoRepo => StatusView::NoRepo,
            RepoStatus::Loading(_) => StatusView::Loading,
            RepoStatus::Ready(r) => StatusView::Ready(r),
            RepoStatus::Failed(e) => StatusView::Failed(e),
        };
        let input = HomeInput { root: &self.project_root, status, recent_files: &self.recent, recent_dirs: &self.recent_dirs };
        let Some(action) = home::show(ui, input) else { return };
        let ctx = ui.ctx().clone();
        match action {
            HomeAction::PickFolder => self.folder_picker = Some(FolderPicker::new(&self.project_root)),
            HomeAction::OpenFolder(p) => self.open_folder(&p),
            HomeAction::OpenFile(p) => self.open_path(&p),
            HomeAction::OpenChange(rel) => self.open_change(&rel),
            HomeAction::History => self.history.open = true,
            HomeAction::Compare => self.open_compare(),
            HomeAction::Doctor => self.doctor = Some(DoctorDialog::new(self.project_root.clone(), &ctx)),
            HomeAction::RefreshStatus => self.refresh_status(&ctx),
        }
    }

    /// Un cambio sin commitear: el archivo en `HEAD` contra el disco.
    fn open_change(&mut self, rel: &str) {
        self.change_set = None;
        let Some(repo) = self.history.repo().map(Path::to_path_buf) else { return };
        match self.load_backend_diff(&repo, "HEAD", WORKTREE, Path::new(rel), None, false) {
            Ok(()) => self.status = tr!("status.diff", from = "HEAD", to = "worktree", path = rel),
            Err(e) => self.fail(&tr!("error.diff"), e),
        }
    }

    /// "Comparar versiones…": el archivo abierto (si es del repo) ya elegido.
    pub(super) fn open_compare(&mut self) {
        let Some(repo) = self.history.repo().map(Path::to_path_buf) else { return };
        let mut files = Vec::new();
        collect_files(&self.project_tree, &repo, &self.openable, &mut files);
        let current = self.selected_path.as_ref().and_then(|p| {
            let rel = if p.is_relative() { p.clone() } else { p.strip_prefix(&repo).ok()?.to_path_buf() };
            Some(rel.to_string_lossy().replace('\\', "/"))
        });
        let refs = repo_refs(&repo);
        self.compare = Some(CompareDialog::new(files, refs, current));
    }

    /// Ventanas abiertas encima de todo (selector de carpeta, comparar,
    /// diagnóstico).
    pub(super) fn show_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(picker) = &mut self.folder_picker {
            match picker.show(ctx) {
                Some(Picked::Folder(dir)) => {
                    self.folder_picker = None;
                    self.open_folder(&dir);
                }
                Some(Picked::Cancel) => self.folder_picker = None,
                None => {}
            }
        }
        if let Some(dialog) = &mut self.compare {
            match dialog.show(ctx) {
                Some(CompareOutcome::Go { file, a, b }) => {
                    self.compare = None;
                    let Some(repo) = self.history.repo().map(Path::to_path_buf) else { return };
                    match file {
                        // Todos los archivos que cambiaron: la lista al costado.
                        None => self.open_change_set(&repo, &a, &b, ctx),
                        Some(file) => {
                            self.change_set = None;
                            match self.load_backend_diff(&repo, &a, &b, Path::new(&file), None, false) {
                                Ok(()) => {
                                    self.status = tr!("status.diff", from = short_hash(&a), to = short_hash(&b), path = file)
                                }
                                Err(e) => self.fail(&tr!("error.diff"), e),
                            }
                        }
                    }
                }
                Some(CompareOutcome::Cancel) => self.compare = None,
                None => {}
            }
        }
        if let Some(d) = &mut self.doctor {
            if !d.show(ctx) {
                self.doctor = None;
            }
        }
    }

    /// Se puede exportar lo que se ve: una escena, o una simulación suelta
    /// (la comparación de ondas no guarda los bytes de las dos versiones).
    pub(super) fn can_export(&self) -> bool {
        #[cfg(feature = "spice")]
        if let Some(w) = self.content.wave() {
            return !w.is_diff() && w.path.is_some();
        }
        self.content.scene().is_some()
    }

    /// Guarda la imagen de lo que se ve (lo de `riku render` y `riku diff
    /// -f png`), en un hilo. La ruta queda en el portapapeles.
    pub(super) fn export_image(&mut self, png: bool, ctx: &egui::Context) {
        let size = self.canvas_rect.map_or(egui::vec2(1600.0, 1000.0), |r| r.size() * ctx.pixels_per_point());
        let mut req = crate::export::Request {
            png,
            output: None,
            width: (size.x.round() as u32).clamp(800, 16384),
            height: (size.y.round() as u32).clamp(600, 16384),
            dark: ctx.theme() == egui::Theme::Dark,
            cell: None,
        };
        let worktree = |path: &Path| {
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
            riku_kernel::DiffFiles::new(None, crate::core::git::files::workdir_files(Some(&dir)))
        };
        let (path, before, after, files, label) = if let Some(bs) = self.content.scene() {
            req.cell = bs.scene.current_entry().map(str::to_string);
            match &bs.kind {
                LoadKind::Single => {
                    let files = worktree(Path::new(&bs.path));
                    (bs.path.clone(), None, bs.source.as_ref().clone(), files, "worktree".to_string())
                }
                LoadKind::Diff { before, files, .. } => {
                    let label = self
                        .diff
                        .as_ref()
                        .map_or_else(String::new, |d| format!("{} → {}", short_hash(&d.commit_a), short_hash(&d.commit_b)));
                    (bs.path.clone(), Some(before.as_ref().clone()), bs.source.as_ref().clone(), files.clone(), label)
                }
            }
        } else {
            let Some(path) = self.wave_path() else { return };
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) => return self.fail(&tr!("error.read", file = path.display()), e),
            };
            let files = worktree(&path);
            (path.to_string_lossy().to_string(), None, bytes, files, "worktree".to_string())
        };
        let opts = crate::core::config::options_for(&self.project_root, Default::default()).unwrap_or_default();
        let ctx = ctx.clone();
        self.export_job = Some(Promise::spawn_thread("riku-export", move || {
            let out = crate::export::image(&crate::modules::registry(), &path, before, after, files, &label, &req, &opts);
            ctx.request_repaint();
            out
        }));
        self.notify(ToastKind::Info, tr!("toast.exporting"));
    }
}

impl RikuGuiApp {
    /// El `.raw` de la vista de ondas, si es una sola simulación.
    #[cfg(feature = "spice")]
    fn wave_path(&self) -> Option<PathBuf> {
        self.content.wave().filter(|w| !w.is_diff()).and_then(|w| w.path.clone())
    }

    #[cfg(not(feature = "spice"))]
    fn wave_path(&self) -> Option<PathBuf> {
        None
    }
}

/// Archivos que se pueden abrir, relativos a la raíz del repo.
fn collect_files(entry: &ProjectEntry, repo: &Path, openable: &[String], out: &mut Vec<String>) {
    match entry {
        ProjectEntry::Directory { children, .. } => {
            for c in children {
                collect_files(c, repo, openable, out);
            }
        }
        ProjectEntry::File { path, .. } if is_openable(path, openable) => {
            if let Ok(rel) = path.strip_prefix(repo) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        ProjectEntry::File { .. } => {}
    }
}

/// Ramas y tags del repo (sin `HEAD`), ordenados.
fn repo_refs(repo: &Path) -> Vec<String> {
    use crate::core::domain::ports::GitRepository;
    let Ok(svc) = crate::core::git::git_service::GitService::open(repo) else { return Vec::new() };
    let mut refs: Vec<String> = svc.refs_by_oid().unwrap_or_default().into_values().flatten().filter(|r| r != "HEAD").collect();
    refs.sort();
    refs.dedup();
    refs
}
