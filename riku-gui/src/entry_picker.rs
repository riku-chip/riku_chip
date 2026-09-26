//! Selector de sub-vistas (celdas de un GDS) con buscador.
//!
//! Una librería de celdas estándar trae cientos de top cells; la lista se
//! filtra por texto y, por defecto, muestra solo las raíces. Las filas se
//! dibujan virtualizadas (`show_rows`): solo las visibles cuestan.

use eframe::egui::{self, Color32, RichText};
use viewer_core::scene::ViewEntry;

/// Estado del filtro que el caller conserva entre frames.
pub struct PickerState<'a> {
    pub query: &'a mut String,
    pub only_roots: &'a mut bool,
}

/// Índices de `entries` que pasan el filtro, en el orden original.
/// Búsqueda por subcadena sin distinguir mayúsculas.
pub fn filter_entries(entries: &[ViewEntry], query: &str, only_roots: bool) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !only_roots || e.is_root)
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
    ui.horizontal(|ui| {
        ui.label(RichText::new("Celdas").strong());
        ui.label(
            RichText::new(format!("{roots} top / {}", entries.len()))
                .small()
                .color(Color32::from_gray(150)),
        );
    });
    ui.add(
        egui::TextEdit::singleline(state.query)
            .hint_text("buscar celda…")
            .desired_width(f32::INFINITY),
    );
    ui.checkbox(state.only_roots, "solo top cells");

    let visible = filter_entries(entries, state.query, *state.only_roots);
    if visible.is_empty() {
        ui.label(RichText::new("sin coincidencias").italics().color(Color32::from_gray(140)));
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
                    .add(egui::Button::selectable(selected, row_text(ui, &e.id)).truncate())
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

/// Fila: nombre distintivo primero (lo que se trunca es la librería, al
/// final) y el prefijo en gris, así `sky130_ef_sc_hd__decap_12` y
/// `sky130_fd_sc_hd__decap_12` se distinguen sin perder la parte útil.
fn row_text(ui: &egui::Ui, id: &str) -> egui::text::LayoutJob {
    let (cell, lib) = split_name(id);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let strong = ui.visuals().text_color();
    let mut job = egui::text::LayoutJob::default();
    job.append(cell, 0.0, egui::TextFormat::simple(font.clone(), strong));
    if let Some(lib) = lib {
        job.append(lib, 6.0, egui::TextFormat::simple(font, Color32::from_gray(120)));
    }
    job
}

fn hover_text(e: &ViewEntry) -> String {
    let kind = if e.is_root { "top cell" } else { "subcelda" };
    match e.size {
        Some((w, h)) => format!("{}\n{kind} · {w:.3} × {h:.3} µm", e.id),
        None => format!("{}\n{kind} · sin geometría", e.id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, is_root: bool) -> ViewEntry {
        ViewEntry { id: id.into(), is_root, size: None }
    }

    fn lib() -> Vec<ViewEntry> {
        vec![
            entry("sky130_fd_sc_hd__inv_1", true),
            entry("sky130_fd_sc_hd__INV_4", true),
            entry("sky130_fd_sc_hd__nand2_1", true),
            entry("sky130_fd_pr__inv_core", false),
        ]
    }

    #[test]
    fn filter_is_case_insensitive_and_respects_roots() {
        assert_eq!(filter_entries(&lib(), "inv", true), vec![0, 1]);
        assert_eq!(filter_entries(&lib(), " INV ", false), vec![0, 1, 3]);
        assert_eq!(filter_entries(&lib(), "", true), vec![0, 1, 2]);
        assert!(filter_entries(&lib(), "xor", false).is_empty());
    }

    #[test]
    fn split_name_separates_library_prefix() {
        assert_eq!(split_name("sky130_fd_sc_hd__inv_1"), ("inv_1", Some("sky130_fd_sc_hd")));
        assert_eq!(split_name("sky130_ef_sc_hd__decap_12"), ("decap_12", Some("sky130_ef_sc_hd")));
        assert_eq!(split_name("TOP"), ("TOP", None));
        assert_eq!(split_name("__x"), ("__x", None));
    }

    #[test]
    fn hover_text_shows_kind_and_size() {
        let mut e = entry("sky130_fd_sc_hd__inv_1", true);
        e.size = Some((1.38, 3.2));
        assert_eq!(hover_text(&e), "sky130_fd_sc_hd__inv_1\ntop cell · 1.380 × 3.200 µm");
        assert!(hover_text(&entry("X", false)).contains("subcelda · sin geometría"));
    }
}
