use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, RichText};
use poll_promise::Promise;
use tokio::runtime::Runtime;
use viewer_core::{
    backend::ViewerBackend, bbox::BoundingBox, diff::ChangeKind, element::Layer, scene::SceneHandle,
    viewport::Viewport as VcViewport,
    CancellationToken,
};

use crate::gui::entry_picker;
use crate::gui::history::{self, model::Request as HistoryRequest, HistoryPanel};
use crate::gui::launch::LaunchArgs;
use crate::gui::motion::{theme_fade_alpha, Inertia, ViewAnimation};
use crate::gui::project::ProjectEntry;
use crate::gui::scene_painter::{
    fit_bbox, fit_scene, focus_area, hover_info, paint_scene, to_color32, zoom_at_screen, PaintOptions, ScreenXform,
};
use crate::gui::theme::{space, CanvasTheme};
use crate::gui::toast::{ToastKind, Toasts};
use crate::gui::{i18n, tr};
#[cfg(feature = "spice")]
use crate::gui::wave_view::{self, WaveView};

// ─── Estado del schematic ─────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffTab {
    Before,
    After,
    Diff,
}

// ─── App ──────────────────────────────────────────────────────────────────────

/// Contexto persistente de un diff cargado — permite recargarlo sin perder estado.
struct DiffContext {
    /// Vacío: el commit inicial, que se compara contra nada.
    commit_a: String,
    commit_b: String,
    file: PathBuf,
    /// Se abrió desde el panel History (la ruta sobre el lienzo lo dice).
    from_history: bool,
}

/// Estado de una carga via `ViewerBackend`: todos los formatos (esquemáticos,
/// layouts) se muestran por esta ruta.
struct BackendState {
    scene: SceneHandle,
    viewport: VcViewport,
    /// Backend que produjo la escena y bytes/ruta de origen: permiten cargar
    /// otra sub-vista (celda) sin volver a leer el disco.
    backend: Arc<dyn ViewerBackend>,
    source: Arc<Vec<u8>>,
    path: String,
    /// Encuadrar la escena en el próximo frame (al cargar o con "Fit"). El
    /// fit necesita el tamaño real del panel, que solo se conoce al pintar.
    needs_fit: bool,
    /// Tamaño del lienzo en el último encuadre automático; `None` si el
    /// usuario movió la vista a mano (entonces no se re-encuadra solo).
    fitted_size: Option<egui::Vec2>,
    /// Capas ocultas desde el panel de detalles, por **nombre** (`"met1 68/20"`):
    /// las claves numéricas cambian entre celdas, el nombre no.
    hidden_layers: HashSet<String>,
    /// Buscador y filtro del selector de celdas.
    entry_query: String,
    only_roots: bool,
    /// En diff: listar solo celdas con cambios.
    only_changed: bool,
    /// Qué se carga: un archivo suelto o un diff entre dos versiones.
    kind: LoadKind,
    /// Zona a encuadrar en el próximo frame (clic en un cambio).
    focus: Option<BoundingBox>,
    /// El próximo encuadre lo pidió el usuario (Encuadrar / F): se anima.
    /// Los automáticos (al cargar, al redimensionar) son inmediatos.
    animate_fit: bool,
    /// Zoom pedido por teclado (+/−), aplicado sobre el centro del lienzo.
    pending_zoom: Option<f64>,
    /// Transición de vista en curso (spring interrumpible).
    anim: Option<ViewAnimation>,
    /// Inercia del pan tras soltar un arrastre rápido.
    inertia: Option<Inertia>,
}

/// Qué produce una carga via backend. En modo diff, `source` (en
/// `BackendState`) es la versión "después" y `before` la "antes".
#[derive(Clone)]
enum LoadKind {
    Single,
    Diff { before: Arc<Vec<u8>>, tab: DiffTab },
}

impl BackendState {
    /// Claves de capa de la escena actual que están ocultas.
    fn hidden_keys(&self) -> HashSet<Layer> {
        self.scene
            .layer_list()
            .into_iter()
            .filter(|(_, p)| self.hidden_layers.contains(&p.name))
            .map(|(k, _)| k)
            .collect()
    }
}

/// Resultado de una carga async via backend, antes de fusionarse con el estado.
struct LoadedScene {
    scene: SceneHandle,
    backend: Arc<dyn ViewerBackend>,
    source: Arc<Vec<u8>>,
    path: String,
    kind: LoadKind,
    /// Re-encuadrar al llegar (nueva celda). Al cambiar de pestaña del diff
    /// se conserva la vista para comparar la misma zona.
    refit: bool,
}

pub struct RikuGuiApp {
    project_root: PathBuf,
    project_tree: ProjectEntry,
    selected_path: Option<PathBuf>,
    diff_ctx: Option<DiffContext>,
    status: String,
    error: Option<String>,

    // ─── Nivel 2: ruta async via ViewerBackend ──────────────────────────────
    /// Runtime Tokio compartido. Se queda vivo mientras la app vive.
    runtime: Arc<Runtime>,
    /// Backends registrados. El primero que responda `accepts()` gana.
    backends: Vec<Arc<dyn ViewerBackend>>,
    /// Extensiones que saben abrir los backends (filtro del árbol).
    openable: Vec<String>,
    /// Escena actual cargada via backend (path neutro).
    backend_state: Option<BackendState>,
    /// Carga async en vuelo (solo una — al llegar una nueva se cancela).
    pending_load: Option<Promise<Result<LoadedScene, String>>>,
    /// Token de cancelación de la carga en vuelo.
    pending_token: Option<CancellationToken>,
    /// Formas de onda (`.raw`): no pasan por `ViewerBackend`, tienen su vista.
    #[cfg(feature = "spice")]
    wave: Option<WaveView>,
    /// Expresiones de la vista de formas de onda (`--expr` o las de la sesión
    /// anterior); se aplican a cada `.raw` que se abre.
    #[cfg(feature = "spice")]
    wave_exprs: Vec<String>,

    // ─── Preferencias (persisten entre sesiones) ────────────────────────────
    /// Dibujar etiquetas de texto en el lienzo.
    show_labels: bool,
    /// Árbol de proyecto con todos los archivos, no solo los que se abren.
    show_all_files: bool,
    /// Sin animaciones ni inercia (accesibilidad: movimiento reducido).
    reduce_motion: bool,
    /// Resumir en bloques lo menor a un píxel al alejarse (layouts grandes).
    simplify: bool,
    /// Lado máximo de un bloque de nivel de detalle, en píxeles
    /// (`RIKU_LOD_PX` para ajustarlo; por defecto `viewer_core::index::BLOCK_PX`).
    block_px: f64,
    /// `RIKU_PROFILE`: imprimir tiempos de pintado por cuadro.
    profile: bool,

    // ─── Lectura del lienzo para la barra de estado (frame anterior) ────────
    /// Posición del cursor en coordenadas de mundo, si está sobre el lienzo.
    cursor_world: Option<(f64, f64)>,
    /// Tamaño de un píxel en unidades de mundo (escala actual).
    px_world: Option<f64>,
    /// Etiquetas omitidas por solaparse en el último frame.
    labels_hidden: usize,
    /// Título aplicado a la ventana (para enviarlo solo cuando cambia).
    window_title: String,
    /// Mensajes temporales (feedback de estado, completado, aviso, error).
    toasts: Toasts,
    /// Reloj de egui del frame actual (para fechar los mensajes).
    now: f64,
    /// Archivos abiertos recientemente (persistente), el último primero.
    recent: Vec<String>,
    /// Área del lienzo en el último frame (los mensajes se anclan ahí).
    canvas_rect: Option<egui::Rect>,
    /// Archivo de la carga en vuelo (para nombrarlo si falla).
    loading_path: Option<String>,
    /// Tema del frame anterior (para detectar el cambio y fundirlo).
    last_dark: Option<bool>,
    /// Fundido en curso al cambiar de tema: (inicio, fondo del tema anterior).
    theme_fade: Option<(f64, egui::Color32)>,
    /// Panel History: el historial con su grafo (abajo, H).
    history: HistoryPanel,
}

