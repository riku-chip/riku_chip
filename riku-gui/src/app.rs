use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use crate::project::ProjectEntry;
use crate::sch_painter::{SchViewport, paint_sch};

// ─── Estado del schematic abierto ────────────────────────────────────────────

struct SchState {
    scene: xschem_viewer::ResolvedScene,
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
    pub fn new(_cc: &eframe::CreationContext<'_>, initial_path: Option<PathBuf>) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let (project_root, selected_path) = match initial_path {
            Some(path) if path.is_file() => {
                let root = path.parent().map(Path::to_path_buf).unwrap_or_else(|| cwd.clone());
                (root, Some(path))
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

        if let Some(path) = app.selected_path.clone() {
            app.open_path(&path);
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
                Err(e) => {
                    self.error = Some(e.clone());
                    self.status = format!("Error: {}", path.display());
                }
            }
        } else {
            self.status = format!("{} — formato no soportado aún", path.display());
        }
    }

    fn load_sch(&mut self, path: &Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;

        let mut opts = xschem_viewer::RenderOptions::dark().with_sym_paths_from_xschemrc();
        if let (Ok(root), Ok(pdk)) = (std::env::var("PDK_ROOT"), std::env::var("PDK")) {
            let pdk_path = std::path::Path::new(&root).join(&pdk).join("libs.tech/xschem");
            if pdk_path.exists() {
                opts = opts.with_sym_path(pdk_path.to_string_lossy().to_string());
            }
        }

        let parsed = xschem_viewer::parser::parse(&content).map_err(|e| e.to_string())?;
        let scene = xschem_viewer::SceneBuilder::new(&opts).build(&parsed);

        let mut viewport = SchViewport::default();
        let dummy_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        viewport.fit_to(&scene, dummy_rect);

        self.sch = Some(SchState { scene, viewport, diff: None });
        Ok(())
    }
}

impl eframe::App for RikuGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut reload = false;

        // ── Top bar ───────────────────────────────────────────────────────────
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
                        let dummy = egui::Rect::from_min_size(
                            egui::Pos2::ZERO, egui::vec2(800.0, 600.0),
                        );
                        sch.viewport.fit_to(&sch.scene, dummy);
                    }
                    let mut scale = sch.viewport.scale as f32;
                    if ui.add(egui::Slider::new(&mut scale, 0.1..=20.0).text("Zoom")).changed() {
                        sch.viewport.scale = scale as f64;
                    }
                }
            });
        });

        // ── Panel izquierdo: árbol de proyecto ────────────────────────────────
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

        // ── Panel derecho: info + diff ────────────────────────────────────────
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
                        for c in diff.components.iter().filter(|c| !c.cosmetic) {
                            let (label, color) = match c.kind {
                                riku::core::models::ChangeKind::Added =>
                                    (format!("+ {}", c.name), egui::Color32::from_rgb(0, 200, 0)),
                                riku::core::models::ChangeKind::Removed =>
                                    (format!("- {}", c.name), egui::Color32::from_rgb(200, 0, 0)),
                                riku::core::models::ChangeKind::Modified =>
                                    (format!("~ {}", c.name), egui::Color32::from_rgb(255, 180, 0)),
                            };
                            ui.colored_label(color, label);
                        }
                        for net in &diff.nets_added {
                            ui.colored_label(egui::Color32::from_rgb(0, 200, 0), format!("+ net:{net}"));
                        }
                        for net in &diff.nets_removed {
                            ui.colored_label(egui::Color32::from_rgb(200, 0, 0), format!("- net:{net}"));
                        }
                    }
                }

                if let Some(error) = &self.error {
                    ui.separator();
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
                }
            });

        // ── Canvas central ────────────────────────────────────────────────────
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
                    paint_sch(ui, &sch.scene, &sch.viewport, sch.diff.as_ref());
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

fn is_sch_renderable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("sch"))
        .unwrap_or(false)
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
