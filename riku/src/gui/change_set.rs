//! Diff de todo el repo en el visor (lo de `riku diff A B` sin archivo): la
//! lista de archivos que cambiaron entre dos versiones, al costado del
//! lienzo. Un clic abre el diff de ese archivo y la lista queda para pasar
//! al siguiente.
//!
//! La lista sale enseguida (solo lee Git); el resumen de cada archivo llega
//! después, de a uno, en un hilo: con layouts grandes, se puede hacer clic
//! desde el primer momento. Es el mismo flujo que `status` y `log`
//! (`diff_pair` + `FileSummary`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use eframe::egui::{self, RichText};

use crate::core::analysis::diff_pair::{diff_pair, End, OnError, Version, WORKTREE};
use crate::core::analysis::diff_set::{self, Side};
use crate::core::analysis::summary::{DetailLevel, FileSummary, SummaryCategory};
use crate::core::domain::git_types::ChangeStatus;
use crate::core::domain::ports::RepoRoot;
use crate::core::git::git_service::GitService;
use crate::gui::history::counts_text;
use crate::gui::theme::space;
use crate::gui::tr;

/// Un archivo que cambió.
pub(crate) struct Entry {
    pub path: String,
    pub status: ChangeStatus,
    /// Algún módulo lo sabe comparar (si no, se lista atenuado y sin clic).
    pub openable: bool,
    /// Llega después, del hilo; `None` mientras tanto.
    pub summary: Option<FileSummary>,
}

enum Msg {
    Listed(Vec<Entry>),
    Summary(usize, FileSummary),
    Failed(String),
}

/// Qué pidió el usuario en la lista.
pub(crate) enum Request {
    Open(String),
    Close,
}

pub(crate) struct ChangeSet {
    /// Raíz del repo que se compara.
    pub repo: PathBuf,
    /// Las dos versiones como las nombra el visor (`:worktree` es el disco).
    pub from: String,
    pub to: String,
    pub entries: Vec<Entry>,
    /// Aún no llegó la lista.
    pub listing: bool,
    pub error: Option<String>,
    pub selected: Option<usize>,
    rx: mpsc::Receiver<Msg>,
    cancel: Arc<AtomicBool>,
    scroll_to_selection: bool,
}

fn side(token: &str) -> Side {
    if token == WORKTREE { Side::WorkTree } else { Side::Rev(token.to_string()) }
}

impl ChangeSet {
    /// Empieza a listar y resumir lo que cambió de `from` a `to` en `repo`.
    pub(crate) fn start(repo: PathBuf, from: String, to: String, ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let (stop, ctx, a, b, root) = (cancel.clone(), ctx.clone(), from.clone(), to.clone(), repo.clone());
        std::thread::Builder::new()
            .name("riku-change-set".into())
            .spawn(move || {
                let send = |m| {
                    let _ = tx.send(m);
                    ctx.request_repaint();
                };
                if let Err(e) = work(&root, &a, &b, &stop, &send) {
                    send(Msg::Failed(e));
                }
            })
            .expect("hilo del diff de todo el repo");
        Self {
            repo,
            from,
            to,
            entries: Vec::new(),
            listing: true,
            error: None,
            selected: None,
            rx,
            cancel,
            scroll_to_selection: false,
        }
    }

