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
use crate::gui::launch::LaunchArgs;
use crate::gui::motion::{theme_fade_alpha, Inertia, ViewAnimation};
use crate::gui::project::ProjectEntry;
use crate::gui::sch_painter::{SchViewport, fit_viewport_to_scene, paint_sch};
use crate::gui::scene_painter::{
    fit_bbox, fit_scene, focus_area, hover_info, paint_scene, to_color32, zoom_at_screen, PaintOptions, ScreenXform,
};
use crate::gui::theme::{space, CanvasTheme};
use crate::gui::toast::{ToastKind, Toasts};

// ─── Estado del schematic ─────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffTab {
    Before,
    After,
    Diff,
}

struct SchState {
    /// Escena del commit B (estado posterior / archivo actual)
    scene: xschem_viewer::ResolvedScene,
    /// Escena del commit A (estado anterior) — solo en modo diff
    scene_a: Option<xschem_viewer::ResolvedScene>,
    viewport: SchViewport,
    diff: Option<crate::core::domain::models::FileChange>,
    /// Encuadrar en el próximo frame con el tamaño real del panel.
    needs_fit: bool,
    /// Tab activo (solo relevante en modo diff)
    tab: DiffTab,
}

// ─── App ──────────────────────────────────────────────────────────────────────

/// Contexto persistente de un diff cargado — permite recargarlo sin perder estado.
struct DiffContext {
    repo: PathBuf,
    commit_a: String,
    commit_b: String,
    file: PathBuf,
}

/// Estado de una carga genérica via `ViewerBackend` (GDS o cualquier formato
/// futuro). Convive con `SchState` — el path rico se conserva intacto para
/// schematics Xschem con features especiales (diff, fantasmas, pins).
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
    sch: Option<SchState>,
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

    // ─── Preferencias (persisten entre sesiones) ────────────────────────────
    /// Dibujar etiquetas de texto en el lienzo.
    show_labels: bool,
    /// Árbol de proyecto con todos los archivos, no solo los que se abren.
    show_all_files: bool,
    /// Sin animaciones ni inercia (accesibilidad: movimiento reducido).
    reduce_motion: bool,

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
}

