//! Las zonas de la ventana: barra superior, barra de estado, panel
//! izquierdo (proyecto, vistas del diff, celdas), panel de detalles y el
//! centro (lienzo, ondas o la pantalla inicial).

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use super::{short_hash, RikuGuiApp};
use crate::gui::canvas::{self, CanvasOptions, Readout};
use crate::gui::content::{DiffTab, SceneState};
use crate::gui::details_panel;
use crate::gui::project::ProjectEntry;
use crate::gui::theme::space;
use crate::gui::{i18n, tr};
#[cfg(feature = "spice")]
use crate::gui::wave_view;

impl RikuGuiApp {
    /// Acciones, de izquierda a derecha por uso; ajustes y tema a la derecha.
    pub(super) fn top_bar(&mut self, ui: &mut egui::Ui) {
        // ─── Barra de herramientas: acciones, de izquierda a derecha por uso ──
        egui::Panel::top("top_bar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Riku").strong().size(16.0));
                ui.separator();

                let has_view = self.content.scene().is_some();
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
                        // Los idiomas salen de riku/locales/*.yml: uno nuevo aparece solo.
                        ui.horizontal_wrapped(|ui| {
                            for (code, name) in i18n::languages() {
                                if ui.selectable_label(current == code, name).clicked() {
                                    i18n::set(&code);
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
    }

    /// Qué pasa, dónde está el cursor y la escala.
    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        // ─── Barra de estado: qué pasa, dónde está el cursor, escala ─────────
        egui::Panel::bottom("status_bar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if self.loader.busy() {
                    ui.spinner();
                }
                let status = RichText::new(&self.status);
                ui.label(if self.error.is_some() { status.color(ui.visuals().error_fg_color) } else { status });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let unit = self.content.scene()
                        .and_then(|bs| bs.scene.world_unit().map(str::to_string))
                        .unwrap_or_default();
                    if let Some(px) = self.readout.px_world {
                        ui.label(RichText::new(format!("1 px = {} {unit}", fmt_len(px))).weak());
                    }
                    if let Some((x, y)) = self.readout.cursor_world {
                        ui.separator();
                        ui.monospace(format!("x {:>9.3}  y {:>9.3} {unit}", x, y));
                    }
                    if self.readout.labels_hidden > 0 && self.show_labels {
                        ui.separator();
                        ui.label(RichText::new(tr!("status.labels_hidden", count = self.readout.labels_hidden)).weak());
                    }
                });
            });
        });
    }

    /// Proyecto (o las vistas de un diff) y el selector de celdas.
    pub(super) fn left_panel(&mut self, ui: &mut egui::Ui) {
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
                if let (Some(ctx), Some(current)) = (self.diff.as_ref(), self.content.scene().and_then(SceneState::diff_tab)) {
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
                } else if !(wave_tabs && self.diff.is_some()) {
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
                    let has_picker = self.content.scene().is_some_and(|bs| bs.scene.entries().len() > 1);
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
    }

    /// Detalles de lo que se ve.
    pub(super) fn right_panel(&mut self, ui: &mut egui::Ui) {
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
                if let Some(view) = self.content.wave_mut() {
                    let candidates = wave_view::raw_files(&self.project_tree);
                    wave_view::show_details(ui, view, &candidates);
                    if view.take_exprs_changed() {
                        self.wave_exprs = view.expr_texts().to_vec();
                    }
                    return;
                }
                if let Some(bs) = self.content.scene_mut() {
                    details_panel::show(ui, bs);
                } else {
                    ui.label(RichText::new(tr!("panel.nothing_open")).weak());
                }
            });
    }

    /// Lienzo, ondas o la pantalla inicial.
    pub(super) fn central(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.canvas_rect = Some(ui.max_rect());
            // Lecturas para la barra de estado: las repone quien pinte.
            self.readout = Readout::default();

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
            if let Some(view) = self.content.wave_mut() {
                wave_view::show_plot(ui, view);
                return;
            }

            // Escena cargada por el backend del formato (todos los formatos).
            let opts = CanvasOptions {
                reduce_motion: self.reduce_motion,
                labels: self.show_labels,
                simplify: self.simplify,
                block_px: self.block_px,
                profile: self.profile,
            };
            if let Some(bs) = self.content.scene_mut() {
                self.readout = canvas::show(ui, bs, opts);
                return;
            }

            if self.loader.busy() {
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
    }

    /// Selector de celdas bajo el panel izquierdo, si el archivo tiene más de una.
    pub(super) fn show_entry_picker(&mut self, ui: &mut egui::Ui) {
        let picked = match self.content.scene_mut() {
            Some(bs) if bs.scene.entries().len() > 1 => {
                ui.separator();
                details_panel::entry_picker(ui, bs)
            }
            _ => None,
        };
        if let Some(id) = picked {
            self.select_entry(&id);
        }
    }

    /// Selector Diff / Before / After de una comparación de formas de onda.
    /// Cambiar de vista no recarga nada: la vista ya tiene las dos versiones.
    #[cfg(feature = "spice")]
    pub(super) fn show_wave_tabs(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(w) = self.content.wave_mut().filter(|w| w.is_diff()) else { return false };
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

    /// Pantalla inicial: qué es esto, cómo empezar, archivos recientes y
    /// atajos. Retorna el archivo reciente elegido, si hubo clic.
    pub(super) fn empty_state(&self, ui: &mut egui::Ui) -> Option<PathBuf> {
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

    /// Ruta de lo que se está viendo: archivo › celda › vista del diff.
    pub(super) fn breadcrumb(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if let Some(ctx) = &self.diff {
            if ctx.from_history {
                parts.push(tr!("history.title"));
            }
            parts.push(format!("{} → {}", short_hash(&ctx.commit_a), short_hash(&ctx.commit_b)));
        }
        if let Some(p) = &self.selected_path {
            parts.push(p.file_name().unwrap_or_default().to_string_lossy().to_string());
        }
        if let Some(bs) = self.content.scene() {
            if let Some(cell) = bs.scene.current_entry() {
                parts.push(cell.to_string());
            }
            if let Some(tab) = bs.diff_tab() {
                parts.push(tab.label().to_string());
            }
        }
        #[cfg(feature = "spice")]
        if let Some(w) = self.content.wave().filter(|w| w.is_diff()) {
            parts.push(w.tab.label().to_string());
        }
        parts
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_len_uses_three_significant_digits() {
        assert_eq!(fmt_len(0.005678), "0.00568");
        assert_eq!(fmt_len(1.25), "1.25");
        assert_eq!(fmt_len(310.4), "310");
    }
}
