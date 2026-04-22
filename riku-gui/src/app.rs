use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use gds_renderer::{RenderConfig, RenderScene, scene_from_cell};
use gdstk_rs::Library;

use crate::project::{ProjectEntry, is_gds_renderable};
use crate::scene_painter::paint_scene;
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
    // GDS
    selected_cell: usize,
    cell_names: Vec<String>,
    scene: Option<RenderScene>,
    // SCH
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
            selected_cell: 0,
            cell_names: Vec::new(),
            scene: None,
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
        self.scene = None;
        self.sch = None;

        if is_sch_renderable(path) {
            match self.load_sch(path) {
                Ok(()) => self.status = format!("Loaded {}", path.display()),
                Err(e) => {
                    self.error = Some(e.clone());
                    self.status = format!("Failed to load {}", path.display());
                }
            }
        } else if is_gds_renderable(path) {
            match self.load_gds(path) {
                Ok(()) => self.status = format!("Loaded {}", path.display()),
                Err(e) => {
                    self.error = Some(e.clone());
                    self.status = format!("Failed to load {}", path.display());
                }
            }
        } else {
            self.status = format!("{} — formato no soportado", path.display());
        }
    }

    fn load_sch(&mut self, path: &Path) -> Result<(), String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let opts = xschem_viewer::RenderOptions::dark().with_sym_paths_from_xschemrc();
        let scene = xschem_viewer::SceneBuilder::new(&opts)
            .build(&xschem_viewer::parser::parse(&content).map_err(|e| e.to_string())?);

        let mut viewport = SchViewport::default();
        // Calculamos el rect aproximado para fit — usamos 800x600 como referencia
        let dummy_rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        );
        viewport.fit_to(&scene, dummy_rect);

        self.sch = Some(SchState { scene, viewport, diff: None });
        Ok(())
    }

    fn load_gds(&mut self, path: &Path) -> Result<(), String> {
        let lib = Library::open(path.to_string_lossy().as_ref());
        self.cell_names = lib
            .top_level()
            .cells()
            .map(|cell| cell.name().to_string())
            .collect();

        if self.cell_names.is_empty() {
            return Err(format!("No top-level cells found in {}", path.display()));
        }

        if self.selected_cell >= self.cell_names.len() {
            self.selected_cell = 0;
        }

        let selected_name = self.cell_names[self.selected_cell].clone();
        let cell = lib
            .find_cell(&selected_name)
            .ok_or_else(|| format!("Cell '{}' not found", selected_name))?;

        let config = RenderConfig::default();
        self.scene = Some(scene_from_cell(&cell, &config));
        Ok(())
    }

    fn reload_selected_cell(&mut self) {
        let Some(path) = self.selected_path.clone() else { return };
        if !is_gds_renderable(&path) { return }
        if let Err(err) = self.load_gds(&path) {
            self.error = Some(err.clone());
            self.status = format!("Failed to render {}", path.display());
        }
    }
}

