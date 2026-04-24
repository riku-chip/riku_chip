use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, RichText};
use poll_promise::Promise;
use tokio::runtime::Runtime;
use viewer_core::{
    backend::ViewerBackend, bbox::BoundingBox as VcBBox, scene::SceneHandle,
    viewport::Viewport as VcViewport, CancellationToken,
};

use crate::launch::LaunchArgs;
use crate::project::ProjectEntry;
use crate::sch_painter::{SchViewport, fit_viewport_to_scene, paint_sch};
use crate::scene_painter::paint_scene;

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
    diff: Option<riku::core::models::DiffReport>,
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
    backend_name: &'static str,
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
    /// Escena actual cargada via backend (path neutro).
    backend_state: Option<BackendState>,
    /// Carga async en vuelo (solo una — al llegar una nueva se cancela).
    pending_load: Option<Promise<Result<BackendState, String>>>,
    /// Token de cancelación de la carga en vuelo.
    pending_token: Option<CancellationToken>,
}

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
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let (project_root, selected_path): (PathBuf, Option<PathBuf>) = match &launch.file {
            Some(path) if path.is_file() => {
                let root = path.parent().map(Path::to_path_buf).unwrap_or_else(|| cwd.clone());
                (root, Some(path.clone()))
            }
            Some(path) if path.is_dir() => (path.clone(), None),
            _ => (cwd.clone(), None),
        };

        let project_tree = ProjectEntry::build(&project_root);

        // Runtime multi-hilo: spawn_blocking (parseo pesado) no bloquea al
        // scheduler principal. Dos workers son suficientes para una GUI.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");
        let runtime = Arc::new(runtime);

        // Registry de backends. XschemBackend siempre presente; otros se
        // agregarán cuando los crates estén disponibles (gds-renderer, ...).
        let backends: Vec<Arc<dyn ViewerBackend>> = vec![
            Arc::new(xschem_viewer::XschemBackend::new()),
        ];

        let mut app = Self {
            project_root,
            project_tree,
            selected_path,
            sch: None,
            diff_ctx: None,
            status: String::from("Ready"),
            error: None,
            runtime,
            backends,
            backend_state: None,
            pending_load: None,
            pending_token: None,
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
            match app.load_diff(repo, ca, cb, file) {
                Ok(()) => app.status = format!("Diff {} → {}", ca, cb),
                Err(e) => { app.error = Some(e.clone()); app.status = "Error en diff".to_string(); }
            }
        } else if let Some(path) = app.selected_path.clone() {
            if is_sch_renderable(&path) {
                match app.load_sch(&path) {
                    Ok(()) => app.status = format!("Loaded {}", path.display()),
                    Err(e) => { app.error = Some(e.clone()); app.status = "Error".to_string(); }
                }
            }
        }

        app
    }

    fn refresh_tree(&mut self) {
        self.project_tree = ProjectEntry::build(&self.project_root);
    }

    fn open_path(&mut self, path: &Path) {
        self.selected_path = Some(path.to_path_buf());
        self.error = None;
        self.sch = None;
        self.backend_state = None;

        // .sch sigue por la ruta rica (diff semántico, fantasmas, etc.)
        if is_sch_renderable(path) {
            match self.load_sch(path) {
                Ok(()) => self.status = format!("Loaded {}", path.display()),
                Err(e) => { self.error = Some(e.clone()); self.status = "Error".to_string(); }
            }
            return;
        }

        // Fallback: intentar con algún backend registrado (ruta neutra async).
        if self.load_via_backend(path) {
            self.status = format!("Cargando {} …", path.display());
        } else {
            self.status = format!("{} — formato no soportado aún", path.display());
        }
    }

    /// Intenta cargar `path` via alguno de los backends registrados.
    /// Retorna `true` si algún backend aceptó el archivo (la carga queda en vuelo).
    fn load_via_backend(&mut self, path: &Path) -> bool {
        let content = match std::fs::read(path) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("read: {e}"));
                return false;
            }
        };
        let path_str = path.to_string_lossy().to_string();

        let backend = self.backends.iter()
            .find(|b| b.accepts(&content, Some(&path_str)))
            .cloned();
        let Some(backend) = backend else { return false };

        // Cancelar carga previa si había.
        if let Some(tok) = self.pending_token.take() {
            tok.cancel();
        }
        let token = CancellationToken::new();
        self.pending_token = Some(token.clone());

        let backend_name = backend.info().name;
        let _guard = self.runtime.enter();
        let fut = async move {
            backend.load(content, Some(path_str), token).await
                .map(|scene| BackendState {
                    scene,
                    viewport: VcViewport::default(),
                    backend_name,
                })
                .map_err(|e| e.to_string())
        };
        self.pending_load = Some(Promise::spawn_async(fut));
        true
    }

    /// Drenar el promise si está listo. Se llama desde `ui()`.
    fn poll_pending_load(&mut self) {
        let ready = self.pending_load.as_ref().and_then(|p| p.ready()).is_some();
        if !ready { return; }
        let Some(promise) = self.pending_load.take() else { return };
        match promise.block_and_take() {
            Ok(state) => {
                self.status = format!("Loaded via {}", state.backend_name);
                self.backend_state = Some(state);
            }
            Err(e) => {
                // Ignoramos errores de cancelación — vienen de nosotros mismos
                // al abrir otro archivo antes de que terminara la carga previa.
                if !e.contains("cancelled") {
                    self.error = Some(e);
                    self.status = "Error".to_string();
                }
            }
        }
        self.pending_token = None;
    }

    fn load_sch(&mut self, path: &Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let scene = build_scene(&content)?;
        let mut viewport = SchViewport::default();
        let dummy = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        fit_viewport_to_scene(&mut viewport, &scene, dummy);
        self.sch = Some(SchState { scene, scene_a: None, viewport, diff: None, tab: DiffTab::After });
        Ok(())
    }

    fn load_diff(&mut self, repo: &Path, commit_a: &str, commit_b: &str, file: &Path) -> Result<(), String> {
        use riku::adapters::xschem_driver::XschemDriver;
        use riku::core::diff_view::DiffView;
        use riku::adapters::xschem_driver::parse;

        let file_str = file.to_string_lossy();
        let driver = XschemDriver::new();
        let view = DiffView::from_commits(repo, commit_a, commit_b, &file_str, &driver, |b| parse(b))
            .map_err(|e| e.to_string())?;

        let opts = sch_render_opts();

        let sch_a = get_blob_content(repo, commit_a, &file_str)?;
        let parsed_a = xschem_viewer::parser::parse(&sch_a).map_err(|e| e.to_string())?;
        let scene_a = xschem_viewer::SceneBuilder::new(&opts).build(&parsed_a);

        let sch_content = get_blob_content(repo, commit_b, &file_str)?;
        let parsed = xschem_viewer::parser::parse(&sch_content).map_err(|e| e.to_string())?;
        let scene = xschem_viewer::SceneBuilder::new(&opts).build(&parsed);

        let mut viewport = SchViewport::default();
        let dummy = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        fit_viewport_to_scene(&mut viewport, &scene, dummy);

        self.selected_path = Some(file.to_path_buf());
        self.sch = Some(SchState { scene, scene_a: Some(scene_a), viewport, diff: Some(view.report), tab: DiffTab::Diff });
        Ok(())
    }
}

