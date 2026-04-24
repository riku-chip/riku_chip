use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use crate::launch::LaunchArgs;
use crate::project::ProjectEntry;
use crate::sch_painter::{SchViewport, paint_sch};

// ─── Estado del schematic ─────────────────────────────────────────────────────

struct SchState {
    scene: xschem_viewer::ResolvedScene,
    /// Escena del commit A para mostrar fantasmas de componentes movidos/eliminados
    scene_a: Option<xschem_viewer::ResolvedScene>,
    viewport: SchViewport,
    diff: Option<riku::core::models::DiffReport>,
}

// ─── App ──────────────────────────────────────────────────────────────────────

pub struct RikuGuiApp {
    project_root: PathBuf,
    project_tree: ProjectEntry,
    selected_path: Option<PathBuf>,
    sch: Option<SchState>,
    status: String,
    error: Option<String>,
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
        let mut app = Self {
            project_root,
            project_tree,
            selected_path,
            sch: None,
            status: String::from("Ready"),
            error: None,
        };

        // Modo diff: commits pasados desde el CLI
        if let (Some(file), Some(ca), Some(cb)) = (&launch.file, &launch.commit_a, &launch.commit_b) {
            let repo = launch.repo.as_deref().unwrap_or(Path::new("."));
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

        if is_sch_renderable(path) {
            match self.load_sch(path) {
                Ok(()) => self.status = format!("Loaded {}", path.display()),
                Err(e) => { self.error = Some(e.clone()); self.status = "Error".to_string(); }
            }
        } else {
            self.status = format!("{} — formato no soportado aún", path.display());
        }
    }

    fn load_sch(&mut self, path: &Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let scene = build_scene(&content)?;
        let mut viewport = SchViewport::default();
        let dummy = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        viewport.fit_to(&scene, dummy);
        self.sch = Some(SchState { scene, scene_a: None, viewport, diff: None });
        Ok(())
    }

    fn load_diff(&mut self, repo: &Path, commit_a: &str, commit_b: &str, file: &Path) -> Result<(), String> {
        use riku::adapters::xschem_driver::XschemDriver;
        use riku::core::diff_view::DiffView;
        use riku::parsers::xschem::parse;

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
        viewport.fit_to(&scene, dummy);

        self.selected_path = Some(file.to_path_buf());
        self.sch = Some(SchState { scene, scene_a: Some(scene_a), viewport, diff: Some(view.report) });
        Ok(())
    }
}

impl eframe::App for RikuGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut reload = false;

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
                        sch.viewport.fit_to(&sch.scene, dummy);
                    }
                    let mut scale = sch.viewport.scale as f32;
                    if ui.add(egui::Slider::new(&mut scale, 0.1..=20.0).text("Zoom")).changed() {
                        sch.viewport.scale = scale as f64;
                    }
                }
            });
        });

        egui::Panel::left("project_tree")
            .resizable(true)
            .default_size(240.0)
            .show_inside(ui, |ui| {
                ui.heading("Project");
                ui.label(self.project_root.display().to_string());
                ui.separator();
                let tree = self.project_tree.clone();
                let selected_path = self.selected_path.clone();
                let mut open_path = |path: &Path| self.open_path(path);
                show_entry_tree(ui, &tree, selected_path.as_deref(), &mut open_path);
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
                    paint_sch(ui, &sch.scene, sch.scene_a.as_ref(), &sch.viewport, sch.diff.as_ref());
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Selecciona un archivo .sch del árbol de proyecto.");
                });
            }
        });

        if reload {
            if let Some(path) = self.selected_path.clone() {
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