impl eframe::App for RikuGuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        egui::Panel::top("top_bar").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Riku").strong());
                ui.separator();
                ui.label(&self.status);

                if ui.button("Refresh tree").clicked() {
                    self.refresh_tree();
                }

                // Controles GDS
                if let Some(scene) = self.scene.as_mut() {
                    ui.separator();
                    ui.checkbox(&mut scene.show_labels, "Labels");
                    let mut scale = scene.viewport.scale as f32;
                    if ui.add(egui::Slider::new(&mut scale, 0.25..=8.0).text("Zoom")).changed() {
                        scene.viewport.scale = scale as f64;
                    }
                    if ui.button("Reset zoom").clicked() {
                        scene.viewport.scale = 1.0;
                        scene.viewport.pan_x = 0.0;
                        scene.viewport.pan_y = 0.0;
                    }
                }

                // Controles SCH
                if let Some(sch) = self.sch.as_mut() {
                    ui.separator();
                    if ui.button("Fit").clicked() {
                        let dummy = egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(800.0, 600.0),
                        );
                        sch.viewport.fit_to(&sch.scene, dummy);
                    }
                    let mut scale = sch.viewport.scale as f32;
                    if ui.add(egui::Slider::new(&mut scale, 0.1..=20.0).text("Zoom")).changed() {
                        sch.viewport.scale = scale as f64;
                    }
                }
            });

            // Selector de celda GDS
            if self.scene.is_some() && !self.cell_names.is_empty() {
                let mut reload_cell = false;
                let mut selected = self.selected_cell;
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("Top-level cell")
                        .selected_text(&self.cell_names[self.selected_cell])
                        .show_ui(ui, |ui| {
                            for (idx, name) in self.cell_names.iter().enumerate() {
                                ui.selectable_value(&mut selected, idx, name);
                            }
                        });
                });
                if selected != self.selected_cell {
                    self.selected_cell = selected;
                    reload_cell = true;
                }
                if reload_cell { self.reload_selected_cell(); }
            }
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

        // ── Panel derecho: info ───────────────────────────────────────────────
        egui::Panel::right("info_panel")
            .resizable(true)
            .default_size(240.0)
            .show_inside(ui, |ui| {
                ui.heading("Details");
                ui.separator();

                if let Some(path) = &self.selected_path {
                    ui.label(format!("File: {}", path.file_name().unwrap_or_default().to_string_lossy()));
                } else {
                    ui.label("No file selected");
                }

                // Info GDS
                if let Some(scene) = self.scene.as_mut() {
                    ui.separator();
                    ui.label(format!("Layers: {}", scene.catalog.layers.len()));
                    ui.label(format!("Commands: {}", scene.commands.len()));
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for layer in &mut scene.catalog.layers {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut layer.visible, "");
                                let color = egui::Color32::from_rgba_unmultiplied(
                                    layer.fill.r, layer.fill.g, layer.fill.b, layer.fill.a,
                                );
                                ui.colored_label(color, format!(
                                    "{} ({},{})", layer.name, layer.tag.layer, layer.tag.datatype
                                ));
                            });
                        }
                    });
                }

                // Info SCH + diff
                if let Some(sch) = &self.sch {
                    ui.separator();
                    ui.label(format!("Elementos: {}", sch.scene.elements.len()));
                    ui.label(format!("Wires: {}", sch.scene.wires.len()));
                    if !sch.scene.missing_symbols.is_empty() {
                        ui.separator();
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 160, 60),
                            format!("Símbolos no resueltos: {}", sch.scene.missing_symbols.len()),
                        );
                        for s in &sch.scene.missing_symbols {
                            ui.label(format!("  • {s}"));
                        }
                    }
                    if let Some(diff) = &sch.diff {
                        ui.separator();
                        ui.label(RichText::new("Diff").strong());
                        let sem: Vec<_> = diff.components.iter().filter(|c| !c.cosmetic).collect();
                        let cos: Vec<_> = diff.components.iter().filter(|c| c.cosmetic).collect();
                        if !sem.is_empty() {
                            for c in &sem {
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
                        }
                        if !cos.is_empty() {
                            ui.label(format!("  {} cosméticos", cos.len()));
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

        // ── Panel central: canvas ─────────────────────────────────────────────
        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(sch) = &mut self.sch {
                let available = ui.available_size_before_wrap();
                let response = ui.allocate_response(available, egui::Sense::drag());

                // Pan con drag
                if response.dragged() {
                    let delta = response.drag_delta();
                    sch.viewport.pan_x -= delta.x as f64 / sch.viewport.scale;
                    sch.viewport.pan_y -= delta.y as f64 / sch.viewport.scale;
                    ctx.request_repaint();
                }

                // Zoom con scroll
                let scroll = ctx.input(|i| i.smooth_scroll_delta.y as f64);
                if scroll.abs() > f64::EPSILON && response.hovered() {
                    let factor = 1.0 + scroll * 0.002;
                    sch.viewport.scale = (sch.viewport.scale * factor).clamp(0.01, 100.0);
                    ctx.request_repaint();
                }

                ui.scope_builder(egui::UiBuilder::new().max_rect(response.rect), |ui| {
                    paint_sch(ui, &sch.scene, &sch.viewport, sch.diff.as_ref());
                });

            } else if let Some(scene) = &mut self.scene {
                let response = ui.allocate_response(
                    ui.available_size_before_wrap(), egui::Sense::drag(),
                );
                if response.dragged() {
                    let delta = response.drag_delta();
                    let scale = scene.viewport.scale.max(0.1);
                    scene.viewport.pan_x -= delta.x as f64 / scale;
                    scene.viewport.pan_y += delta.y as f64 / scale;
                    ctx.request_repaint();
                }
                let scroll = ctx.input(|i| i.smooth_scroll_delta.y as f64);
                if scroll.abs() > f64::EPSILON && response.hovered() {
                    let scale = (scene.viewport.scale * (1.0 + scroll * 0.001)).clamp(0.05, 32.0);
                    scene.viewport.scale = scale;
                    ctx.request_repaint();
                }
                ui.scope_builder(egui::UiBuilder::new().max_rect(response.rect), |ui| {
                    paint_scene(ui, &*scene);
                });

            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Selecciona un archivo .sch o .gds del árbol de proyecto.");
                });
            }
        });
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