/// Claves de persistencia (eframe storage).
const PREF_LABELS: &str = "riku.show_labels";
const PREF_ALL_FILES: &str = "riku.show_all_files";
const PREF_REDUCE_MOTION: &str = "riku.reduce_motion";
const PREF_SIMPLIFY: &str = "riku.simplify";
const PREF_RECENT: &str = "riku.recent_files";
const PREF_LANG: &str = "riku.lang";
const PREF_HISTORY_H: &str = "riku.history_height";
#[cfg(feature = "spice")]
const PREF_WAVE_EXPRS: &str = "riku.wave_exprs";
/// Cuántos archivos recientes se recuerdan.
const MAX_RECENT: usize = 6;

impl RikuGuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: LaunchArgs) -> Self {
        // Load a system font explicitly — egui's embedded font sometimes fails with glow backend
        let mut fonts = egui::FontDefinitions::default();
        for path in [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert(
                    "system".to_owned(),
                    egui::FontData::from_owned(bytes).into(),
                );
                fonts.families.entry(egui::FontFamily::Proportional)
                    .or_default().insert(0, "system".to_owned());
                fonts.families.entry(egui::FontFamily::Monospace)
                    .or_default().insert(0, "system".to_owned());
                break;
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        crate::gui::theme::install_style(&cc.egui_ctx);
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        // Ruta absoluta: con `riku-gui archivo.gds` el parent de una ruta
        // relativa es "" y el árbol de proyecto quedaba vacío.
        let launch_path = launch.file.as_deref().map(|p| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()));
        let (project_root, selected_path): (PathBuf, Option<PathBuf>) = match launch_path {
            Some(path) if path.is_file() => {
                let root = path.parent().map(Path::to_path_buf).unwrap_or_else(|| cwd.clone());
                (root, Some(path))
            }
            Some(path) if path.is_dir() => (path, None),
            _ => (cwd.clone(), None),
        };

        let pref = |key: &str, default: bool| {
            cc.storage.and_then(|s| eframe::get_value::<bool>(s, key)).unwrap_or(default)
        };
        let show_labels = pref(PREF_LABELS, true);
        let show_all_files = pref(PREF_ALL_FILES, false);
        let reduce_motion = pref(PREF_REDUCE_MOTION, false);
        let simplify = pref(PREF_SIMPLIFY, true);
        let recent: Vec<String> = cc.storage.and_then(|s| eframe::get_value(s, PREF_RECENT)).unwrap_or_default();
        let saved_lang: Option<String> = cc.storage.and_then(|s| eframe::get_value(s, PREF_LANG));
        i18n::set(i18n::initial(saved_lang.as_deref()));

        // Runtime multi-hilo: spawn_blocking (parseo pesado) no bloquea al
        // scheduler principal. Dos workers son suficientes para una GUI.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");
        let runtime = Arc::new(runtime);

        // Los módulos del ejecutable deciden qué formatos se pueden abrir.
        let modules = crate::modules::registry();
        let backends: Vec<Arc<dyn ViewerBackend>> = modules.viewers();
        #[allow(unused_mut)]
        let mut openable: Vec<String> =
            backends.iter().flat_map(|b| b.info().extensions.iter().map(|e| e.to_string())).collect();
        #[cfg(feature = "spice")]
        openable.push("raw".to_string());
        let project_tree = ProjectEntry::build(&project_root, show_all_files, &openable);
        let history_h: f32 = cc.storage.and_then(|s| eframe::get_value(s, PREF_HISTORY_H)).unwrap_or(history::DEFAULT_HEIGHT);
        let history = HistoryPanel::new(&project_root, history_h);

