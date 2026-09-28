//! Pantalla de inicio: lo que se ve sin nada abierto. `riku gui` debe
//! bastar para trabajar sin la terminal: la carpeta y su repo, abrir otra,
//! las acciones de la CLI (cambios sin commitear, historial, comparar,
//! diagnóstico) y los archivos recientes.
//!
//! Solo dibuja y devuelve lo que se pidió ([`HomeAction`]); `app` lo hace.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use crate::core::analysis::status::StatusReport;
use crate::core::analysis::summary::SummaryCategory;
use crate::gui::history::counts_text;
use crate::gui::theme::space;
use crate::gui::tr;

/// Los cambios sin commitear del repo, según vayan llegando.
pub(crate) enum StatusView<'a> {
    NoRepo,
    Loading,
    Ready(&'a StatusReport),
    Failed(&'a str),
}

pub(crate) struct HomeInput<'a> {
    pub root: &'a Path,
    pub status: StatusView<'a>,
    pub recent_files: &'a [String],
    pub recent_dirs: &'a [String],
}

pub(crate) enum HomeAction {
    PickFolder,
    OpenFolder(PathBuf),
    OpenFile(PathBuf),
    /// Un archivo con cambios sin commitear: su diff contra `HEAD`.
    OpenChange(String),
    History,
    Compare,
    Doctor,
    RefreshStatus,
}

/// Ancho de la columna del inicio.
const WIDTH: f32 = 720.0;

pub(crate) fn show(ui: &mut egui::Ui, input: HomeInput<'_>) -> Option<HomeAction> {
    let mut action = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            let width = WIDTH.min(ui.available_width() - 2.0 * space::M);
            ui.add_space((ui.available_height() * 0.06).max(space::L));
            ui.label(RichText::new("Riku").size(26.0).strong());
            ui.label(RichText::new(tr!("home.subtitle")).weak());
            ui.add_space(space::L);

            ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    project_card(ui, &input, &mut action);
                    ui.add_space(space::M);
                    actions_card(ui, &input, &mut action);
                    if !matches!(input.status, StatusView::NoRepo) {
                        ui.add_space(space::M);
                        changes_card(ui, &input.status, &mut action);
                    }
                    let recent: Vec<&String> = input.recent_files.iter().filter(|p| Path::new(p).is_file()).collect();
                    if !recent.is_empty() {
                        ui.add_space(space::M);
                        recent_card(ui, &recent, &mut action);
                    }
                    ui.add_space(space::L);
                    ui.vertical_centered(|ui| ui.label(RichText::new(tr!("empty.shortcuts")).small().weak()));
                });
            });
        });
    });
    action
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(space::M as i8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

fn title(ui: &mut egui::Ui, text: String) {
    ui.label(RichText::new(text).strong());
    ui.add_space(space::XS);
}

/// La carpeta abierta, su repo y las carpetas recientes.
fn project_card(ui: &mut egui::Ui, input: &HomeInput<'_>, action: &mut Option<HomeAction>) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            let name = input.root.file_name().map_or_else(|| input.root.display().to_string(), |n| n.to_string_lossy().to_string());
            ui.label(RichText::new(name).size(17.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let open = egui::Button::new(RichText::new(tr!("home.open_folder")).strong()).fill(ui.visuals().selection.bg_fill);
                if ui.add(open).on_hover_text(tr!("home.open_folder_hint")).clicked() {
                    *action = Some(HomeAction::PickFolder);
                }
            });
        });
        ui.add(egui::Label::new(RichText::new(input.root.display().to_string()).small().weak()).truncate());
        ui.add_space(space::XS);
        let repo_line = match &input.status {
            StatusView::NoRepo => RichText::new(tr!("home.no_repo")).color(ui.visuals().warn_fg_color),
            StatusView::Ready(r) => {
                let branch = r.branch.as_ref().map_or_else(|| tr!("home.detached"), |b| b.name.clone());
                let n = r.files.len();
                let changes = if n == 0 { tr!("home.clean") } else { tr!("home.changes_count", count = n) };
                RichText::new(format!("Git · {branch} · {changes}"))
            }
            StatusView::Loading => RichText::new(format!("Git · {}", tr!("home.status_loading"))).weak(),
            StatusView::Failed(e) => RichText::new(format!("Git · {e}")).color(ui.visuals().error_fg_color),
        };
        ui.label(repo_line);

        let others: Vec<&String> = input
            .recent_dirs
            .iter()
            .filter(|d| Path::new(d) != input.root && Path::new(d).is_dir())
            .collect();
        if !others.is_empty() {
            ui.add_space(space::S);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tr!("home.recent_folders")).small().weak());
                for d in others {
                    let name = Path::new(d).file_name().map_or_else(|| d.clone(), |n| n.to_string_lossy().to_string());
                    if ui.small_button(name).on_hover_text(d.as_str()).clicked() {
                        *action = Some(HomeAction::OpenFolder(PathBuf::from(d)));
                    }
                }
            });
        }
    });
}

