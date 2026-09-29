//! Panel de detalles de una escena (derecha): resumen, avisos, cambios del
//! diff y capas; y el selector de celdas (izquierda).

use eframe::egui::{self, RichText};
use viewer_core::{bbox::BoundingBox, diff::ChangeKind, ChangeItem};

use crate::gui::content::SceneState;
use crate::gui::entry_picker;
use crate::gui::scene_painter::to_color32;
use crate::gui::theme::{space, CanvasTheme};
use crate::gui::tr;

/// Detalles en secciones: resumen (metadatos del backend), avisos, cambios
/// (en diff) y capas con color y visibilidad. Las capas salen en el orden
/// que da la escena (para GDS: de abajo hacia arriba en el apilado del PDK).
pub(crate) fn show(ui: &mut egui::Ui, bs: &mut SceneState) {
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
            picked = change_items(ui, changes);
        });
        if let Some(target) = picked {
            bs.focus = Some(target);
        }
    }

    // Una fila por nombre: una capa puede venir en dos claves (en un diff de
    // esquemático, la atenuada y la del resaltado); ocultarla oculta las dos.
    let mut seen = std::collections::HashSet::new();
    let layers: Vec<_> = scene.layer_list().into_iter().filter(|(_, p)| seen.insert(p.name.clone())).collect();
    bs.layer_hover_panel = None;
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
                let row = ui.horizontal(|ui| {
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
                    // Clic en el nombre: resaltarla (fija); otro clic la suelta.
                    let name = RichText::new(&paint.name);
                    let pinned = bs.layer_focus.as_deref() == Some(paint.name.as_str());
                    if ui
                        .selectable_label(pinned, if visible { name } else { name.weak() })
                        .on_hover_text(tr!("details.layer_focus_hint"))
                        .clicked()
                    {
                        bs.toggle_layer_focus(&paint.name);
                    }
                });
                // Puntero sobre la fila: resaltarla mientras esté encima.
                if row.response.contains_pointer() {
                    bs.layer_hover_panel = Some(paint.name.clone());
                }
            }
        });
    });
}

/// Selector de celdas de la escena. Retorna el id de la celda elegida (la
/// carga la dispara quien llama).
pub(crate) fn entry_picker(ui: &mut egui::Ui, bs: &mut SceneState) -> Option<String> {
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
fn change_items(ui: &mut egui::Ui, changes: &[ChangeItem]) -> Option<BoundingBox> {
    let mut picked = None;
    egui::ScrollArea::vertical()
        .id_salt("change_list")
        .max_height(220.0)
        .show(ui, |ui| {
            for c in changes {
                let sign = match c.kind {
                    _ if c.error => "!",
                    ChangeKind::Added => "+",
                    ChangeKind::Removed => "−",
                    ChangeKind::Modified => "~",
                };
                // Un abierto o un corto: en rojo (el color de lo quitado) y en negrita.
                let kind = if c.error { ChangeKind::Removed } else { c.kind };
                let color = crate::gui::theme::change_color(kind, ui.visuals().dark_mode);
                let dim = |col: egui::Color32| if c.cosmetic { col.gamma_multiply(0.45) } else { col };
                let mut text = RichText::new(format!("{sign} {}", c.label)).color(dim(color));
                if c.error {
                    text = text.strong();
                }
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
