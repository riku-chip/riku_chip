use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use gds_renderer::{RenderConfig, RenderScene, scene_from_cell};
use gdstk_rs::Library;

use crate::project::{ProjectEntry, is_gds_renderable};
use crate::scene_painter::paint_scene;

pub struct RikuGuiApp {
    project_root: PathBuf,
    project_tree: ProjectEntry,
    selected_path: Option<PathBuf>,
    selected_cell: usize,
    cell_names: Vec<String>,
    scene: Option<RenderScene>,
    status: String,
    error: Option<String>,
}

impl RikuGuiApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, initial_path: Option<PathBuf>) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let (project_root, selected_path) = match initial_path {
            Some(path) if path.is_file() => {
                let root = path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| cwd.clone());
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

        if !is_gds_renderable(path) {
            self.scene = None;
            self.cell_names.clear();
            self.status = format!("{} is not a renderable GDS file yet.", path.display());
            return;
        }

        match self.load_gds(path) {
            Ok(()) => {
                self.status = format!("Loaded {}", path.display());
            }
            Err(err) => {
                self.scene = None;
                self.cell_names.clear();
                self.error = Some(err.clone());
                self.status = format!("Failed to load {}", path.display());
            }
        }
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
            .ok_or_else(|| format!("Cell '{}' not found in {}", selected_name, path.display()))?;

        let config = RenderConfig::default();
        self.scene = Some(scene_from_cell(&cell, &config));
        Ok(())
    }

    fn reload_selected_cell(&mut self) {
        let Some(path) = self.selected_path.clone() else {
            return;
        };
        if !is_gds_renderable(&path) {
            return;
        }
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
                ui.label(self.status.clone());

                if ui.button("Refresh tree").clicked() {
                    self.refresh_tree();
                }

                if ui.button("Fit cell").clicked() {
                    self.selected_cell = 0;
                    self.reload_selected_cell();
                }
            });

            let mut reload_cell = false;
            if let Some(scene) = self.scene.as_mut() {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut scene.show_labels, "Labels");
                    let mut scale = scene.viewport.scale as f32;
                    if ui
                        .add(egui::Slider::new(&mut scale, 0.25..=8.0).text("Zoom"))
                        .changed()
                    {
                        scene.viewport.scale = scale as f64;
                    }
                    if ui.button("Reset zoom").clicked() {
                        scene.viewport.scale = 1.0;
                        scene.viewport.pan_x = 0.0;
                        scene.viewport.pan_y = 0.0;
                    }
                });

                if !self.cell_names.is_empty() {
                    let mut selected = self.selected_cell;
                    egui::ComboBox::from_label("Top-level cell")
                        .selected_text(self.cell_names[self.selected_cell].as_str())
                        .show_ui(ui, |ui| {
                            for (idx, name) in self.cell_names.iter().enumerate() {
                                ui.selectable_value(&mut selected, idx, name);
                            }
                        });
                    if selected != self.selected_cell {
                        self.selected_cell = selected;
                        reload_cell = true;
                    }
                }
            }

            if reload_cell {
                self.reload_selected_cell();
            }
        });

        egui::Panel::left("project_tree")
            .resizable(true)
            .default_size(280.0)
            .show_inside(ui, |ui| {
                ui.heading("Project");
                ui.label(self.project_root.display().to_string());
                ui.separator();
                let tree = self.project_tree.clone();
                let selected_path = self.selected_path.clone();
                let mut open_path = |path: &Path| {
                    self.open_path(path);
                };
                show_entry_tree(ui, &tree, selected_path.as_deref(), &mut open_path);
            });

        egui::Panel::right("info_panel")
            .resizable(true)
            .default_size(280.0)
            .show_inside(ui, |ui| {
                ui.heading("Details");
                ui.separator();

                if let Some(path) = &self.selected_path {
                    ui.label(format!("Selected: {}", path.display()));
                } else {
                    ui.label("Selected: none");
                }

                if let Some(scene) = self.scene.as_mut() {
                    ui.separator();
                    ui.label(format!("Layers: {}", scene.catalog.layers.len()));
                    ui.label(format!("Commands: {}", scene.commands.len()));
                    ui.label(format!("Labels: {}", scene.show_labels));
                    ui.separator();
                    ui.label("Layers");
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for layer in &mut scene.catalog.layers {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut layer.visible, "");
                                let color = egui::Color32::from_rgba_unmultiplied(
                                    layer.fill.r,
                                    layer.fill.g,
                                    layer.fill.b,
                                    layer.fill.a,
                                );
                                ui.colored_label(
                                    color,
                                    format!(
                                        "{}  ({}, {})",
                                        layer.name, layer.tag.layer, layer.tag.datatype
                                    ),
                                );
                            });
                        }
                    });
                }

                if let Some(error) = &self.error {
                    ui.separator();
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
                }
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(scene) = &mut self.scene {
                let response =
                    ui.allocate_response(ui.available_size_before_wrap(), egui::Sense::drag());
                if response.dragged() {
                    let delta = response.drag_delta();
                    let scale = scene.viewport.scale.max(0.1);
                    scene.viewport.pan_x -= delta.x as f64 / scale;
                    scene.viewport.pan_y += delta.y as f64 / scale;
                    ctx.request_repaint();
                }

                let scroll = ctx.input(|input| input.smooth_scroll_delta.y as f64);
                if scroll.abs() > f64::EPSILON && response.hovered() {
                    let mut scale = scene.viewport.scale * (1.0 + scroll * 0.001);
                    if !scale.is_finite() || scale <= 0.05 {
                        scale = 0.05;
                    }
                    scene.viewport.scale = scale.clamp(0.05, 32.0);
                    ctx.request_repaint();
                }

                ui.scope_builder(egui::UiBuilder::new().max_rect(response.rect), |ui| {
                    paint_scene(ui, &*scene);
                });
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Select a GDS file from the project tree.");
                });
            }
        });
    }
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
        ProjectEntry::Directory {
            path,
            name,
            children,
        } => {
            egui::CollapsingHeader::new(name)
                .default_open(selected.map_or(false, |current| current == path.as_path()))
                .show(ui, |ui| {
                    for child in children {
                        show_entry_tree(ui, child, selected, on_select);
                    }
                });
        }
        ProjectEntry::File { path, name } => {
            let selected = selected == Some(path.as_path());
            if ui.selectable_label(selected, name).clicked() {
                on_select(path);
            }
        }
    }
}