/// Lo que se puede hacer: los mismos comandos que la CLI.
fn actions_card(ui: &mut egui::Ui, input: &HomeInput<'_>, action: &mut Option<HomeAction>) {
    let has_repo = !matches!(input.status, StatusView::NoRepo);
    card(ui, |ui| {
        title(ui, tr!("home.actions"));
        // Tres del mismo ancho en una fila; en una ventana angosta, uno
        // debajo del otro.
        let gap = ui.spacing().item_spacing.x;
        let w = ((ui.available_width() - 2.0 * gap) / 3.0).floor();
        let w = if w < 180.0 { ui.available_width() } else { w };
        let no_repo = tr!("home.needs_repo");
        let buttons = [
            (has_repo, tr!("history.title"), tr!("home.history_hint"), no_repo.clone(), HomeAction::History),
            (has_repo, tr!("home.compare"), tr!("home.compare_hint"), no_repo, HomeAction::Compare),
            (true, tr!("home.doctor"), tr!("home.doctor_hint"), String::new(), HomeAction::Doctor),
        ];
        // Todos del alto del más alto: una ayuda que ocupa dos líneas en una
        // ventana angosta no desalinea la fila.
        let h = buttons.iter().map(|(_, t, hint, _, _)| action_height(ui, w, t, hint)).fold(0.0_f32, f32::max);
        ui.horizontal_wrapped(|ui| {
            for (enabled, title, hint, disabled_hint, act) in buttons {
                if big_button(ui, [w, h], enabled, &title, &hint, &disabled_hint) {
                    *action = Some(act);
                }
            }
        });
    });
}

/// Márgenes internos de un botón de acción.
const PAD: egui::Vec2 = egui::vec2(12.0, 9.0);

/// Título y ayuda de un botón, ya armados para el ancho `w` (la ayuda se
/// ajusta en varias líneas si no entra; el título se corta con "…").
fn action_texts(ui: &egui::Ui, w: f32, title: &str, hint: &str) -> (std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>) {
    let wrap = w - 2.0 * PAD.x;
    let title = egui::WidgetText::from(RichText::new(title).strong()).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        wrap,
        egui::TextStyle::Body,
    );
    let hint = egui::WidgetText::from(RichText::new(hint).small()).into_galley(
        ui,
        Some(egui::TextWrapMode::Wrap),
        wrap,
        egui::TextStyle::Small,
    );
    (title, hint)
}

fn action_height(ui: &egui::Ui, w: f32, title: &str, hint: &str) -> f32 {
    let (t, h) = action_texts(ui, w, title, hint);
    2.0 * PAD.y + t.size().y + 3.0 + h.size().y
}