impl eframe::App for RikuGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut reload = false;

        // Drenar carga async antes de pintar; si sigue en vuelo, solicitar
        // repaint para que el promise se consulte en el siguiente frame.
        self.poll_pending_load();
        if self.pending_load.is_some() {
            ctx.request_repaint();
        }

        egui::Panel::top("top_bar").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Riku").strong());
                ui.separator();
                ui.label(&self.status);

                if ui.button("Refresh").clicked() {
                    self.refresh_tree();
                }

                if let Some(sch) = self.sch.as_mut() {
                    ui.separator();
                    if ui.button("Fit").clicked() {
                        let dummy = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
                        fit_viewport_to_scene(&mut sch.viewport, &sch.scene, dummy);
                    }
                    let mut scale = sch.viewport.scale as f32;
                    if ui.add(egui::Slider::new(&mut scale, 0.1..=20.0).text("Zoom")).changed() {
                        sch.viewport.scale = scale as f64;
                    }
                }
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
                } else {
                    // Modo archivo único: árbol de proyecto
                    ui.heading("Project");
                    ui.label(self.project_root.display().to_string());
                    ui.separator();
                    let tree = self.project_tree.clone();
                    let selected_path = self.selected_path.clone();
                    let mut open_path = |path: &Path| self.open_path(path);
                    show_entry_tree(ui, &tree, selected_path.as_deref(), &mut open_path);
                }
            });

        egui::Panel::right("info_panel")
            .resizable(true)
            .default_size(220.0)
            .show_inside(ui, |ui| {
                ui.heading("Details");
                ui.separator();

                if let Some(path) = &self.selected_path {
                    ui.label(path.file_name().unwrap_or_default().to_string_lossy().as_ref());
                }

                if let Some(sch) = &self.sch {
                    ui.label(format!("Elementos: {}", sch.scene.elements.len()));
                    ui.label(format!("Wires: {}", sch.scene.wires.len()));

                    if !sch.scene.missing_symbols.is_empty() {
                        ui.separator();
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 160, 60),
                            format!("Sin resolver: {}", sch.scene.missing_symbols.len()),
                        );
                        for s in &sch.scene.missing_symbols {
                            ui.small(s);
                        }
                        if ui.button("Recargar").clicked() {
                            reload = true;
                        }
                    }

                    if let Some(diff) = &sch.diff {
                        ui.separator();
                        ui.label(RichText::new("Cambios").strong());
                        render_change_list(ui, diff);
                    }
                }

                if let Some(error) = &self.error {
                    ui.separator();
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
                }
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            // Path neutro: escena cargada via ViewerBackend.
            if self.sch.is_none() {
                if let Some(bs) = &mut self.backend_state {
                    let available = ui.available_size_before_wrap();
                    let response = ui.allocate_response(available, egui::Sense::drag());
                    if response.dragged() {
                        let delta = response.drag_delta();
                        bs.viewport.pan_by_screen(delta.x as f64, delta.y as f64);
                        ctx.request_repaint();
                    }
                    let scroll = ctx.input(|i| i.smooth_scroll_delta.y as f64);
                    if scroll.abs() > f64::EPSILON && response.hovered() {
                        let (cx, cy) = (response.rect.center().x as f64, response.rect.center().y as f64);
                        bs.viewport.zoom_at(1.0 + scroll * 0.002, cx, cy);
                        ctx.request_repaint();
                    }
                    // Auto-fit la primera vez que se pinta.
                    if bs.viewport.scale == 1.0 && bs.viewport.pan_x == 0.0 && bs.viewport.pan_y == 0.0 {
                        bs.viewport.fit_to(
                            &bs.scene.bbox(),
                            response.rect.width() as f64,
                            response.rect.height() as f64,
                        );
                    }
                    ui.scope_builder(egui::UiBuilder::new().max_rect(response.rect), |ui| {
                        paint_scene(ui, bs.scene.as_ref(), &bs.viewport);
                    });
                    return;
                }
            }

            if let Some(sch) = &mut self.sch {
                let available = ui.available_size_before_wrap();
                let response = ui.allocate_response(available, egui::Sense::drag());

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
                ui.centered_and_justified(|ui| {
                    if self.pending_load.is_some() {
                        ui.label("Cargando…");
                    } else {
                        ui.label("Selecciona un archivo del árbol de proyecto.");
                    }
                });
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
                    self.error = Some(e);
                }
            } else if let Some(path) = self.selected_path.clone() {
                if let Err(e) = self.load_sch(&path) {
                    self.error = Some(e);
                }
            }
        }
    }
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
    use riku::core::git_service::GitService;
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