/// Claves de persistencia (eframe storage).
const PREF_LABELS: &str = "riku.show_labels";
const PREF_ALL_FILES: &str = "riku.show_all_files";
const PREF_REDUCE_MOTION: &str = "riku.reduce_motion";
const PREF_RECENT: &str = "riku.recent_files";
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
        let recent: Vec<String> = cc.storage.and_then(|s| eframe::get_value(s, PREF_RECENT)).unwrap_or_default();

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
        let openable: Vec<String> =
            backends.iter().flat_map(|b| b.info().extensions.iter().map(|e| e.to_string())).collect();
        let project_tree = ProjectEntry::build(&project_root, show_all_files, &openable);

        let mut app = Self {
            project_root,
            project_tree,
            selected_path,
            sch: None,
            diff_ctx: None,
            status: String::from("Listo — abre un .sch, .gds u .oas del panel Proyecto"),
            error: None,
            runtime,
            backends,
            openable,
            backend_state: None,
            pending_load: None,
            pending_token: None,
            show_labels,
            show_all_files,
            reduce_motion,
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
        };

        // Modo diff: commits pasados desde el CLI
        if let (Some(file), Some(ca), Some(cb)) = (&launch.file, &launch.commit_a, &launch.commit_b) {
            let repo = launch.repo.as_deref().unwrap_or(Path::new("."));
            app.diff_ctx = Some(DiffContext {
                repo: repo.to_path_buf(),
                commit_a: ca.clone(),
                commit_b: cb.clone(),
                file: file.clone(),
            });
            // .sch: diff semántico rico; otros formatos (GDS): diff via backend.
            let result = if is_sch_renderable(file) {
                app.load_diff(repo, ca, cb, file)
            } else {
                app.load_backend_diff(repo, ca, cb, file, launch.cell.clone())
            };
            match result {
                Ok(()) => app.status = format!("Diff {} → {}", ca, cb),
                Err(e) => app.fail("No se pudo calcular el diff", e),
            }
        } else if let Some(path) = app.selected_path.clone() {
            app.remember_recent(&path);
            if is_sch_renderable(&path) {
                match app.load_sch(&path) {
                    Ok(()) => app.status = format!("Abierto {}", path.display()),
                    Err(e) => app.fail("No se pudo abrir", e),
                }
            } else if app.load_via_backend(&path, launch.cell.clone()) {
                // Otros formatos (ej: .gds) van por la ruta neutra ViewerBackend.
                app.status = format!("Cargando {} …", path.display());
            } else {
                app.status = format!("{} — formato no soportado", path.display());
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
        self.status = format!("{what} — error");
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
        self.sch = None;
        self.backend_state = None;
        self.remember_recent(path);
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();

        // .sch sigue por la ruta rica (diff semántico, fantasmas, etc.)
        if is_sch_renderable(path) {
            match self.load_sch(path) {
                Ok(()) => {
                    self.status = format!("Abierto {}", path.display());
                    self.notify(ToastKind::Success, format!("{name} abierto"));
                }
                Err(e) => self.fail(&format!("No se pudo abrir {name}"), e),
            }
            return;
        }

        // Fallback: intentar con algún backend registrado (ruta neutra async).
        if self.load_via_backend(path, None) {
            self.status = format!("Cargando {} …", path.display());
        } else if self.error.is_none() {
            self.status = format!("{} — formato no soportado aún", path.display());
            self.notify(ToastKind::Warning, format!("{name}: formato no soportado (se abren .sch, .sym, .gds y .oas)"));
        }
    }

    /// Intenta cargar `path` via alguno de los backends registrados; `entry`
    /// elige una sub-vista (celda GDS) o `None` para la de por defecto.
    /// Retorna `true` si algún backend aceptó el archivo (la carga queda en vuelo).
    fn load_via_backend(&mut self, path: &Path, entry: Option<String>) -> bool {
        let content = match std::fs::read(path) {
            Ok(c) => c,
            Err(e) => {
                self.fail(&format!("No se pudo leer {}", path.display()), e);
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

    /// Diff de un archivo no-Xschem entre dos commits via backend. El
    /// archivo puede no existir en el commit "antes" (archivo nuevo).
    fn load_backend_diff(&mut self, repo: &Path, commit_a: &str, commit_b: &str, file: &Path, entry: Option<String>) -> Result<(), String> {
        use crate::core::domain::ports::GitRepository;
        use crate::core::git::git_service::GitService;

        let svc = GitService::open(repo).map_err(|e| e.to_string())?;
        let file_str = file.to_string_lossy().to_string();
        let after = svc.get_blob(commit_b, &file_str).map_err(|e| format!("{commit_b}: {e}"))?;
        // Si falta en "antes" se compara contra vacío: todo cuenta como añadido.
        let before = svc.get_blob(commit_a, &file_str).unwrap_or_default();

        let backend = self.backends.iter()
            .find(|b| b.accepts(&after, Some(&file_str)))
            .cloned()
            .ok_or_else(|| format!("{file_str}: formato sin visor"))?;

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
        self.status = format!("Cargando celda {id} …");
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
            parts.push(format!("{} → {}", short_hash(&ctx.commit_a), short_hash(&ctx.commit_b)));
        }
        if let Some(p) = &self.selected_path {
            parts.push(p.file_name().unwrap_or_default().to_string_lossy().to_string());
        }
        if let Some(bs) = self.backend_state.as_ref().filter(|_| self.sch.is_none()) {
            if let Some(cell) = bs.scene.current_entry() {
                parts.push(cell.to_string());
            }
            if let LoadKind::Diff { tab, .. } = bs.kind {
                parts.push(tab_label(tab).to_string());
            }
        } else if let Some(sch) = self.sch.as_ref().filter(|s| s.diff.is_some()) {
            parts.push(tab_label(sch.tab).to_string());
        }
        parts
    }

    /// Pantalla inicial: qué es esto, cómo empezar, archivos recientes y
    /// atajos. Retorna el archivo reciente elegido, si hubo clic.
    fn empty_state(&self, ui: &mut egui::Ui) -> Option<PathBuf> {
        let mut picked = None;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.22).max(space::L));
            ui.label(RichText::new("Abre un diseño").size(22.0).strong());
            ui.add_space(space::XS);
            ui.label(RichText::new("Elige un .sch, .gds u .oas en el panel Proyecto, o arrastra un archivo a la ventana.").weak());
            ui.add_space(space::L);

            let recent: Vec<&String> = self.recent.iter().filter(|p| Path::new(p).is_file()).collect();
            if !recent.is_empty() {
                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::same(space::M as i8))
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);
                        ui.label(RichText::new("Recientes").strong());
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
                RichText::new("F encuadrar  ·  L etiquetas  ·  rueda para zoom  ·  arrastrar para mover")
                    .small()
                    .weak(),
            );
        });
        picked
    }

    /// Encuadrar todo, pedido por el usuario (se anima salvo movimiento reducido).
    fn request_fit(&mut self) {
        if let Some(sch) = self.sch.as_mut() {
            sch.needs_fit = true;
        } else if let Some(bs) = self.backend_state.as_mut() {
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
        let (fit, labels, zoom_in, zoom_out) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::L),
                i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals),
                i.key_pressed(egui::Key::Minus),
            )
        });
        if fit {
            self.request_fit();
        }
        if labels {
            self.show_labels = !self.show_labels;
            // Con el teclado no se ve el botón cambiar: confirmarlo.
            let msg = if self.show_labels { "Etiquetas visibles" } else { "Etiquetas ocultas (L para mostrar)" };
            self.notify(ToastKind::Info, msg);
        }
        if let Some(bs) = self.backend_state.as_mut().filter(|_| self.sch.is_none()) {
            let step = 1.25;
            let factor = if zoom_in { Some(step) } else if zoom_out { Some(1.0 / step) } else { None };
            if let Some(f) = factor {
                bs.pending_zoom = Some(bs.pending_zoom.unwrap_or(1.0) * f);
            }
        }
    }

    /// Pestaña actual si hay un diff cargado via backend.
    fn backend_diff_tab(&self) -> Option<DiffTab> {
        match &self.backend_state.as_ref()?.kind {
            LoadKind::Diff { tab, .. } if self.sch.is_none() => Some(*tab),
            _ => None,
        }
    }

    /// Selector de celdas bajo el panel izquierdo, si el archivo tiene más de una.
    fn show_entry_picker(&mut self, ui: &mut egui::Ui) {
        let picked = match &mut self.backend_state {
            Some(bs) if self.sch.is_none() && bs.scene.entries().len() > 1 => {
                ui.separator();
                render_entry_picker(ui, bs)
            }
            _ => None,
        };
        if let Some(id) = picked {
            self.select_entry(&id);
        }
    }

    /// Relee el archivo del disco conservando la sub-vista actual.
    fn reload_backend(&mut self) {
        let Some(bs) = &self.backend_state else { return };
        // En diff los bytes vienen de git, no del disco.
        if matches!(bs.kind, LoadKind::Diff { .. }) {
            return;
        }
        let path = PathBuf::from(&bs.path);
        let entry = bs.scene.current_entry().map(str::to_string);
        self.error = None;
        if self.load_via_backend(&path, entry) {
            self.status = format!("Recargando {} …", path.display());
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
                    LoadKind::Single => format!("Abierto ({name})"),
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
                    self.notify(ToastKind::Success, format!("{shown} cargado"));
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
                    self.fail(&format!("No se pudo abrir {name}"), friendly_error(&e));
                }
            }
        }
        self.pending_token = None;
    }

    fn load_sch(&mut self, path: &Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let scene = build_scene(&content)?;
        // El encuadre se hace al pintar, con el tamaño real del panel.
        let viewport = SchViewport::default();
        self.sch = Some(SchState { scene, scene_a: None, viewport, diff: None, needs_fit: true, tab: DiffTab::After });
        Ok(())
    }

    fn load_diff(&mut self, repo: &Path, commit_a: &str, commit_b: &str, file: &Path) -> Result<(), String> {
        let file_str = file.to_string_lossy();
        // El diff lo da el driver del registro, igual que en la CLI.
        let report = crate::core::analysis::commit_diff::analyze_diff(
            repo,
            commit_a,
            commit_b,
            &file_str,
            &crate::modules::registry(),
            &riku_kernel::DiffOptions::default(),
        )
            .map_err(|e| e.to_string())?;

        let opts = sch_render_opts();

        let sch_a = get_blob_content(repo, commit_a, &file_str)?;
        let parsed_a = xschem_viewer::parser::parse(&sch_a).map_err(|e| e.to_string())?;
        let scene_a = xschem_viewer::SceneBuilder::new(&opts).build(&parsed_a);

        let sch_content = get_blob_content(repo, commit_b, &file_str)?;
        let parsed = xschem_viewer::parser::parse(&sch_content).map_err(|e| e.to_string())?;
        let scene = xschem_viewer::SceneBuilder::new(&opts).build(&parsed);

        let viewport = SchViewport::default();

        self.selected_path = Some(file.to_path_buf());
        self.sch = Some(SchState { scene, scene_a: Some(scene_a), viewport, diff: Some(report), needs_fit: true, tab: DiffTab::Diff });
        Ok(())
    }
}