    /// Recibe lo que llegó del hilo desde el último cuadro.
    pub(crate) fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Listed(entries) => {
                    self.entries = entries;
                    self.listing = false;
                }
                Msg::Summary(i, s) => {
                    if let Some(e) = self.entries.get_mut(i) {
                        e.summary = Some(s);
                    }
                }
                Msg::Failed(e) => {
                    self.listing = false;
                    self.error = Some(e);
                }
            }
        }
    }

    /// Resúmenes que faltan (para el contador "revisando…").
    pub(crate) fn pending(&self) -> usize {
        self.entries.iter().filter(|e| e.openable && e.summary.is_none()).count()
    }

    /// El archivo abrible siguiente (o anterior) al elegido; `None` si no hay.
    pub(crate) fn step(&mut self, delta: i64) -> Option<String> {
        let openable: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].openable).collect();
        if openable.is_empty() {
            return None;
        }
        let pos = self.selected.and_then(|s| openable.iter().position(|&i| i == s));
        let next = match pos {
            None if delta >= 0 => 0,
            None => openable.len() - 1,
            Some(p) => (p as i64 + delta).clamp(0, openable.len() as i64 - 1) as usize,
        };
        let i = openable[next];
        if Some(i) == self.selected {
            return None;
        }
        self.selected = Some(i);
        self.scroll_to_selection = true;
        Some(self.entries[i].path.clone())
    }

    /// La lista, con su título. Retorna lo que pidió el usuario.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui) -> Option<Request> {
        let mut req = None;
        let label = |t: &str| crate::gui::app::short_hash(t);
        ui.horizontal(|ui| {
            ui.heading(tr!("changes.title"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("×").on_hover_text(tr!("changes.close")).clicked() {
                    req = Some(Request::Close);
                }
            });
        });
        let pending = self.pending();
        let mut line = format!("{} → {} · {}", label(&self.from), label(&self.to), tr!("changes.count", count = self.entries.len()));
        if pending > 0 {
            line.push_str(&format!(" · {}", tr!("changes.checking", count = pending)));
        }
        ui.label(RichText::new(line).small().weak());
        ui.add_space(space::XS);

        if self.listing {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tr!("changes.listing")).weak());
            });
            return req;
        }
        if let Some(e) = &self.error {
            ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
            return req;
        }
        if self.entries.is_empty() {
            ui.label(RichText::new(tr!("changes.empty")).weak());
            return req;
        }

        let row_h = 2.0 * ui.spacing().interact_size.y;
        let max_h = (ui.available_height() * 0.45).max(3.0 * row_h);
        let scroll_to = std::mem::take(&mut self.scroll_to_selection).then_some(self.selected).flatten();
        egui::ScrollArea::vertical()
            .id_salt("change_set")
            .max_height(max_h)
            .auto_shrink([false, true])
            .show_rows(ui, row_h, self.entries.len(), |ui, range| {
                for i in range {
                    let e = &self.entries[i];
                    let selected = self.selected == Some(i);
                    let resp = row(ui, e, selected, row_h);
                    if scroll_to == Some(i) {
                        resp.scroll_to_me(Some(egui::Align::Center));
                    }
                    if e.openable && resp.clicked() {
                        self.selected = Some(i);
                        req = Some(Request::Open(e.path.clone()));
                    }
                }
            });
        req
    }
}