        let mut app = Self {
            project_root,
            project_tree,
            selected_path,
            diff_ctx: None,
            status: tr!("status.ready"),
            error: None,
            runtime,
            backends,
            openable,
            backend_state: None,
            pending_load: None,
            pending_token: None,
            #[cfg(feature = "spice")]
            wave: None,
            #[cfg(feature = "spice")]
            wave_exprs: if launch.exprs.is_empty() {
                cc.storage.and_then(|s| eframe::get_value(s, PREF_WAVE_EXPRS)).unwrap_or_default()
            } else {
                launch.exprs.clone()
            },
            show_labels,
            show_all_files,
            reduce_motion,
            simplify,
            profile: std::env::var_os("RIKU_PROFILE").is_some(),
            block_px: std::env::var("RIKU_LOD_PX")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|v: &f64| *v > 0.0)
                .unwrap_or(viewer_core::index::BLOCK_PX),
            cursor_world: None,
            px_world: None,
            labels_hidden: 0,
            window_title: String::new(),
            toasts: Toasts::default(),
            now: 0.0,
            recent,
            canvas_rect: None,
            loading_path: None,
            last_dark: None,
            theme_fade: None,
            history,
        };

        // Modo diff: commits pasados desde el CLI
        if let (Some(file), Some(ca), Some(cb)) = (&launch.file, &launch.commit_a, &launch.commit_b) {
            let repo = launch.repo.as_deref().unwrap_or(Path::new("."));
            app.diff_ctx = Some(DiffContext {
                commit_a: ca.clone(),
                commit_b: cb.clone(),
                file: file.clone(),
                from_history: false,
            });
            match app.load_backend_diff(repo, ca, cb, file, launch.cell.clone()) {
                Ok(()) => app.status = format!("Diff {} → {}", ca, cb),
                Err(e) => app.fail(&tr!("error.diff"), e),
            }
        } else if let Some(path) = app.selected_path.clone() {
            app.remember_recent(&path);
            if app.load_via_backend(&path, launch.cell.clone()) {
                app.status = tr!("status.loading", file = path.display());
            } else {
                app.status = tr!("status.unsupported", file = path.display());
            }
        }

        app
    }

    fn refresh_tree(&mut self) {
        self.project_tree = ProjectEntry::build(&self.project_root, self.show_all_files, &self.openable);
    }

    /// Feedback breve (estado, completado, aviso) en un mensaje temporal.
    fn notify(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(kind, text, self.now);
    }

    /// Error: queda en la barra de estado y en un mensaje que no se va solo.
    fn fail(&mut self, what: &str, e: impl std::fmt::Display) {
        let msg = format!("{what}: {e}");
        self.status = tr!("status.error", what = what);
        self.error = Some(msg.clone());
        self.toasts.push(ToastKind::Error, msg, self.now);
    }

    /// Recuerda un archivo abierto (el más reciente primero, sin repetir).
    fn remember_recent(&mut self, path: &Path) {
        let p = path.to_string_lossy().to_string();
        self.recent.retain(|r| *r != p);
        self.recent.insert(0, p);
        self.recent.truncate(MAX_RECENT);
    }

    fn open_path(&mut self, path: &Path) {
        self.selected_path = Some(path.to_path_buf());
        self.error = None;
        self.backend_state = None;
        self.remember_recent(path);
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

        // Cualquier formato con un backend registrado (ruta neutra async).
        if self.load_via_backend(path, None) {
            self.status = tr!("status.loading", file = path.display());
        } else if self.error.is_none() {
            self.status = tr!("status.unsupported", file = path.display());
            self.notify(ToastKind::Warning, tr!("toast.unsupported", name = name));
        }
    }

    /// Intenta cargar `path` via alguno de los backends registrados; `entry`
    /// elige una sub-vista (celda GDS) o `None` para la de por defecto.
    /// Retorna `true` si algún backend aceptó el archivo (la carga queda en vuelo).
    fn load_via_backend(&mut self, path: &Path, entry: Option<String>) -> bool {
        #[cfg(feature = "spice")]
        {
            if is_raw(path) {
                return self.open_raw(path);
            }
            self.wave = None;
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

        self.spawn_backend_load(backend, Arc::new(content), path_str, entry, LoadKind::Single, true);
        true
    }

    /// Diff de un archivo entre dos commits via el backend de su formato. El
    /// archivo puede no existir en el commit "antes" (archivo nuevo); un
    /// commit que no existe es un error, no "sin cambios".
    fn load_backend_diff(&mut self, repo: &Path, commit_a: &str, commit_b: &str, file: &Path, entry: Option<String>) -> Result<(), String> {
        use crate::core::domain::ports::GitRepository;
        use crate::core::git::git_service::GitService;

        let svc = GitService::open(repo).map_err(|e| e.to_string())?;
        let file_str = file.to_string_lossy().to_string();
        // Si falta de un lado se compara contra vacío: todo cuenta como
        // añadido (archivo nuevo, o `commit_a` vacío: el commit inicial) o
        // eliminado (borrado en `commit_b`).
        let read = |commit: &str| -> Result<Vec<u8>, String> {
            if commit.is_empty() {
                return Ok(Vec::new());
            }
            match svc.get_blob(commit, &file_str) {
                Ok(bytes) => Ok(bytes),
                Err(crate::core::domain::git_types::GitError::BlobNotFound { .. }) => Ok(Vec::new()),
                Err(e) => Err(format!("{commit}: {e}")),
            }
        };
        let after = read(commit_b)?;
        let before = read(commit_a)?;

        #[cfg(feature = "spice")]
        if is_raw(file) {
            use crate::modules::spice::raw;
            let parse = |bytes: &[u8], commit: &str| -> Result<raw::RawFile, String> {
                if bytes.is_empty() {
                    return Ok(raw::RawFile { plots: Vec::new() });
                }
                raw::parse(bytes).map_err(|e| format!("{commit}: {e}"))
            };
            let view = WaveView::compare(
                parse(&before, commit_a)?,
                parse(&after, commit_b)?,
                short_hash(commit_a),
                short_hash(commit_b),
                None,
                &self.wave_exprs,
            );
            self.status = view.summary();
            self.selected_path = Some(file.to_path_buf());
            self.backend_state = None;
            self.wave = Some(view);
            return Ok(());
        }

        let backend = self.backends.iter()
            .find(|b| b.accepts(&after, Some(&file_str)))
            .cloned()
            .ok_or_else(|| tr!("error.no_viewer", file = file_str))?;

        self.selected_path = Some(file.to_path_buf());
        let kind = LoadKind::Diff { before: Arc::new(before), tab: DiffTab::Diff };
        self.spawn_backend_load(backend, Arc::new(after), file_str, entry, kind, true);
        Ok(())
    }

    /// Carga otra sub-vista del archivo ya abierto (sin releer el disco). La
    /// escena actual sigue visible hasta que llega la nueva.
    fn select_entry(&mut self, id: &str) {
        let Some(bs) = &self.backend_state else { return };
        let (backend, source, path, kind) = (bs.backend.clone(), bs.source.clone(), bs.path.clone(), bs.kind.clone());
        self.error = None;
        self.status = tr!("status.loading_cell", cell = id);
        self.spawn_backend_load(backend, source, path, Some(id.to_string()), kind, true);
    }

    /// Cambia la pestaña del diff (Diff / Before / After) sobre la misma
    /// celda, conservando la vista para comparar la misma zona.
    fn select_diff_tab(&mut self, tab: DiffTab) {
        let Some(bs) = &self.backend_state else { return };
        let LoadKind::Diff { before, .. } = &bs.kind else { return };
        let kind = LoadKind::Diff { before: before.clone(), tab };
        let entry = bs.scene.current_entry().map(str::to_string);
        let (backend, source, path) = (bs.backend.clone(), bs.source.clone(), bs.path.clone());
        self.error = None;
        self.spawn_backend_load(backend, source, path, entry, kind, false);
    }

    /// Ruta de lo que se está viendo: archivo › celda › vista del diff.
    fn breadcrumb(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if let Some(ctx) = &self.diff_ctx {
            if ctx.from_history {
                parts.push(tr!("history.title"));
            }
            parts.push(format!("{} → {}", short_hash(&ctx.commit_a), short_hash(&ctx.commit_b)));
        }
        if let Some(p) = &self.selected_path {
            parts.push(p.file_name().unwrap_or_default().to_string_lossy().to_string());
        }
        if let Some(bs) = self.backend_state.as_ref() {
            if let Some(cell) = bs.scene.current_entry() {
                parts.push(cell.to_string());
            }
            if let LoadKind::Diff { tab, .. } = bs.kind {
                parts.push(tab_label(tab).to_string());
            }
        }
        #[cfg(feature = "spice")]
        if let Some(w) = self.wave.as_ref().filter(|w| w.is_diff()) {
            parts.push(tab_label(w.tab).to_string());
        }
        parts
    }

    /// Pantalla inicial: qué es esto, cómo empezar, archivos recientes y
    /// atajos. Retorna el archivo reciente elegido, si hubo clic.
    fn empty_state(&self, ui: &mut egui::Ui) -> Option<PathBuf> {
        let mut picked = None;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.22).max(space::L));
            ui.label(RichText::new(tr!("empty.title")).size(22.0).strong());
            ui.add_space(space::XS);
            ui.label(RichText::new(tr!("empty.hint")).weak());
            ui.add_space(space::L);

            let recent: Vec<&String> = self.recent.iter().filter(|p| Path::new(p).is_file()).collect();
            if !recent.is_empty() {
                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::same(space::M as i8))
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);
                        ui.label(RichText::new(tr!("empty.recent")).strong());
                        ui.add_space(space::XS);
                        for p in recent {
                            let path = Path::new(p);
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            let dir = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                            let resp = ui
                                .add(egui::Button::new(RichText::new(name.as_ref()).strong()).frame(false))
                                .on_hover_text(p.as_str())
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                            ui.add(egui::Label::new(RichText::new(dir).small().weak()).truncate());
                            if resp.clicked() {
                                picked = Some(path.to_path_buf());
                            }
                        }
                    });
                ui.add_space(space::L);
            }
            ui.label(
                RichText::new(tr!("empty.shortcuts"))
                    .small()
                    .weak(),
            );
        });
        picked
    }

    /// Abre en el lienzo lo que pidió el panel History.
    fn handle_history_request(&mut self, req: HistoryRequest) {
        let HistoryRequest::OpenDiff { parent, commit, path } = req;
        let Some(repo) = self.history.repo().map(Path::to_path_buf) else { return };
        let parent = parent.unwrap_or_default();
        let file = PathBuf::from(&path);
        self.diff_ctx = Some(DiffContext { commit_a: parent.clone(), commit_b: commit.clone(), file: file.clone(), from_history: true });
        self.selected_path = Some(repo.join(&path));
        match self.load_backend_diff(&repo, &parent, &commit, &file, None) {
            Ok(()) => self.status = format!("Diff {} → {} · {path}", short_hash(&parent), short_hash(&commit)),
            Err(e) => self.fail(&tr!("error.diff"), e),
        }
    }

    /// Encuadrar todo, pedido por el usuario (se anima salvo movimiento reducido).
    fn request_fit(&mut self) {
        if let Some(bs) = self.backend_state.as_mut() {
            bs.needs_fit = true;
            bs.animate_fit = true;
        }
    }

    /// Atajos de teclado, solo si ningún campo de texto tiene el foco (si no,
    /// escribir "f" en el buscador de celdas encuadraría la vista).
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::H)) {
            self.history.toggle();
        }
        let (fit, labels, zoom_in, zoom_out) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::L),
                // Todas las pulsaciones del cuadro: con un layout pesado, varias
                // caen en el mismo y no deben perderse.
                i.num_presses(egui::Key::Plus) + i.num_presses(egui::Key::Equals),
                i.num_presses(egui::Key::Minus),
            )
        });
        if fit {
            self.request_fit();
        }
        if labels {
            self.show_labels = !self.show_labels;
            // Con el teclado no se ve el botón cambiar: confirmarlo.
            let msg = if self.show_labels { tr!("toast.labels_on") } else { tr!("toast.labels_off") };
            self.notify(ToastKind::Info, msg);
        }
        if let Some(bs) = self.backend_state.as_mut() {
            let step: f64 = 1.25;
            let presses = zoom_in as i32 - zoom_out as i32;
            let factor = (presses != 0).then(|| step.powi(presses));
            if let Some(f) = factor {
                bs.pending_zoom = Some(bs.pending_zoom.unwrap_or(1.0) * f);
            }
        }
    }

    /// Pestaña actual si hay un diff cargado via backend.
    fn backend_diff_tab(&self) -> Option<DiffTab> {
        match &self.backend_state.as_ref()?.kind {
            LoadKind::Diff { tab, .. } => Some(*tab),
            _ => None,
        }
    }

    /// Selector de celdas bajo el panel izquierdo, si el archivo tiene más de una.
    fn show_entry_picker(&mut self, ui: &mut egui::Ui) {
        let picked = match &mut self.backend_state {
            Some(bs) if bs.scene.entries().len() > 1 => {
                ui.separator();
                render_entry_picker(ui, bs)
            }
            _ => None,
        };
        if let Some(id) = picked {
            self.select_entry(&id);
        }
    }

    /// Abre un `.raw` en la vista de formas de onda. `true` si se pudo.
    #[cfg(feature = "spice")]
    fn open_raw(&mut self, path: &Path) -> bool {
        match read_raw(path) {
            Ok(file) => {
                let view = WaveView::single(file, path.to_path_buf(), &self.wave_exprs);
                self.status = view.summary();
                self.backend_state = None;
                self.wave = Some(view);
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
    fn handle_wave_request(&mut self) {
        let Some(view) = self.wave.as_mut() else { return };
        let Some(req) = view.request.take() else { return };
        let Some(path) = view.path.clone() else { return };
        match req {
            wave_view::Request::CompareWith(other) => match (read_raw(&other), read_raw(&path)) {
                (Ok(a), Ok(b)) => {
                    let label = |p: &Path| p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let view = WaveView::compare(a, b, label(&other), label(&path), Some(path.clone()), &self.wave_exprs);
                    self.status = view.summary();
                    self.wave = Some(view);
                }
                (Err(e), _) | (_, Err(e)) => self.fail(&tr!("error.compare"), e),
            },
            wave_view::Request::StopComparing => {
                self.open_raw(&path);
            }
        }
    }

    /// Selector Diff / Before / After de una comparación de formas de onda.
    /// Cambiar de vista no recarga nada: la vista ya tiene las dos versiones.
    #[cfg(feature = "spice")]
    fn show_wave_tabs(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(w) = self.wave.as_mut().filter(|w| w.is_diff()) else { return false };
        ui.heading(tr!("panel.views"));
        if let Some(p) = &self.selected_path {
            ui.label(RichText::new(p.file_name().unwrap_or_default().to_string_lossy().as_ref())
                .color(egui::Color32::from_gray(180)));
        }
        ui.label(RichText::new(format!("{} → {}", w.label_a, w.label_b)).small().color(egui::Color32::from_gray(140)));
        ui.separator();
        for (tab, label, hint) in [
            (DiffTab::Diff, "Diff", tr!("wave.tab_diff_hint")),
            (DiffTab::Before, "Before", tr!("wave.tab_before_hint")),
            (DiffTab::After, "After", tr!("wave.tab_after_hint")),
        ] {
            ui.radio_value(&mut w.tab, tab, label).on_hover_text(hint);
        }
        true
    }

    /// Relee el archivo del disco conservando la sub-vista actual.
    fn reload_backend(&mut self) {
        #[cfg(feature = "spice")]
        if let Some(path) = self.wave.as_ref().filter(|w| !w.is_diff()).and_then(|w| w.path.clone()) {
            self.open_raw(&path);
            return;
        }
        let Some(bs) = &self.backend_state else { return };
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

    fn spawn_backend_load(
        &mut self,
        backend: Arc<dyn ViewerBackend>,
        source: Arc<Vec<u8>>,
        path: String,
        entry: Option<String>,
        kind: LoadKind,
        refit: bool,
    ) {
        // Cancelar carga previa si había.
        if let Some(tok) = self.pending_token.take() {
            tok.cancel();
        }
        let token = CancellationToken::new();
        self.pending_token = Some(token.clone());
        self.loading_path = Some(path.clone());

        let _guard = self.runtime.enter();
        let fut = async move {
            let hint = Some(path.clone());
            let result = match &kind {
                LoadKind::Single | LoadKind::Diff { tab: DiffTab::After, .. } => {
                    backend.load_entry(source.as_ref().clone(), hint, entry, token).await
                }
                LoadKind::Diff { before, tab: DiffTab::Before } => {
                    backend.load_entry(before.as_ref().clone(), hint, entry, token).await
                }
                LoadKind::Diff { before, tab: DiffTab::Diff } => {
                    backend
                        .load_diff(before.as_ref().clone(), source.as_ref().clone(), hint, entry, token)
                        .await
                }
            };
            result
                .map(|scene| LoadedScene { scene, backend, source, path, kind, refit })
                .map_err(|e| e.to_string())
        };
        self.pending_load = Some(Promise::spawn_async(fut));
    }

    /// Drenar el promise si está listo. Se llama desde `ui()`.
    fn poll_pending_load(&mut self) {
        let ready = self.pending_load.as_ref().and_then(|p| p.ready()).is_some();
        if !ready { return; }
        let Some(promise) = self.pending_load.take() else { return };
        match promise.block_and_take() {
            Ok(loaded) => {
                let name = loaded.backend.info().name;
                let what = match &loaded.kind {
                    LoadKind::Single => tr!("status.opened", name = name),
                    LoadKind::Diff { tab: DiffTab::Diff, .. } => "Diff".to_string(),
                    LoadKind::Diff { tab: DiffTab::Before, .. } => "Before".to_string(),
                    LoadKind::Diff { tab: DiffTab::After, .. } => "After".to_string(),
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
                // Mismo archivo (cambio de celda, de pestaña o recarga): se
                // conservan capas ocultas, buscador y vista. Una celda nueva
                // tiene otro tamaño → re-encuadre; otra pestaña, no.
                let prev = self.backend_state.take().filter(|bs| bs.path == loaded.path);
                self.backend_state = Some(match prev {
                    Some(bs) => BackendState {
                        scene: loaded.scene,
                        backend: loaded.backend,
                        source: loaded.source,
                        kind: loaded.kind,
                        needs_fit: loaded.refit || bs.needs_fit,
                        ..bs
                    },
                    None => BackendState {
                        scene: loaded.scene,
                        viewport: VcViewport::default(),
                        backend: loaded.backend,
                        source: loaded.source,
                        path: loaded.path,
                        needs_fit: true,
                        fitted_size: None,
                        hidden_layers: HashSet::new(),
                        entry_query: String::new(),
                        only_roots: true,
                        only_changed: true,
                        kind: loaded.kind,
                        focus: None,
                        animate_fit: false,
                        pending_zoom: None,
                        anim: None,
                        inertia: None,
                    },
                });
            }
            Err(e) => {
                // Ignoramos errores de cancelación — vienen de nosotros mismos
                // al abrir otro archivo antes de que terminara la carga previa.
                if !e.contains("cancelled") {
                    let path = self.loading_path.take().unwrap_or_default();
                    let name = Path::new(&path).file_name().unwrap_or_default().to_string_lossy().to_string();
                    // Un archivo que no abre no debe ofrecerse como reciente.
                    self.recent.retain(|r| *r != path);
                    self.fail(&tr!("error.open", file = name), friendly_error(&e));
                }
            }
        }
        self.pending_token = None;
    }

}

impl eframe::App for RikuGuiApp {
    /// Preferencias propias; el tema lo persiste egui junto a su memoria.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PREF_LABELS, &self.show_labels);
        eframe::set_value(storage, PREF_ALL_FILES, &self.show_all_files);
        eframe::set_value(storage, PREF_REDUCE_MOTION, &self.reduce_motion);
        eframe::set_value(storage, PREF_HISTORY_H, &self.history.height);
        eframe::set_value(storage, PREF_SIMPLIFY, &self.simplify);
        eframe::set_value(storage, PREF_RECENT, &self.recent);
        eframe::set_value(storage, PREF_LANG, &i18n::current());
        #[cfg(feature = "spice")]
        eframe::set_value(storage, PREF_WAVE_EXPRS, &self.wave_exprs);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.now = ctx.input(|i| i.time);

        // Cambio de tema: fundir desde el fondo anterior en vez de saltar de
        // golpe entre claro y oscuro (salvo movimiento reducido).
        let dark = ctx.theme() == egui::Theme::Dark;
        if self.last_dark.is_some_and(|was| was != dark) && !self.reduce_motion {
            let prev = if dark { egui::Visuals::light() } else { egui::Visuals::dark() };
            self.theme_fade = Some((self.now, CanvasTheme::from_visuals(&prev).background));
        }
        self.last_dark = Some(dark);

        // Arrastrar un archivo desde el explorador lo abre.
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
        if let Some(path) = dropped {
            self.open_path(&path);
        }

        // Drenar carga async antes de pintar; si sigue en vuelo, solicitar
        // repaint para que el promise se consulte en el siguiente frame.
        self.poll_pending_load();
        if self.pending_load.is_some() {
            ctx.request_repaint();
        }

        self.handle_shortcuts(&ctx);

        // Título de ventana: archivo (y celda) abiertos.
        let file = self.selected_path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
        let cell = self.backend_state.as_ref().and_then(|bs| bs.scene.current_entry().map(str::to_string));
        let title = match (file, cell) {
            (Some(f), Some(c)) => format!("{f} · {c} — Riku"),
            (Some(f), None) => format!("{f} — Riku"),
            _ => "Riku".to_string(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }

        // ─── Barra de herramientas: acciones, de izquierda a derecha por uso ──
        egui::Panel::top("top_bar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Riku").strong().size(16.0));
                ui.separator();

                let has_view = self.backend_state.is_some();
                if ui
                    .add_enabled(has_view, egui::Button::new(tr!("toolbar.fit")))
                    .on_hover_text(tr!("toolbar.fit_hint"))
                    .clicked()
                {
                    self.request_fit();
                }
                if ui
                    .button(tr!("toolbar.reload"))
                    .on_hover_text(tr!("toolbar.reload_hint"))
                    .clicked()
                {
                    self.refresh_tree();
                    // El archivo abierto se relee del disco en la misma sub-vista.
                    self.reload_backend();
                }
                ui.separator();
                ui.toggle_value(&mut self.show_labels, tr!("toolbar.labels"))
                    .on_hover_text(tr!("toolbar.labels_hint"));
                let has_repo = self.history.repo().is_some();
                let mut open = self.history.open;
                let hint = if has_repo { tr!("toolbar.history_hint") } else { tr!("toolbar.history_no_repo") };
                if ui
                    .add_enabled(has_repo, egui::Button::selectable(open, tr!("history.title")))
                    .on_hover_text(hint.clone())
                    .on_disabled_hover_text(hint)
                    .clicked()
                {
                    open = !open;
                    self.history.open = open;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Lo menos frecuente, un nivel más abajo.
                    ui.menu_button(tr!("settings.menu"), |ui| {
                        ui.checkbox(&mut self.reduce_motion, tr!("settings.reduce_motion"))
                            .on_hover_text(tr!("settings.reduce_motion_hint"));
                        ui.checkbox(&mut self.simplify, tr!("settings.simplify"))
                            .on_hover_text(tr!("settings.simplify_hint"));
                        ui.separator();
                        ui.label(RichText::new(tr!("settings.language")).strong());
                        let current = i18n::current();
                        ui.horizontal(|ui| {
                            for (code, name) in i18n::LANGUAGES {
                                if ui.selectable_label(current == code, name).clicked() {
                                    i18n::set(code);
                                }
                            }
                        });
                        ui.separator();
                        ui.label(RichText::new(tr!("settings.shortcuts")).strong());
                        for (k, what) in [
                            ("F".to_string(), tr!("shortcut.fit")),
                            ("L".to_string(), tr!("shortcut.labels")),
                            ("H".to_string(), tr!("shortcut.history")),
                            ("+ / −".to_string(), tr!("shortcut.zoom_keys")),
                            (tr!("shortcut.wheel"), tr!("shortcut.wheel_what")),
                            (tr!("shortcut.drag"), tr!("shortcut.drag_what")),
                        ] {
                            ui.horizontal(|ui| {
                                ui.monospace(format!("{k:>9}"));
                                ui.label(what);
                            });
                        }
                    });
                    ui.separator();
                    theme_selector(ui);
                });
            });
        });

        // ─── Barra de estado: qué pasa, dónde está el cursor, escala ─────────
        egui::Panel::bottom("status_bar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if self.pending_load.is_some() {
                    ui.spinner();
                }
                let status = RichText::new(&self.status);
                ui.label(if self.error.is_some() { status.color(ui.visuals().error_fg_color) } else { status });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let unit = self.backend_state.as_ref()
                        
                        .and_then(|bs| bs.scene.world_unit().map(str::to_string))
                        .unwrap_or_default();
                    if let Some(px) = self.px_world {
                        ui.label(RichText::new(format!("1 px = {} {unit}", fmt_len(px))).weak());
                    }
                    if let Some((x, y)) = self.cursor_world {
                        ui.separator();
                        ui.monospace(format!("x {:>9.3}  y {:>9.3} {unit}", x, y));
                    }
                    if self.labels_hidden > 0 && self.show_labels {
                        ui.separator();
                        ui.label(RichText::new(tr!("status.labels_hidden", count = self.labels_hidden)).weak());
                    }
                });
            });
        });

        // ─── History: abajo, a todo el ancho, sobre la barra de estado ───────
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.05) as f64;
        // Relativo al repo; un diff abierto entre commits ya guarda la ruta
        // relativa.
        let open_file = self.selected_path.as_ref().and_then(|p| {
            let rel = if p.is_relative() { p.as_path() } else { p.strip_prefix(self.history.repo()?).ok()? };
            Some(rel.to_string_lossy().replace('\\', "/"))
        });
        for req in self.history.show(ui, self.now, dt, self.reduce_motion, open_file) {
            self.handle_history_request(req);
        }

        egui::Panel::left("left_panel")
            .resizable(true)
            .default_size(200.0)
            .show_inside(ui, |ui| {
                // Formas de onda comparadas: mismas vistas que los demás formatos.
                // Entre commits reemplazan al árbol; comparando dos archivos del
                // proyecto, el árbol sigue abajo para abrir otro.
                #[cfg(feature = "spice")]
                let wave_tabs = self.show_wave_tabs(ui);
                #[cfg(not(feature = "spice"))]
                let wave_tabs = false;

                // Modo diff: selector de vistas (Diff/Before/After); cada una es otra carga.
                if let (Some(ctx), Some(current)) = (self.diff_ctx.as_ref(), self.backend_diff_tab()) {
                    ui.heading(tr!("panel.views"));
                    ui.label(RichText::new(ctx.file.file_name()
                        .unwrap_or_default().to_string_lossy().as_ref())
                        .color(egui::Color32::from_gray(180)));
                    ui.label(RichText::new(format!("{} → {}",
                        short_hash(&ctx.commit_a), short_hash(&ctx.commit_b)))
                        .small().color(egui::Color32::from_gray(140)));
                    ui.separator();
                    let mut tab = current;
                    view_selector(ui, &mut tab, DiffTab::Diff, "Diff");
                    view_selector(ui, &mut tab, DiffTab::Before, "Before");
                    view_selector(ui, &mut tab, DiffTab::After, "After");
                    if tab != current {
                        self.select_diff_tab(tab);
                    }
                    self.show_entry_picker(ui);
                } else if !(wave_tabs && self.diff_ctx.is_some()) {
                    if wave_tabs {
                        ui.separator();
                    }
                    // Modo archivo único: árbol de proyecto
                    ui.heading(tr!("panel.project"));
                    let root = self.project_root.display().to_string();
                    ui.add(egui::Label::new(RichText::new(&root).small().weak()).truncate())
                        .on_hover_text(&root);
                    if ui
                        .checkbox(&mut self.show_all_files, tr!("panel.all_files"))
                        .on_hover_text(tr!("panel.all_files_hint"))
                        .changed()
                    {
                        self.refresh_tree();
                    }
                    ui.separator();
                    let tree = self.project_tree.clone();
                    let selected_path = self.selected_path.clone();
                    // Con scroll propio: un árbol más alto que la ventana
                    // agrandaba toda la UI y el lienzo quedaba fuera de pantalla.
                    // Si abajo va el selector de celdas, el árbol cede espacio.
                    let has_picker = self.backend_state.as_ref().is_some_and(|bs| bs.scene.entries().len() > 1);
                    let tree_h = ui.available_height() * if has_picker { 0.4 } else { 1.0 };
                    egui::ScrollArea::vertical()
                        .id_salt("project_tree")
                        .max_height(tree_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            let mut open_path = |path: &Path| self.open_path(path);
                            show_entry_tree(ui, &tree, selected_path.as_deref(), &mut open_path);
                        });
                    self.show_entry_picker(ui);
                }
            });

        egui::Panel::right("info_panel")
            .resizable(true)
            .default_size(220.0)
            .show_inside(ui, |ui| {
                ui.heading(tr!("panel.details"));
                if let Some(path) = &self.selected_path {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    ui.add(egui::Label::new(RichText::new(&name).strong()).truncate())
                        .on_hover_text(path.display().to_string());
                }
                ui.add_space(space::XS);

                #[cfg(feature = "spice")]
                if let Some(view) = self.wave.as_mut() {
                    let candidates = raw_files(&self.project_tree);
                    wave_view::show_details(ui, view, &candidates);
                    if view.take_exprs_changed() {
                        self.wave_exprs = view.expr_texts().to_vec();
                    }
                    return;
                }
                if let Some(bs) = &mut self.backend_state {
                    render_backend_details(ui, bs);
                } else {
                    ui.label(RichText::new(tr!("panel.nothing_open")).weak());
                }
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.canvas_rect = Some(ui.max_rect());
            // Lecturas para la barra de estado: las repone quien pinte.
            self.cursor_world = None;
            self.px_world = None;
            self.labels_hidden = 0;

            // ¿Dónde estoy? Ruta archivo › celda › vista sobre el lienzo.
            let crumbs = self.breadcrumb();
            if !crumbs.is_empty() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    let last = crumbs.len() - 1;
                    for (i, c) in crumbs.iter().enumerate() {
                        let t = RichText::new(c);
                        ui.label(if i == last { t.strong() } else { t.weak() });
                        if i < last {
                            ui.label(RichText::new("›").weak());
                        }
                    }
                });
                ui.add_space(space::XS);
            }

            // Formas de onda: su propia vista (ejes, unidades, A vs B).
            #[cfg(feature = "spice")]
            if let Some(view) = self.wave.as_mut() {
                wave_view::show_plot(ui, view);
                return;
            }

            // Escena cargada por el backend del formato (todos los formatos).
            {
                if let Some(bs) = &mut self.backend_state {
                    let available = ui.available_size_before_wrap();
                    let response = ui.allocate_response(available, egui::Sense::drag());
                    canvas_cursor(&ctx, &response);
                    let rect = response.rect;
                    let (w, h) = (rect.width() as f64, rect.height() as f64);
                    let animate = !self.reduce_motion;

                    // El usuario manda: tocar el lienzo o usar la rueda frena
                    // cualquier animación o inercia en el valor que tenga en
                    // pantalla (interrumpible, sin saltos).
                    let pressed = response.hovered() && ctx.input(|i| i.pointer.any_pressed());
                    let scroll = ctx.input(|i| i.smooth_scroll_delta.y as f64);
                    let wheel = scroll.abs() > f64::EPSILON && response.hovered();
                    if pressed || wheel {
                        bs.anim = None;
                        bs.inertia = None;
                    }
                    if response.dragged() {
                        let delta = response.drag_delta();
                        bs.viewport.pan_by_screen(delta.x as f64, delta.y as f64);
                        bs.fitted_size = None;
                        ctx.request_repaint();
                    }
                    // Al soltar, la vista sigue a la velocidad del puntero y
                    // desacelera (proyección de momento).
                    if response.drag_stopped() && animate {
                        let v = ctx.input(|i| i.pointer.velocity());
                        bs.inertia = Inertia::from_release(v.x as f64, v.y as f64);
                    }
                    if wheel {
                        // Zoom anclado al cursor (o al centro si no hay puntero).
                        let anchor = response.hover_pos().unwrap_or(rect.center());
                        zoom_at_screen(&mut bs.viewport, 1.0 + scroll * 0.002, anchor, rect);
                        bs.fitted_size = None;
                        ctx.request_repaint();
                    }

                    // Cambio de vista hacia `target`: animado si lo pidió el
                    // usuario (y no hay movimiento reducido), inmediato si no.
                    let go_to = |bs: &mut BackendState, target: VcViewport, user: bool| {
                        if user && animate {
                            bs.anim = Some(ViewAnimation::to(target));
                        } else {
                            bs.viewport = target;
                        }
                        bs.inertia = None;
                    };

                    // Encuadre al cargar / "Encuadrar", y de nuevo si el lienzo
                    // cambia de tamaño (paneles, ventana) mientras el usuario no
                    // haya movido la vista a mano.
                    let size = rect.size();
                    let resized = bs.anim.is_none() && bs.fitted_size.is_some_and(|s| s != size);
                    if (bs.needs_fit || resized) && size.x > 0.0 && size.y > 0.0 {
                        let mut target = bs.viewport;
                        fit_scene(&mut target, bs.scene.as_ref(), rect);
                        let user = std::mem::take(&mut bs.animate_fit);
                        go_to(bs, target, user);
                        bs.needs_fit = false;
                        bs.fitted_size = Some(size);
                    }
                    // Clic en un cambio: encuadrarlo con contexto alrededor. Es
                    // una vista elegida, no se re-encuadra sola al redimensionar.
                    if let Some(area) = bs.focus.take() {
                        let mut target = bs.viewport;
                        fit_bbox(&mut target, &focus_area(&area, &bs.scene.bbox()), bs.scene.y_axis(), rect);
                        go_to(bs, target, true);
                        bs.fitted_size = None;
                    }
                    // Zoom por teclado (+/−) sobre el centro del lienzo.
                    if let Some(factor) = bs.pending_zoom.take() {
                        let mut target = bs.anim.map_or(bs.viewport, |a| a.target());
                        zoom_at_screen(&mut target, factor, rect.center(), rect);
                        go_to(bs, target, true);
                        bs.fitted_size = None;
                    }

                    // Avanzar animación e inercia (tiempo real, no por frame).
                    let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.05) as f64;
                    if let Some(mut anim) = bs.anim.take() {
                        if !anim.step(&mut bs.viewport, dt, w, h) {
                            bs.anim = Some(anim);
                        }
                        ctx.request_repaint();
                    }
                    if let Some(mut it) = bs.inertia.take() {
                        let ((dx, dy), alive) = it.step(dt);
                        bs.viewport.pan_by_screen(dx, dy);
                        bs.fitted_size = None;
                        if alive {
                            bs.inertia = Some(it);
                        }
                        ctx.request_repaint();
                    }
                    let hidden = bs.hidden_keys();
                    let opts = PaintOptions {
                        theme: CanvasTheme::from_visuals(ui.visuals()),
                        labels: self.show_labels,
                        lod: self.simplify,
                        block_px: self.block_px,
                    };
                    let t_paint = std::time::Instant::now();
                    let stats = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(response.rect), |ui| {
                            paint_scene(ui, bs.scene.as_ref(), &bs.viewport, &hidden, opts)
                        })
                        .inner;
                    if self.profile {
                        eprintln!(
                            "PROFILE paint_scene {:.1} ms · entre cuadros {:.1} ms · {} elementos · nivel {:?}",
                            t_paint.elapsed().as_secs_f64() * 1e3,
                            ctx.input(|i| i.unstable_dt) as f64 * 1e3,
                            stats.elements,
                            stats.lod_level
                        );
                    }
                    // Para la barra de estado (se muestra en el próximo frame).
                    self.labels_hidden = stats.labels_hidden;
                    self.px_world = Some(1.0 / bs.viewport.scale);
                    let xf = ScreenXform::new(response.rect, &bs.viewport, bs.scene.y_axis());
                    self.cursor_world = response.hover_pos().map(|p| xf.to_world(p));
                    // Tooltip con capa/área del polígono bajo el cursor (no
                    // mientras se arrastra: estorba al hacer pan).
                    if let Some(pos) = response.hover_pos().filter(|_| !response.dragged()) {
                        if let Some(info) = hover_info(bs.scene.as_ref(), &bs.viewport, response.rect, pos, &hidden) {
                            response.on_hover_text_at_pointer(info);
                        }
                    }
                    return;
                }
            }

            if self.pending_load.is_some() {
                    ui.centered_and_justified(|ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tr!("canvas.loading"));
                        });
                    });
                } else if let Some(path) = self.empty_state(ui) {
                    self.open_path(&path);
            }
        });

        #[cfg(feature = "spice")]
        self.handle_wave_request();

        // Archivo arrastrado sobre la ventana: indicar que se puede soltar.
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            drop_hint(&ctx);
        }
        let area = self.canvas_rect.unwrap_or_else(|| ctx.content_rect());
        self.toasts.show(&ctx, area);

        if let Some((start, from)) = self.theme_fade {
            match theme_fade_alpha(self.now - start) {
                Some(alpha) => {
                    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("theme_fade"));
                    ctx.layer_painter(layer).rect_filled(ctx.content_rect(), 0.0, from.gamma_multiply(alpha));
                    ctx.request_repaint();
                }
                None => self.theme_fade = None,
            }
        }
    }
}