impl eframe::App for RikuGuiApp {
    /// Preferencias propias; el tema lo persiste egui junto a su memoria.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PREF_LABELS, &self.show_labels);
        eframe::set_value(storage, PREF_ALL_FILES, &self.show_all_files);
        eframe::set_value(storage, PREF_REDUCE_MOTION, &self.reduce_motion);
        eframe::set_value(storage, PREF_RECENT, &self.recent);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut reload = false;
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
        let cell = self.backend_state.as_ref().filter(|_| self.sch.is_none()).and_then(|bs| bs.scene.current_entry().map(str::to_string));
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

                let has_view = self.sch.is_some() || self.backend_state.is_some();
                if ui
                    .add_enabled(has_view, egui::Button::new("Encuadrar"))
                    .on_hover_text("Ajustar la vista para ver todo el diseño  (F)")
                    .clicked()
                {
                    self.request_fit();
                }
                if ui
                    .button("Recargar")
                    .on_hover_text("Volver a leer el archivo y el árbol de proyecto desde el disco")
                    .clicked()
                {
                    self.refresh_tree();
                    // Un GDS abierto se relee del disco en la misma celda.
                    if self.sch.is_none() {
                        self.reload_backend();
                    }
                }
                ui.separator();
                ui.toggle_value(&mut self.show_labels, "Etiquetas")
                    .on_hover_text("Mostrar u ocultar los textos del layout (pines, nombres)  (L)");

