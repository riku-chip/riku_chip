//! Selector de sub-vistas (celdas de un GDS) con buscador.
//!
//! Una librería de celdas estándar trae cientos de top cells; la lista se
//! filtra por texto y, por defecto, muestra solo las raíces. En un diff, las
//! celdas cambiadas llevan un marcador y se pueden listar solas. Las filas se
//! dibujan virtualizadas (`show_rows`): solo las visibles cuestan.

use eframe::egui::{self, Color32, RichText};
use viewer_core::{diff::ChangeKind, scene::ViewEntry};

use crate::gui::tr;

/// Estado del filtro que el caller conserva entre frames.
pub struct PickerState<'a> {
    pub query: &'a mut String,
    pub only_roots: &'a mut bool,
    /// Solo celdas con cambios (se ignora si ninguna entrada tiene cambios).
    pub only_changed: &'a mut bool,
}

/// Criterios de filtrado ya resueltos.
#[derive(Clone, Copy)]
pub struct Filter<'a> {
    pub query: &'a str,
    pub only_roots: bool,
    pub only_changed: bool,
}

/// Índices de `entries` que pasan el filtro, en el orden original.
/// Búsqueda por subcadena sin distinguir mayúsculas.
pub fn filter_entries(entries: &[ViewEntry], f: Filter<'_>) -> Vec<usize> {
    let q = f.query.trim().to_lowercase();
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !f.only_roots || e.is_root)
        .filter(|(_, e)| !f.only_changed || e.change.is_some())
        .filter(|(_, e)| q.is_empty() || e.id.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect()
}

/// Dibuja el selector. Retorna el id elegido con un clic (si hubo).
pub fn show(
    ui: &mut egui::Ui,
    entries: &[ViewEntry],
    current: Option<&str>,
    state: PickerState<'_>,
) -> Option<String> {
    let roots = entries.iter().filter(|e| e.is_root).count();
    let changed = entries.iter().filter(|e| e.change.is_some()).count();
    ui.horizontal(|ui| {
        ui.label(RichText::new(tr!("cells.title")).strong());
        ui.label(
            RichText::new(format!("{roots} top / {}", entries.len()))
                .small()
                .color(Color32::from_gray(150)),
        );
    });
    ui.add(
        egui::TextEdit::singleline(state.query)
            .hint_text(tr!("cells.search"))
            .desired_width(f32::INFINITY),
    );
    ui.checkbox(state.only_roots, tr!("cells.only_top"));
    if changed > 0 {
        ui.checkbox(state.only_changed, tr!("cells.only_changed", count = changed));
    }

    let filter = Filter {
        query: state.query,
        only_roots: *state.only_roots,
        only_changed: *state.only_changed && changed > 0,
    };
    let visible = filter_entries(entries, filter);
    if visible.is_empty() {
        let hint = if filter.only_changed && filter.only_roots {
            tr!("cells.no_match_hint")
        } else {
            tr!("cells.no_match")
        };
        ui.label(RichText::new(hint).italics().color(Color32::from_gray(140)));
        return None;
    }

    let mut picked = None;
    let row_h = ui.spacing().interact_size.y;
    egui::ScrollArea::vertical()
        .id_salt("entry_picker")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, visible.len(), |ui, range| {
            for &i in &visible[range] {
                let e = &entries[i];
                let selected = current == Some(e.id.as_str());
                let resp = ui
                    .add(egui::Button::selectable(selected, row_text(ui, e)).truncate())
                    .on_hover_text(hover_text(e));
                if resp.clicked() && !selected {
                    picked = Some(e.id.clone());
                }
            }
        });
    picked
}

/// Parte distintiva y prefijo de librería de un nombre de celda:
/// `sky130_fd_sc_hd__inv_1` → (`inv_1`, `sky130_fd_sc_hd`). Sin `__`, todo
/// es parte distintiva.
fn split_name(id: &str) -> (&str, Option<&str>) {
    match id.rsplit_once("__") {
        Some((lib, cell)) if !lib.is_empty() && !cell.is_empty() => (cell, Some(lib)),
        _ => (id, None),
    }
}

/// Marcador y color de un cambio de celda (mismos colores que el overlay).
fn change_mark(kind: ChangeKind, dark: bool) -> (&'static str, Color32) {
    let mark = match kind {
        ChangeKind::Added => "+",
        ChangeKind::Removed => "−",
        ChangeKind::Modified => "~",
    };
    (mark, crate::gui::theme::change_color(kind, dark))
}