#[cfg(feature = "spice")]
fn is_raw(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("raw"))
}

#[cfg(feature = "spice")]
fn read_raw(path: &Path) -> Result<crate::modules::spice::raw::RawFile, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    crate::modules::spice::raw::parse(&bytes).map_err(|e| e.to_string())
}

/// Archivos `.raw` del árbol de proyecto (candidatos para comparar).
#[cfg(feature = "spice")]
fn raw_files(tree: &ProjectEntry) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![tree];
    while let Some(e) = stack.pop() {
        match e {
            ProjectEntry::Directory { children, .. } => stack.extend(children.iter()),
            ProjectEntry::File { path, .. } if is_raw(path) => out.push(path.clone()),
            ProjectEntry::File { .. } => {}
        }
    }
    out.sort();
    out
}

/// Velo sobre toda la ventana mientras se arrastra un archivo encima.
fn drop_hint(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop_hint")));
    let v = ctx.global_style().visuals.clone();
    painter.rect_filled(screen, 0.0, v.window_fill.gamma_multiply(0.85));
    painter.rect_stroke(
        screen.shrink(space::L),
        12.0,
        egui::Stroke::new(2.0_f32, v.selection.stroke.color),
        egui::StrokeKind::Inside,
    );
    painter.text(
        screen.center(),
        egui::Align2::CENTER_CENTER,
        tr!("drop.hint"),
        egui::FontId::proportional(20.0),
        v.strong_text_color(),
    );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────


// ─── Selector de celdas de una escena cargada via backend ───────────────────

/// Retorna el id de la celda elegida (la carga la dispara el caller).
fn render_entry_picker(ui: &mut egui::Ui, bs: &mut BackendState) -> Option<String> {
    let scene = bs.scene.clone();
    entry_picker::show(
        ui,
        scene.entries(),
        scene.current_entry(),
        entry_picker::PickerState {
            query: &mut bs.entry_query,
            only_roots: &mut bs.only_roots,
            only_changed: &mut bs.only_changed,
        },
    )
}

// ─── Lista de cambios de una escena de diff ─────────────────────────────────

/// Sección plegable de un panel: título con peso, contador tenue y estado
/// abierto/cerrado que egui recuerda. Agrupa lo relacionado y deja plegar lo
/// que no se usa (una lista de 40 capas no debe esconder el resumen).
fn section(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    count: Option<usize>,
    default_open: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let mut job = egui::text::LayoutJob::default();
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    let font = egui::TextStyle::Body.resolve(ui.style());
    job.append(title, 0.0, egui::TextFormat::simple(font.clone(), strong));
    if let Some(n) = count {
        job.append(&n.to_string(), space::S, egui::TextFormat::simple(font, weak));
    }
    egui::CollapsingHeader::new(job)
        .id_salt(id)
        .default_open(default_open)
        .show(ui, |ui| {
            add_contents(ui);
            ui.add_space(space::XS);
        });
}

/// Lista de cambios (relevantes primero; los cosméticos en gris). Un clic en
/// un cambio con ubicación retorna su bbox para encuadrarlo.
fn render_change_items(ui: &mut egui::Ui, changes: &[viewer_core::ChangeItem]) -> Option<BoundingBox> {
    let mut picked = None;
    egui::ScrollArea::vertical()
        .id_salt("change_list")
        .max_height(220.0)
        .show(ui, |ui| {
            for c in changes {
                let sign = match c.kind {
                    ChangeKind::Added => "+",
                    ChangeKind::Removed => "−",
                    ChangeKind::Modified => "~",
                };
                let color = crate::gui::theme::change_color(c.kind, ui.visuals().dark_mode);
                let dim = |col: egui::Color32| if c.cosmetic { col.gamma_multiply(0.45) } else { col };
                let text = RichText::new(format!("{sign} {}", c.label)).color(dim(color));
                let resp = ui.add(
                    egui::Label::new(text)
                        .truncate()
                        .sense(if c.bbox.is_some() { egui::Sense::click() } else { egui::Sense::hover() }),
                );
                if c.bbox.is_some() {
                    resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
                }
                if !c.detail.is_empty() {
                    ui.label(RichText::new(&c.detail).small().color(dim(ui.visuals().weak_text_color())));
                }
                let hover = match (c.cosmetic, c.bbox.is_some()) {
                    (true, _) => tr!("change.cosmetic_hint"),
                    (false, true) => tr!("change.go_hint"),
                    (false, false) => tr!("change.no_location"),
                };
                if resp.on_hover_text(hover).clicked() {
                    picked = c.bbox;
                }
            }
        });
    picked
}

// ─── Detalles de una escena cargada via backend ─────────────────────────────

/// Detalles en secciones: resumen (metadatos del backend), cambios (en diff)
/// y capas con color y visibilidad. Las capas salen en el orden que da la
/// escena (para GDS: de abajo hacia arriba en el apilado del PDK).
fn render_backend_details(ui: &mut egui::Ui, bs: &mut BackendState) {
    let scene = bs.scene.clone();

    let meta = scene.metadata();
    if !meta.is_empty() {
        section(ui, "summary", &tr!("details.summary"), None, true, |ui| {
            egui::Grid::new("scene_meta").num_columns(2).spacing([space::M, space::XS]).show(ui, |ui| {
                for (k, v) in meta {
                    ui.label(RichText::new(k).weak());
                    // Truncado: un nombre de celda largo no debe ensanchar el
                    // panel (y achicar el lienzo); completo en el tooltip.
                    ui.add(egui::Label::new(v).truncate()).on_hover_text(v);
                    ui.end_row();
                }
            });
        });
    }

    // Avisos del backend (p. ej. símbolos sin resolver): arriba, visibles.
    let notices = scene.notices();
    if !notices.is_empty() {
        section(ui, "notices", &tr!("details.notices"), Some(notices.len()), true, |ui| {
            for n in notices {
                ui.label(RichText::new(n).small().color(ui.visuals().warn_fg_color));
            }
        });
    }

    let changes = scene.changes();
    if !changes.is_empty() {
        let mut picked = None;
        section(ui, "changes", &tr!("details.changes"), Some(changes.len()), true, |ui| {
            picked = render_change_items(ui, changes);
        });
        if let Some(target) = picked {
            bs.focus = Some(target);
        }
    }

    let layers = scene.layer_list();
    if layers.is_empty() {
        return;
    }
    let shown = layers.iter().filter(|(_, p)| !bs.hidden_layers.contains(&p.name)).count();
    let title_count = if shown == layers.len() { layers.len() } else { shown };
    section(ui, "layers", &tr!("details.layers"), Some(title_count), true, |ui| {
        ui.horizontal(|ui| {
            if ui.small_button(tr!("details.show_all")).clicked() {
                bs.hidden_layers.clear();
            }
            if ui.small_button(tr!("details.hide_all")).clicked() {
                bs.hidden_layers.extend(layers.iter().map(|(_, p)| p.name.clone()));
            }
            if shown < layers.len() {
                ui.label(RichText::new(tr!("details.hidden", count = layers.len() - shown)).small().weak());
            }
        });
        egui::ScrollArea::vertical().id_salt("layer_list").show(ui, |ui| {
            let theme = CanvasTheme::from_visuals(ui.visuals());
            for (_, paint) in &layers {
                ui.horizontal(|ui| {
                    let mut visible = !bs.hidden_layers.contains(&paint.name);
                    if ui.checkbox(&mut visible, "").changed() {
                        if visible {
                            bs.hidden_layers.remove(&paint.name);
                        } else {
                            bs.hidden_layers.insert(paint.name.clone());
                        }
                    }
                    // Muestra con los mismos colores que el lienzo (según el tema).
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    let (fill, stroke) = theme.layer_colors(to_color32(paint.fill), to_color32(paint.stroke));
                    ui.painter().rect(rect, 3.0, fill, egui::Stroke::new(1.5_f32, stroke), egui::StrokeKind::Inside);
                    let name = RichText::new(&paint.name);
                    ui.label(if visible { name } else { name.weak() });
                });
            }
        });
    });
}

// ─── Selector de vistas (modo diff) ──────────────────────────────────────────

/// Opción de vista del diff (Diff / Before / After) con su descripción: el
/// radio estándar se adapta al tema y se reconoce de inmediato.
fn view_selector(ui: &mut egui::Ui, current: &mut DiffTab, this: DiffTab, label: &str) {
    let hint = match this {
        DiffTab::Diff => tr!("tab.diff_hint"),
        DiffTab::Before => tr!("tab.before_hint"),
        DiffTab::After => tr!("tab.after_hint"),
    };
    ui.radio_value(current, this, label).on_hover_text(hint);
}

/// Selector de tema (claro / oscuro / según el sistema). Se dibuja de
/// derecha a izquierda. egui persiste la preferencia en su memoria.
fn theme_selector(ui: &mut egui::Ui) {
    use egui::ThemePreference as T;
    let current = ui.ctx().options(|o| o.theme_preference);
    // Orden visual (izq→der): Claro · Oscuro · Sistema.
    for (pref, label, tip) in [
        (T::System, tr!("theme.system"), tr!("theme.system_hint")),
        (T::Dark, tr!("theme.dark"), tr!("theme.dark_hint")),
        (T::Light, tr!("theme.light"), tr!("theme.light_hint")),
    ] {
        if ui.selectable_label(current == pref, label).on_hover_text(tip).clicked() {
            ui.ctx().set_theme(pref);
        }
    }
    ui.label(RichText::new(tr!("theme.label")).weak());
}

/// Longitud con 3 cifras significativas (`0.0123`, `1.25`, `310`).
fn fmt_len(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return format!("{v}");
    }
    let decimals = (2 - v.abs().log10().floor() as i32).clamp(0, 9) as usize;
    format!("{v:.decimals$}")
}