                if let Some(sch) = self.sch.as_mut() {
                    ui.separator();
                    let mut scale = sch.viewport.scale as f32;
                    if ui.add(egui::Slider::new(&mut scale, 0.1..=20.0).text("Zoom")).changed() {
                        sch.viewport.scale = scale as f64;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Lo menos frecuente, un nivel más abajo.
                    ui.menu_button("Ajustes", |ui| {
                        ui.checkbox(&mut self.reduce_motion, "Reducir movimiento")
                            .on_hover_text("Sin animaciones al encuadrar ni inercia al soltar un arrastre");
                        ui.separator();
                        ui.label(RichText::new("Atajos").strong());
                        for (k, what) in [
                            ("F", "Encuadrar"),
                            ("L", "Etiquetas"),
                            ("+ / −", "Acercar / alejar"),
                            ("Rueda", "Zoom en el cursor"),
                            ("Arrastrar", "Mover (suelta con impulso)"),
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
                        .filter(|_| self.sch.is_none())
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
                        ui.label(RichText::new(format!("{} etiquetas ocultas por solaparse — acerca el zoom", self.labels_hidden)).weak());
                    }
                });
            });
        });

        egui::Panel::left("left_panel")
            .resizable(true)
            .default_size(200.0)
            .show_inside(ui, |ui| {
                // Modo diff: mostrar solo selector de vistas (Diff/Before/After)
                if let (Some(sch), Some(ctx)) = (self.sch.as_mut(), self.diff_ctx.as_ref()) {
                    ui.heading("Vistas");
                    ui.label(RichText::new(ctx.file.file_name()
                        .unwrap_or_default().to_string_lossy().as_ref())
                        .color(egui::Color32::from_gray(180)));
                    ui.label(RichText::new(format!("{} → {}",
                        short_hash(&ctx.commit_a), short_hash(&ctx.commit_b)))
                        .small().color(egui::Color32::from_gray(140)));
                    ui.separator();
                    view_selector(ui, &mut sch.tab, DiffTab::Diff, "Diff");
                    view_selector(ui, &mut sch.tab, DiffTab::Before, "Before");
                    view_selector(ui, &mut sch.tab, DiffTab::After, "After");
                } else if let (Some(ctx), Some(current)) = (self.diff_ctx.as_ref(), self.backend_diff_tab()) {
                    // Diff via backend (GDS): mismas vistas; cada una es otra carga.
                    ui.heading("Vistas");
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
                } else {
                    // Modo archivo único: árbol de proyecto
                    ui.heading("Proyecto");
                    let root = self.project_root.display().to_string();
                    ui.add(egui::Label::new(RichText::new(&root).small().weak()).truncate())
                        .on_hover_text(&root);
                    if ui
                        .checkbox(&mut self.show_all_files, "Todos los archivos")
                        .on_hover_text("Sin marcar: solo .sch, .sym, .gds y .oas (lo que se puede abrir)")
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
                    let has_picker = self.sch.is_none()
                        && self.backend_state.as_ref().is_some_and(|bs| bs.scene.entries().len() > 1);
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
                ui.heading("Detalles");
                if let Some(path) = &self.selected_path {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    ui.add(egui::Label::new(RichText::new(&name).strong()).truncate())
                        .on_hover_text(path.display().to_string());
                }
                ui.add_space(space::XS);

                if self.sch.is_none() {
                    if let Some(bs) = &mut self.backend_state {
                        render_backend_details(ui, bs);
                    } else {
                        ui.label(RichText::new("Nada abierto todavía.").weak());
                    }
                }

                if let Some(sch) = &self.sch {
                    section(ui, "sch_summary", "Resumen", None, true, |ui| {
                        egui::Grid::new("sch_meta").num_columns(2).spacing([space::M, space::XS]).show(ui, |ui| {
                            ui.label(RichText::new("Elementos").weak());
                            ui.monospace(sch.scene.elements.len().to_string());
                            ui.end_row();
                            ui.label(RichText::new("Wires").weak());
                            ui.monospace(sch.scene.wires.len().to_string());
                            ui.end_row();
                        });
                    });

                    if !sch.scene.missing_symbols.is_empty() {
                        let n = sch.scene.missing_symbols.len();
                        section(ui, "sch_missing", "Símbolos sin resolver", Some(n), true, |ui| {
                            ui.label(
                                RichText::new("Se dibujan como marcadores rojos. Revisa las rutas de símbolos (riku doctor).")
                                    .small()
                                    .color(ui.visuals().warn_fg_color),
                            );
                            for s in &sch.scene.missing_symbols {
                                ui.add(egui::Label::new(RichText::new(s).small().monospace()).truncate());
                            }
                            if ui.button("Recargar").on_hover_text("Volver a resolver los símbolos").clicked() {
                                reload = true;
                            }
                        });
                    }

                    if let Some(diff) = &sch.diff {
                        let n = diff.changes.iter().filter(|c| !matches!(c.element, crate::core::domain::models::Element::Whole)).count();
                        section(ui, "sch_changes", "Cambios", Some(n), true, |ui| render_change_list(ui, diff));
                    }
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

            // Path neutro: escena cargada via ViewerBackend.
            if self.sch.is_none() {
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
                    let opts = PaintOptions { theme: CanvasTheme::from_visuals(ui.visuals()), labels: self.show_labels };
                    let stats = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(response.rect), |ui| {
                            paint_scene(ui, bs.scene.as_ref(), &bs.viewport, &hidden, opts)
                        })
                        .inner;
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

            if let Some(sch) = &mut self.sch {
                let available = ui.available_size_before_wrap();
                let response = ui.allocate_response(available, egui::Sense::drag());
                canvas_cursor(&ctx, &response);

                if response.dragged() {
                    let delta = response.drag_delta();
                    sch.viewport.pan_x -= delta.x as f64 / sch.viewport.scale;
                    sch.viewport.pan_y -= delta.y as f64 / sch.viewport.scale;
                    ctx.request_repaint();
                }

                let scroll = ctx.input(|i| i.smooth_scroll_delta.y as f64);
                if scroll.abs() > f64::EPSILON && response.hovered() {
                    sch.viewport.scale = (sch.viewport.scale * (1.0 + scroll * 0.002)).clamp(0.01, 100.0);
                    ctx.request_repaint();
                }
                // Encuadre con el tamaño real del lienzo (antes se usaba un
                // rect ficticio de 800×600 y el esquemático quedaba chico y
                // descentrado).
                if sch.needs_fit && response.rect.width() > 0.0 && response.rect.height() > 0.0 {
                    fit_viewport_to_scene(&mut sch.viewport, &sch.scene, response.rect);
                    sch.needs_fit = false;
                }

                ui.scope_builder(egui::UiBuilder::new().max_rect(response.rect), |ui| {
                    // Determinar qué se pinta según el tab activo
                    let in_diff_mode = sch.diff.is_some() && sch.scene_a.is_some();
                    if in_diff_mode {
                        match sch.tab {
                            DiffTab::Before => {
                                // Solo commit A, sin anotaciones ni fantasmas
                                if let Some(scene_a) = sch.scene_a.as_ref() {
                                    paint_sch(ui, scene_a, None, &sch.viewport, None);
                                }
                            }
                            DiffTab::After => {
                                // Solo commit B, sin anotaciones ni fantasmas
                                paint_sch(ui, &sch.scene, None, &sch.viewport, None);
                            }
                            DiffTab::Diff => {
                                paint_sch(ui, &sch.scene, sch.scene_a.as_ref(), &sch.viewport, sch.diff.as_ref());
                            }
                        }
                    } else {
                        paint_sch(ui, &sch.scene, None, &sch.viewport, None);
                    }
                });
            } else {
                if self.pending_load.is_some() {
                    ui.centered_and_justified(|ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Cargando…");
                        });
                    });
                } else if let Some(path) = self.empty_state(ui) {
                    self.open_path(&path);
                }
            }
        });

        if reload {
            // Si estamos en modo diff, recargar el diff completo (preserva tabs y contexto)
            if let Some(ctx) = self.diff_ctx.as_ref().map(|c| DiffContext {
                repo: c.repo.clone(),
                commit_a: c.commit_a.clone(),
                commit_b: c.commit_b.clone(),
                file: c.file.clone(),
            }) {
                if let Err(e) = self.load_diff(&ctx.repo, &ctx.commit_a, &ctx.commit_b, &ctx.file) {
                    self.fail("No se pudo recargar el diff", e);
                }
            } else if let Some(path) = self.selected_path.clone() {
                if let Err(e) = self.load_sch(&path) {
                    self.fail("No se pudo recargar", e);
                }
            }
        }

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
        "Suelta para abrir (.sch, .sym, .gds, .oas)",
        egui::FontId::proportional(20.0),
        v.strong_text_color(),
    );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn sch_render_opts() -> xschem_viewer::RenderOptions {
    let mut opts = xschem_viewer::RenderOptions::dark().with_sym_paths_from_xschemrc();
    if let (Ok(root), Ok(pdk)) = (std::env::var("PDK_ROOT"), std::env::var("PDK")) {
        let p = std::path::Path::new(&root).join(&pdk).join("libs.tech/xschem");
        if p.exists() { opts = opts.with_sym_path(p.to_string_lossy().to_string()); }
    }
    opts
}

