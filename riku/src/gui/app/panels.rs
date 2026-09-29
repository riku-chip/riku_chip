//! Las zonas de la ventana: barra superior, barra de estado, panel
//! izquierdo (proyecto, vistas del diff, celdas), panel de detalles y el
//! centro (lienzo, ondas o la pantalla inicial).

use std::path::Path;

use eframe::egui::{self, RichText};

use super::{short_hash, RikuGuiApp};
use crate::gui::canvas::{self, CanvasOptions, Readout};
use crate::gui::content::{Content, DiffTab, SceneState};
use crate::gui::details_panel;
use crate::gui::project::ProjectEntry;
use crate::gui::theme::space;
use crate::gui::window_frame;
use crate::gui::{i18n, tr};
#[cfg(feature = "spice")]
use crate::gui::wave_view;

impl RikuGuiApp {
    /// Acciones, de izquierda a derecha por uso; ajustes y tema a la derecha.
    pub(super) fn top_bar(&mut self, ui: &mut egui::Ui) {
        // ─── Barra de herramientas: acciones, de izquierda a derecha por uso ──
        // Sin el marco del sistema, la barra es el título de la ventana: más
        // alta, sin margen a la derecha (los botones llegan a la esquina) y
        // se arrastra para mover la ventana.
        let own_frame = !self.native_frame;
        let frame = egui::Frame::side_top_panel(ui.style()).inner_margin(if own_frame {
            egui::Margin { left: space::S as i8, right: 0, top: 0, bottom: 0 }
        } else {
            egui::Margin::symmetric(space::S as i8, 2)
        });
        egui::Panel::top("top_bar").frame(frame).show(ui, |ui| {
            let h = if own_frame { window_frame::BAR_H } else { ui.spacing().interact_size.y };
            if own_frame {
                let bar = egui::Rect::from_min_size(ui.max_rect().min, egui::vec2(ui.max_rect().width(), h));
                window_frame::drag_area(ui, bar);
            }
            let row = egui::Layout::left_to_right(egui::Align::Center);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), h), row, |ui| {
                ui.label(RichText::new("Riku").strong().size(16.0));
                ui.separator();
                let at_home = matches!(self.content, Content::Home) && !self.loader.busy();
                if ui
                    .add_enabled(!at_home, egui::Button::new(tr!("toolbar.home")))
                    .on_hover_text(tr!("toolbar.home_hint"))
                    .clicked()
                {
                    self.go_home();
                }

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
                    // Y lo del repo: un commit o un cambio hecho afuera.
                    self.history.invalidate();
                    if !matches!(self.repo_status, super::actions::RepoStatus::Loading(_)) {
                        self.repo_status = super::actions::RepoStatus::Stale;
                    }
                }
                ui.separator();
                ui.toggle_value(&mut self.show_labels, tr!("toolbar.labels"))
                    .on_hover_text(tr!("toolbar.labels_hint"));
                ui.toggle_value(&mut self.show_legend, tr!("toolbar.legend"))
                    .on_hover_text(tr!("toolbar.legend_hint"));
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

                ui.separator();
                let hint = if has_repo { tr!("toolbar.compare_hint") } else { tr!("toolbar.history_no_repo") };
                if ui
                    .add_enabled(has_repo, egui::Button::new(tr!("toolbar.compare")))
                    .on_hover_text(hint.clone())
                    .on_disabled_hover_text(hint)
                    .clicked()
                {
                    self.open_compare();
                }
                let can_export = self.can_export() && self.export_job.is_none();
                ui.add_enabled_ui(can_export, |ui| {
                    ui.menu_button(tr!("toolbar.export"), |ui| {
                        let ctx = ui.ctx().clone();
                        if ui.button("PNG").on_hover_text(tr!("toolbar.export_png_hint")).clicked() {
                            self.export_image(true, &ctx);
                        }
                        if ui.button("SVG").on_hover_text(tr!("toolbar.export_svg_hint")).clicked() {
                            self.export_image(false, &ctx);
                        }
                    })
                    .response
                    .on_hover_text(tr!("toolbar.export_hint"))
                    .on_disabled_hover_text(tr!("toolbar.export_disabled"));
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if own_frame {
                        ui.scope(window_frame::controls);
                        ui.separator();
                    }
                    // Lo menos frecuente, un nivel más abajo.
                    ui.menu_button(tr!("settings.menu"), |ui| {
                        ui.checkbox(&mut self.reduce_motion, tr!("settings.reduce_motion"))
                            .on_hover_text(tr!("settings.reduce_motion_hint"));
                        ui.checkbox(&mut self.simplify, tr!("settings.simplify"))
                            .on_hover_text(tr!("settings.simplify_hint"));
                        ui.horizontal(|ui| {
                            ui.label(tr!("settings.scroll")).on_hover_text(tr!("settings.scroll_hint"));
                            ui.selectable_value(&mut self.scroll_pans, false, tr!("settings.scroll_zoom"));
                            ui.selectable_value(&mut self.scroll_pans, true, tr!("settings.scroll_pan"));
                        });
                        if ui
                            .checkbox(&mut self.native_frame, tr!("settings.native_frame"))
                            .on_hover_text(tr!("settings.native_frame_hint"))
                            .changed()
                        {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Decorations(self.native_frame));
                        }
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
                            (
                                tr!("shortcut.wheel"),
                                if self.scroll_pans { tr!("shortcut.wheel_pan") } else { tr!("shortcut.wheel_what") },
                            ),
                            (tr!("shortcut.pinch"), tr!("shortcut.pinch_what")),
                            (tr!("shortcut.drag"), tr!("shortcut.drag_what")),
                            ("Backspace / Alt + ←".to_string(), tr!("shortcut.back")),
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
        egui::Panel::bottom("status_bar").show(ui, |ui| {
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
                    if let Some(net) = self.content.scene().and_then(|bs| bs.net_focus.as_ref()) {
                        ui.separator();
                        ui.label(tr!("status.net", name = net.name, count = net.outline.len()));
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
            .max_size(side_panel_max(ui))
            .show(ui, |ui| {
                // Diff de todo el repo: la lista arriba; debajo, las vistas del
                // archivo abierto.
                let mut req = None;
                if let Some(cs) = self.change_set.as_mut() {
                    req = cs.show(ui);
                    ui.separator();
                }
                match req {
                    Some(crate::gui::change_set::Request::Open(path)) => self.open_change_set_file(&path),
                    Some(crate::gui::change_set::Request::Close) => self.change_set = None,
                    None => {}
                }
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
                    if ui.small_button(tr!("panel.open_folder")).on_hover_text(tr!("home.open_folder_hint")).clicked() {
                        self.folder_picker = Some(crate::gui::folder_picker::FolderPicker::new(&self.project_root));
                    }
                    if ui
                        .checkbox(&mut self.show_all_files, tr!("panel.all_files"))
                        .on_hover_text(tr!("panel.all_files_hint", exts = self.openable_text()))
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
            .max_size(side_panel_max(ui))
            .show(ui, |ui| {
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
        egui::CentralPanel::default().show(ui, |ui| {
            self.canvas_rect = Some(ui.max_rect());
            // Lecturas para la barra de estado: las repone quien pinte.
            self.readout = Readout::default();

            // ¿Dónde estoy? Ruta archivo › celda › vista sobre el lienzo.
            let crumbs = self.breadcrumb();
            // "Volver" al nivel de donde se entró (sub-celda, sub-esquemático).
            let back_to = self.content.scene().and_then(|bs| {
                bs.back.last().map(|s| s.entry.clone().unwrap_or_else(|| {
                    Path::new(&bs.path).file_name().unwrap_or_default().to_string_lossy().to_string()
                }))
            });
            let mut go_back = false;
            if !crumbs.is_empty() || back_to.is_some() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = space::XS;
                    if let Some(to) = &back_to {
                        go_back = ui
                            .button(tr!("nav.back"))
                            .on_hover_text(tr!("nav.back_hint", to = to))
                            .clicked();
                        ui.add_space(space::XS);
                    }
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
            if go_back {
                self.go_back();
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
                scroll_pans: self.scroll_pans,
                labels: self.show_labels,
                legend: self.show_legend,
                simplify: self.simplify,
                block_px: self.block_px,
                profile: self.profile,
            };
            if let Some(bs) = self.content.scene_mut() {
                self.readout = canvas::show(ui, bs, opts);
                if let Some(entry) = self.readout.enter.take() {
                    self.enter_entry(&entry);
                }
                return;
            }

            if self.loader.busy() {
                ui.centered_and_justified(|ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr!("canvas.loading"));
                    });
                });
            } else {
                self.show_home(ui);
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
            self.enter_entry(&id);
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

/// Lo más ancho que puede quedar un panel lateral: el lienzo nunca
/// desaparece, aunque algo de adentro pida más lugar.
fn side_panel_max(ui: &egui::Ui) -> f32 {
    (ui.available_width() * 0.4).max(160.0)
}