/// Botón de acción: título arriba y qué hace debajo, alineados a la
/// izquierda. Dibujado a mano: el botón de egui centra el bloque de texto.
fn big_button(ui: &mut egui::Ui, size: [f32; 2], enabled: bool, title: &str, hint: &str, disabled_hint: &str) -> bool {
    let (title_g, hint_g) = action_texts(ui, size[0], title, hint);
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size[0], size[1]), sense);
    let style = if enabled { *ui.style().interact(&resp) } else { ui.visuals().widgets.noninteractive };
    let v = ui.visuals();
    ui.painter().rect(rect, style.corner_radius, style.weak_bg_fill, style.bg_stroke, egui::StrokeKind::Inside);
    let (title_color, hint_color) = if enabled {
        (v.strong_text_color(), v.weak_text_color())
    } else {
        (v.weak_text_color(), v.weak_text_color().gamma_multiply(0.6))
    };
    let top = rect.min + PAD;
    let title_h = title_g.size().y;
    ui.painter().galley(top, title_g, title_color);
    ui.painter().galley(top + egui::vec2(0.0, title_h + 3.0), hint_g, hint_color);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    } else {
        resp.on_hover_text(disabled_hint);
        false
    }
}

/// Cambios sin commitear (lo de `riku status`): cada uno abre su diff
/// contra `HEAD`.
fn changes_card(ui: &mut egui::Ui, status: &StatusView<'_>, action: &mut Option<HomeAction>) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr!("home.changes")).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("↻").on_hover_text(tr!("home.refresh")).clicked() {
                    *action = Some(HomeAction::RefreshStatus);
                }
            });
        });
        ui.add_space(space::XS);
        match status {
            StatusView::NoRepo => {}
            StatusView::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(tr!("home.status_loading")).weak());
                });
            }
            StatusView::Failed(e) => {
                ui.label(RichText::new(*e).color(ui.visuals().error_fg_color));
            }
            StatusView::Ready(r) if r.files.is_empty() => {
                ui.label(RichText::new(tr!("home.clean_hint")).weak());
            }
            StatusView::Ready(r) => {
                egui::ScrollArea::vertical().id_salt("home_changes").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
                    for f in &r.files {
                        change_row(ui, f, action);
                    }
                });
            }
        }
    });
}

fn change_row(ui: &mut egui::Ui, f: &crate::core::analysis::summary::FileSummary, action: &mut Option<HomeAction>) {
    let v = ui.visuals();
    let (what, color) = match f.category {
        SummaryCategory::Semantic => (counts_text(f), v.text_color()),
        SummaryCategory::Cosmetic => (tr!("history.cosmetic"), v.weak_text_color()),
        SummaryCategory::Unchanged => (tr!("home.no_semantic"), v.weak_text_color()),
        SummaryCategory::Unknown => (tr!("home.no_module"), v.weak_text_color()),
        SummaryCategory::Error => (f.errors.join("; "), v.error_fg_color),
    };
    let openable = f.category != SummaryCategory::Unknown;
    ui.horizontal(|ui| {
        let name = RichText::new(&f.path).strong();
        let resp = if openable {
            ui.add(egui::Button::new(name).frame(false))
                .on_hover_text(tr!("home.open_change"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            ui.label(name.weak())
        };
        ui.add(egui::Label::new(RichText::new(what).small().color(color)).truncate());
        if openable && resp.clicked() {
            *action = Some(HomeAction::OpenChange(f.path.clone()));
        }
    });
}

fn recent_card(ui: &mut egui::Ui, recent: &[&String], action: &mut Option<HomeAction>) {
    card(ui, |ui| {
        title(ui, tr!("empty.recent"));
        for p in recent {
            let path = Path::new(p.as_str());
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            ui.horizontal(|ui| {
                let resp = ui
                    .add(egui::Button::new(RichText::new(name.as_ref()).strong()).frame(false))
                    .on_hover_text(p.as_str())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let dir = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                ui.add(egui::Label::new(RichText::new(dir).small().weak()).truncate());
                if resp.clicked() {
                    *action = Some(HomeAction::OpenFile(path.to_path_buf()));
                }
            });
        }
    });
}