fn build_scene(content: &str) -> Result<xschem_viewer::ResolvedScene, String> {
    let opts = sch_render_opts();
    let parsed = xschem_viewer::parser::parse(content).map_err(|e| e.to_string())?;
    Ok(xschem_viewer::SceneBuilder::new(&opts).build(&parsed))
}


fn get_blob_content(repo: &Path, commit: &str, file_path: &str) -> Result<String, String> {
    use crate::core::domain::ports::GitRepository;
    use crate::core::git::git_service::GitService;
    let svc = GitService::open(repo).map_err(|e| e.to_string())?;
    let bytes = svc.get_blob(commit, file_path).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn is_sch_renderable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("sch"))
        .unwrap_or(false)
}

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
                    (true, _) => "Cosmético: bajo el umbral de relevancia",
                    (false, true) => "Clic para ir al cambio",
                    (false, false) => "Sin ubicación en esta celda",
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
        section(ui, "summary", "Resumen", None, true, |ui| {
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

    let changes = scene.changes();
    if !changes.is_empty() {
        let mut picked = None;
        section(ui, "changes", "Cambios", Some(changes.len()), true, |ui| {
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
    section(ui, "layers", "Capas", Some(title_count), true, |ui| {
        ui.horizontal(|ui| {
            if ui.small_button("Mostrar todas").clicked() {
                bs.hidden_layers.clear();
            }
            if ui.small_button("Ocultar todas").clicked() {
                bs.hidden_layers.extend(layers.iter().map(|(_, p)| p.name.clone()));
            }
            if shown < layers.len() {
                ui.label(RichText::new(format!("{} ocultas", layers.len() - shown)).small().weak());
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
        DiffTab::Diff => "Después, con lo añadido en verde y lo eliminado en rojo",
        DiffTab::Before => "Versión anterior (commit A)",
        DiffTab::After => "Versión nueva (commit B)",
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
        (T::System, "Sistema", "Seguir el tema del sistema operativo"),
        (T::Dark, "Oscuro", "Fondo oscuro (como KLayout)"),
        (T::Light, "Claro", "Fondo claro, para ambientes iluminados o para imprimir capturas"),
    ] {
        if ui.selectable_label(current == pref, label).on_hover_text(tip).clicked() {
            ui.ctx().set_theme(pref);
        }
    }
    ui.label(RichText::new("Tema:").weak());
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
    s.chars().take(7).collect()
}

// ─── Panel de cambios ─────────────────────────────────────────────────────────

const COLOR_ADDED: egui::Color32 = egui::Color32::from_rgb(0, 200, 0);
const COLOR_REMOVED: egui::Color32 = egui::Color32::from_rgb(200, 0, 0);
const COLOR_MODIFIED: egui::Color32 = egui::Color32::from_rgb(255, 180, 0);
const COLOR_MOVED: egui::Color32 = egui::Color32::from_rgb(0, 190, 255);

fn render_change_list(ui: &mut egui::Ui, diff: &crate::core::domain::models::FileChange) {
    use crate::core::domain::models::{ChangeKind, Element};

    let mut any_shown = false;

    for c in &diff.changes {
        let Element::Component { name } = &c.element else { continue };
        let name = match (&c.renamed_from, c.kind) {
            (Some(from), ChangeKind::Renamed) => format!("{from} → {name}"),
            _ => name.clone(),
        };
        // Solo-cosmético sin posición cambiada lo omitimos (es "Move All" global)
        if c.cosmetic && !c.position_changed { continue; }

        let moved_only = matches!(c.kind, ChangeKind::Modified | ChangeKind::Renamed) && c.cosmetic && c.position_changed;

        let (prefix, main_color, extra_color) = match c.kind {
            ChangeKind::Added   => ("+", COLOR_ADDED, None),
            ChangeKind::Removed => ("-", COLOR_REMOVED, None),
            ChangeKind::Modified if moved_only =>
                ("↦", COLOR_MOVED, None),
            ChangeKind::Modified if c.position_changed =>
                ("~", COLOR_MODIFIED, Some(COLOR_MOVED)),
            ChangeKind::Modified =>
                ("~", COLOR_MODIFIED, None),
            ChangeKind::Renamed =>
                ("r", COLOR_MODIFIED, c.position_changed.then_some(COLOR_MOVED)),
        };

        ui.horizontal(|ui| {
            // Chip de color a la izquierda
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, main_color);
            if let Some(extra) = extra_color {
                ui.painter().rect_stroke(
                    rect.expand(1.0),
                    2.0,
                    egui::Stroke::new(2.0_f32, extra),
                    egui::StrokeKind::Outside,
                );
            }
            ui.colored_label(main_color, format!("{prefix} {name}"));
        });
        any_shown = true;
    }

    for c in &diff.changes {
        match (&c.element, c.kind) {
            (Element::Net { name }, ChangeKind::Added) => ui.colored_label(COLOR_ADDED, format!("+ net:{name}")),
            (Element::Net { name }, ChangeKind::Removed) => ui.colored_label(COLOR_REMOVED, format!("- net:{name}")),
            _ => continue,
        };
        any_shown = true;
    }

    if !any_shown {
        ui.label(egui::RichText::new("Sin cambios semánticos").italics().color(egui::Color32::from_gray(140)));
    }

    ui.separator();
    ui.label(egui::RichText::new("Leyenda").small().color(egui::Color32::from_gray(160)));
    legend_row(ui, COLOR_ADDED, "+", "Añadido");
    legend_row(ui, COLOR_REMOVED, "-", "Removido");
    legend_row(ui, COLOR_MODIFIED, "~", "Modificado");
    legend_row(ui, COLOR_MOVED, "↦", "Trasladado");
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, COLOR_MODIFIED);
        ui.painter().rect_stroke(rect.expand(1.0), 2.0, egui::Stroke::new(2.0_f32, COLOR_MOVED), egui::StrokeKind::Outside);
        ui.small("Modificado + trasladado");
    });
}

fn legend_row(ui: &mut egui::Ui, color: egui::Color32, prefix: &str, label: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, color);
        ui.small(format!("{prefix} {label}"));
    });
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
        Some("no es un GDSII válido o está dañado")
    } else if e.contains("no existe") || e.contains("No such file") {
        Some("el archivo ya no existe")
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
        assert!(f.starts_with("no es un GDSII válido"), "{f}");
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