/// Fila: marcador de cambio (si hay), nombre distintivo (lo que se trunca es
/// la librería, al final) y el prefijo en gris, así `sky130_ef_sc_hd__decap_12`
/// y `sky130_fd_sc_hd__decap_12` se distinguen sin perder la parte útil.
fn row_text(ui: &egui::Ui, e: &ViewEntry) -> egui::text::LayoutJob {
    let (cell, lib) = split_name(&e.id);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    let name_color = match e.change {
        Some(kind) => {
            let (mark, color) = change_mark(kind, ui.visuals().dark_mode);
            job.append(mark, 0.0, egui::TextFormat::simple(font.clone(), color));
            job.append(" ", 0.0, egui::TextFormat::simple(font.clone(), color));
            color
        }
        None => ui.visuals().text_color(),
    };
    job.append(cell, 0.0, egui::TextFormat::simple(font.clone(), name_color));
    if let Some(lib) = lib {
        job.append(lib, 6.0, egui::TextFormat::simple(font, Color32::from_gray(120)));
    }
    job
}

fn hover_text(e: &ViewEntry) -> String {
    let kind = if e.is_root { tr!("cells.top") } else { tr!("cells.sub") };
    let change = match e.change {
        Some(ChangeKind::Added) => tr!("cells.added"),
        Some(ChangeKind::Removed) => tr!("cells.removed"),
        Some(ChangeKind::Modified) => tr!("cells.modified"),
        None => String::new(),
    };
    match e.size {
        Some((w, h)) => format!("{}\n{kind} · {w:.3} × {h:.3} µm{change}", e.id),
        None => format!("{}\n{kind} · {}{change}", e.id, tr!("cells.no_geometry")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, is_root: bool) -> ViewEntry {
        ViewEntry { id: id.into(), is_root, size: None, change: None, renamed_from: None }
    }

    fn lib() -> Vec<ViewEntry> {
        let mut v = vec![
            entry("sky130_fd_sc_hd__inv_1", true),
            entry("sky130_fd_sc_hd__INV_4", true),
            entry("sky130_fd_sc_hd__nand2_1", true),
            entry("sky130_fd_pr__inv_core", false),
        ];
        v[2].change = Some(ChangeKind::Modified);
        v[3].change = Some(ChangeKind::Added);
        v
    }

    fn f(query: &str, only_roots: bool, only_changed: bool) -> Filter<'_> {
        Filter { query, only_roots, only_changed }
    }

    #[test]
    fn filter_is_case_insensitive_and_respects_roots() {
        assert_eq!(filter_entries(&lib(), f("inv", true, false)), vec![0, 1]);
        assert_eq!(filter_entries(&lib(), f(" INV ", false, false)), vec![0, 1, 3]);
        assert_eq!(filter_entries(&lib(), f("", true, false)), vec![0, 1, 2]);
        assert!(filter_entries(&lib(), f("xor", false, false)).is_empty());
    }

    #[test]
    fn filter_only_changed() {
        assert_eq!(filter_entries(&lib(), f("", false, true)), vec![2, 3]);
        assert_eq!(filter_entries(&lib(), f("", true, true)), vec![2]);
        assert_eq!(filter_entries(&lib(), f("inv", false, true)), vec![3]);
    }

    #[test]
    fn split_name_separates_library_prefix() {
        assert_eq!(split_name("sky130_fd_sc_hd__inv_1"), ("inv_1", Some("sky130_fd_sc_hd")));
        assert_eq!(split_name("sky130_ef_sc_hd__decap_12"), ("decap_12", Some("sky130_ef_sc_hd")));
        assert_eq!(split_name("TOP"), ("TOP", None));
        assert_eq!(split_name("__x"), ("__x", None));
    }

    #[test]
    fn hover_text_shows_kind_size_and_change() {
        let mut e = entry("sky130_fd_sc_hd__inv_1", true);
        e.size = Some((1.38, 3.2));
        assert_eq!(hover_text(&e), "sky130_fd_sc_hd__inv_1\ntop cell · 1.380 × 3.200 µm");
        e.change = Some(ChangeKind::Modified);
        assert!(hover_text(&e).ends_with("µm · modified"));
        assert!(hover_text(&entry("X", false)).contains("subcell · no geometry"));
    }
}