// ─── Selector de vistas (modo diff) ──────────────────────────────────────────

fn view_selector(ui: &mut egui::Ui, current: &mut DiffTab, this: DiffTab, label: &str) {
    let selected = *current == this;
    let text = if selected {
        RichText::new(format!("● {label}")).strong().color(egui::Color32::from_rgb(0, 190, 255))
    } else {
        RichText::new(format!("○ {label}")).color(egui::Color32::from_gray(180))
    };
    if ui.add(egui::Label::new(text).sense(egui::Sense::click())).clicked() {
        *current = this;
    }
}

fn short_hash(s: &str) -> String {
    s.chars().take(7).collect()
}

// ─── Panel de cambios ─────────────────────────────────────────────────────────

const COLOR_ADDED: egui::Color32 = egui::Color32::from_rgb(0, 200, 0);
const COLOR_REMOVED: egui::Color32 = egui::Color32::from_rgb(200, 0, 0);
const COLOR_MODIFIED: egui::Color32 = egui::Color32::from_rgb(255, 180, 0);
const COLOR_MOVED: egui::Color32 = egui::Color32::from_rgb(0, 190, 255);

fn render_change_list(ui: &mut egui::Ui, diff: &riku::core::models::DiffReport) {
    use riku::core::models::ChangeKind;

    let mut any_shown = false;

    for c in &diff.components {
        // Solo-cosmético sin posición cambiada lo omitimos (es "Move All" global)
        if c.cosmetic && !c.position_changed { continue; }

        let moved_only = matches!(c.kind, ChangeKind::Modified) && c.cosmetic && c.position_changed;

        let (prefix, main_color, extra_color) = match c.kind {
            ChangeKind::Added   => ("+", COLOR_ADDED, None),
            ChangeKind::Removed => ("-", COLOR_REMOVED, None),
            ChangeKind::Modified if moved_only =>
                ("↦", COLOR_MOVED, None),
            ChangeKind::Modified if c.position_changed =>
                ("~", COLOR_MODIFIED, Some(COLOR_MOVED)),
            ChangeKind::Modified =>
                ("~", COLOR_MODIFIED, None),
        };

        ui.horizontal(|ui| {
            // Chip de color a la izquierda
            let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, main_color);
            if let Some(extra) = extra_color {
                ui.painter().rect_stroke(
                    rect.expand(1.0),
                    2.0,
                    egui::Stroke::new(2.0, extra),
                    egui::StrokeKind::Outside,
                );
            }
            ui.colored_label(main_color, format!("{prefix} {}", c.name));
        });
        any_shown = true;
    }

    for net in &diff.nets_added {
        ui.colored_label(COLOR_ADDED, format!("+ net:{net}"));
        any_shown = true;
    }
    for net in &diff.nets_removed {
        ui.colored_label(COLOR_REMOVED, format!("- net:{net}"));
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
        ui.painter().rect_stroke(rect.expand(1.0), 2.0, egui::Stroke::new(2.0, COLOR_MOVED), egui::StrokeKind::Outside);
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