impl Drop for ChangeSet {
    /// Al cerrar la lista (o abrir otra), el hilo deja de resumir.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Una fila: estado (A/M/D/R), ruta y el resumen (o "…" mientras llega).
fn row(ui: &mut egui::Ui, e: &Entry, selected: bool, h: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
    let v = ui.visuals();
    if selected {
        ui.painter().rect_filled(rect, 4.0, v.selection.bg_fill);
    } else if resp.hovered() && e.openable {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let dark = v.dark_mode;
    let (letter, kind) = match e.status {
        ChangeStatus::Added => ("A", Some(viewer_core::diff::ChangeKind::Added)),
        ChangeStatus::Removed => ("D", Some(viewer_core::diff::ChangeKind::Removed)),
        ChangeStatus::Modified => ("M", Some(viewer_core::diff::ChangeKind::Modified)),
        ChangeStatus::Renamed => ("R", None),
    };
    let color = kind.map_or(v.text_color(), |k| crate::gui::theme::change_color(k, dark));
    let dim = |c: egui::Color32| if e.openable { c } else { c.gamma_multiply(0.45) };
    let (what, what_color) = match &e.summary {
        _ if !e.openable => (tr!("home.no_module"), v.weak_text_color()),
        None => ("…".to_string(), v.weak_text_color()),
        Some(s) => match s.category {
            SummaryCategory::Semantic => (counts_text(s), v.text_color()),
            SummaryCategory::Cosmetic => (tr!("history.cosmetic"), v.weak_text_color()),
            SummaryCategory::Unchanged => (tr!("home.no_semantic"), v.weak_text_color()),
            SummaryCategory::Unknown => (tr!("home.no_module"), v.weak_text_color()),
            SummaryCategory::Error => (s.errors.join("; "), v.error_fg_color),
        },
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let x0 = rect.left() + space::XS;
    let top = rect.top() + 2.0;
    let p = ui.painter_at(rect);
    p.text(egui::pos2(x0, top), egui::Align2::LEFT_TOP, letter, font.clone(), dim(color));
    let name_x = x0 + 16.0;
    p.text(egui::pos2(name_x, top), egui::Align2::LEFT_TOP, &e.path, font, dim(v.strong_text_color()));
    p.text(egui::pos2(name_x, rect.center().y + 1.0), egui::Align2::LEFT_TOP, what, small, dim(what_color));
    if e.openable {
        resp.on_hover_text(&e.path).on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp.on_hover_text(tr!("home.no_module"))
    }
}

/// El trabajo del hilo: la lista (solo Git) y después cada resumen.
fn work(
    repo: &std::path::Path,
    from: &str,
    to: &str,
    stop: &AtomicBool,
    send: &dyn Fn(Msg),
) -> Result<(), String> {
    let svc = GitService::open(repo).map_err(|e| e.to_string())?;
    let workdir = svc.root().map(std::path::Path::to_path_buf);
    let (a, b) = (side(from), side(to));
    let list = diff_set::changed_paths(&svc, workdir.as_deref(), &a, &b).map_err(|e| e.to_string())?;
    let modules = crate::modules::registry();
    let entries: Vec<Entry> = list
        .iter()
        .map(|(path, (status, _))| Entry {
            path: path.clone(),
            status: status.clone(),
            openable: modules.for_path(path).is_some(),
            summary: None,
        })
        .collect();
    send(Msg::Listed(entries));

    let opts = crate::core::config::options_for(repo, Default::default()).unwrap_or_default();
    for (i, (path, (status, old_path))) in list.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let Some(module) = modules.for_path(path) else { continue };
        // Un lado que no existe no se lee (archivo nuevo o borrado).
        let before = if *status == ChangeStatus::Added { Version::Absent } else { a.version() };
        let after = if *status == ChangeStatus::Removed { Version::Absent } else { b.version() };
        let before = End::new(before, old_path.as_deref().unwrap_or(path));
        let after = End::new(after, path);
        let summary = match diff_pair(&svc, workdir.as_deref(), module.as_ref(), before, after, &opts, OnError::InFile) {
            Ok(report) => FileSummary::from_report_with(&report, path, DetailLevel::Resumen),
            Err(e) => FileSummary::error(path, e.to_string()),
        };
        send(Msg::Summary(i, summary));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(openable: &[bool]) -> ChangeSet {
        let (_tx, rx) = mpsc::channel();
        ChangeSet {
            repo: PathBuf::new(),
            from: "a".into(),
            to: "b".into(),
            entries: openable
                .iter()
                .enumerate()
                .map(|(i, &o)| Entry { path: format!("f{i}"), status: ChangeStatus::Modified, openable: o, summary: None })
                .collect(),
            listing: false,
            error: None,
            selected: None,
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            scroll_to_selection: false,
        }
    }

    #[test]
    fn flechas_saltean_lo_que_no_se_abre_y_no_pasan_los_bordes() {
        let mut s = set(&[true, false, true]);
        assert_eq!(s.step(1).as_deref(), Some("f0"), "sin elección, la primera");
        assert_eq!(s.step(1).as_deref(), Some("f2"), "f1 no se abre");
        assert_eq!(s.step(1), None, "ya es la última");
        assert_eq!(s.step(-1).as_deref(), Some("f0"));
        assert_eq!(s.pending(), 2, "faltan los resúmenes de los que se abren");
        assert_eq!(set(&[false]).step(1), None);
    }

    #[test]
    fn cerrar_la_lista_cancela_el_hilo() {
        let s = set(&[true]);
        let stop = s.cancel.clone();
        drop(s);
        assert!(stop.load(Ordering::Relaxed));
    }
}