fn short_hash(s: &str) -> String {
    // Vacío: el "antes" del commit inicial.
    if s.is_empty() { "∅".to_string() } else { s.chars().take(7).collect() }
}

fn show_entry_tree<F>(
    ui: &mut egui::Ui,
    entry: &ProjectEntry,
    selected: Option<&Path>,
    on_select: &mut F,
) where
    F: FnMut(&Path),
{
    match entry {
        ProjectEntry::Directory { path, name, children } => {
            egui::CollapsingHeader::new(name)
                .default_open(selected.map_or(false, |s| s.starts_with(path)))
                .show(ui, |ui| {
                    for child in children {
                        show_entry_tree(ui, child, selected, on_select);
                    }
                });
        }
        ProjectEntry::File { path, name } => {
            if ui.selectable_label(selected == Some(path.as_path()), name).clicked() {
                on_select(path);
            }
        }
    }
}

/// Nombre visible de cada vista del diff.
fn tab_label(tab: DiffTab) -> &'static str {
    match tab {
        DiffTab::Diff => "Diff",
        DiffTab::Before => "Before",
        DiffTab::After => "After",
    }
}

/// Cursor sobre el lienzo: mano abierta (se puede mover) y cerrada al arrastrar.
fn canvas_cursor(ctx: &egui::Context, response: &egui::Response) {
    if response.dragged() {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// Error de carga en lenguaje claro. Conserva el mensaje técnico al final
/// (entre paréntesis) para quien necesite diagnosticar.
fn friendly_error(e: &str) -> String {
    let plain = if e.contains("GDSII") || e.contains("input file read error") {
        Some(tr!("error.bad_gds"))
    } else if e.contains("no existe") || e.contains("No such file") {
        Some(tr!("error.missing_file"))
    } else {
        None
    };
    match plain {
        Some(p) => format!("{p} ({e})"),
        None => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_error_explains_corrupt_gds_and_keeps_details() {
        let e = "parse error: GDSII parse: gdstk error: input file read error";
        let f = friendly_error(e);
        assert!(f.starts_with("not a valid GDSII"), "{f}");
        assert!(f.contains(e), "conserva el detalle técnico");
        assert_eq!(friendly_error("otra cosa"), "otra cosa");
    }

    #[test]
    fn fmt_len_uses_three_significant_digits() {
        assert_eq!(fmt_len(0.005678), "0.00568");
        assert_eq!(fmt_len(1.25), "1.25");
        assert_eq!(fmt_len(310.4), "310");
    }
}
