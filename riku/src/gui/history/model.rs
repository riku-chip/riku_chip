//! Estado del panel **History**, sin egui: qué commits hay, cuál está
//! elegido, qué páginas se cargaron, el detalle de cada commit y qué pidió
//! abrir el usuario. La vista (`super`) lo dibuja y `app.rs` atiende los
//! pedidos; así las transiciones se prueban sin ventana.

use std::collections::HashMap;

use crate::core::analysis::log::{LogCommit, LogReport};
use crate::core::analysis::show::ShowReport;
use crate::core::analysis::summary::{DetailLevel, FileSummary, SummaryCategory};
use crate::core::domain::git_types::ChangeStatus;

/// Commits por página.
pub const PAGE: usize = 200;

/// Lo que el panel le pide a la app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Abrir el diff de `path` entre el primer padre (`None`: el commit
    /// inicial, se compara contra vacío) y `commit`.
    OpenDiff { parent: Option<String>, commit: String, path: String },
}

/// Un archivo del commit elegido.
#[derive(Clone, Debug)]
pub struct DetailFile {
    pub path: String,
    pub status: Option<ChangeStatus>,
    /// Resumen del módulo del formato; `None` si ningún módulo lo reconoce
    /// (se lista, pero no se puede abrir).
    pub summary: Option<FileSummary>,
}

impl DetailFile {
    /// Se puede abrir en el visor (tiene módulo y algo que comparar).
    pub fn openable(&self) -> bool {
        self.summary.as_ref().is_some_and(|s| !matches!(s.category, SummaryCategory::Unknown | SummaryCategory::Error))
    }
}

/// Todos los archivos que cambió un commit respecto a su primer padre (lo
/// que da `riku show`, también en merges y archivos sin módulo).
#[derive(Clone, Debug)]
pub struct Details {
    pub parent: Option<String>,
    pub files: Vec<DetailFile>,
}

impl Details {
    pub fn from_show(report: &ShowReport) -> Self {
        let files = report
            .files
            .iter()
            .map(|f| DetailFile {
                path: f.path.clone(),
                status: f.status.clone(),
                summary: f.change.as_ref().map(|c| FileSummary::from_report_with(c, &f.path, DetailLevel::Detalle)),
            })
            .collect();
        Self { parent: report.parent().map(str::to_string), files }
    }
}

/// Qué lista recibe ↑/↓ y Enter con el teclado.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    /// La lista de commits.
    #[default]
    Commits,
    /// Los archivos del commit elegido (Tab).
    Files,
}

#[derive(Debug)]
pub struct HistoryModel {
    pub commits: Vec<LogCommit>,
    /// Llegaron los resúmenes por archivo (la segunda fase de la carga).
    pub summaries_ready: bool,
    pub selected: Option<usize>,
    pub focus: Focus,
    /// Archivo marcado (índice en `Details::files` del commit elegido); solo con `Focus::Files`.
    pub file_selected: Option<usize>,
    /// Páginas pedidas: se cargan `pages × PAGE` commits.
    pub pages: usize,
    /// Glob de archivos (`*.gds`); vacío = todos.
    pub filter: String,
    /// Detalle por oid (en cache: volver a un commit no recarga).
    pub details: HashMap<String, Result<Details, String>>,
    requests: Vec<Request>,
}

impl Default for HistoryModel {
    fn default() -> Self {
        Self {
            commits: Vec::new(),
            summaries_ready: false,
            selected: None,
            focus: Focus::Commits,
            file_selected: None,
            pages: 1,
            filter: String::new(),
            details: HashMap::new(),
            requests: Vec::new(),
        }
    }
}

impl HistoryModel {
    pub fn limit(&self) -> usize {
        self.pages * PAGE
    }

    /// Hay más commits que los cargados (la última página vino llena).
    pub fn has_more(&self) -> bool {
        self.commits.len() >= self.limit()
    }

    pub fn load_more(&mut self) {
        self.pages += 1;
    }

    /// Cambia el filtro; vuelve a la primera página.
    pub fn set_filter(&mut self, filter: &str) {
        self.filter = filter.trim().to_string();
        self.pages = 1;
    }

    /// Los commits con su grafo (primera fase). Conserva la selección si el
    /// commit sigue en la lista.
    pub fn set_graph(&mut self, report: LogReport) {
        let keep = self.selected_commit().map(|c| c.info.oid.clone());
        self.commits = report.commits;
        self.summaries_ready = false;
        self.selected = keep.and_then(|oid| self.commits.iter().position(|c| c.info.oid == oid));
        // Si el commit sigue, sus archivos y el marcado también.
        if self.selected.is_none() {
            self.reset_focus();
        }
    }

    /// Los resúmenes por archivo (segunda fase), por oid.
    pub fn set_summaries(&mut self, report: LogReport) {
        let mut by_oid: HashMap<String, LogCommit> = report.commits.into_iter().map(|c| (c.info.oid.clone(), c)).collect();
        for c in &mut self.commits {
            if let Some(full) = by_oid.remove(&c.info.oid) {
                c.files = full.files;
            }
        }
        self.summaries_ready = true;
    }

    pub fn selected_commit(&self) -> Option<&LogCommit> {
        self.commits.get(self.selected?)
    }

    pub fn select(&mut self, index: usize) {
        if index < self.commits.len() {
            if self.selected != Some(index) {
                self.reset_focus();
            }
            self.selected = Some(index);
        }
    }

    /// ↑ (−1) / ↓ (+1). Sin selección, ↓ elige el primero.
    pub fn move_selection(&mut self, delta: i64) {
        if self.commits.is_empty() {
            return;
        }
        let last = self.commits.len() as i64 - 1;
        let next = match self.selected {
            Some(i) => (i as i64 + delta).clamp(0, last),
            None => 0,
        };
        if self.selected != Some(next as usize) {
            self.reset_focus();
        }
        self.selected = Some(next as usize);
    }

    /// Los archivos del commit elegido, si ya llegó su detalle.
    fn selected_files(&self) -> Option<&[DetailFile]> {
        match self.details.get(&self.selected_commit()?.info.oid) {
            Some(Ok(d)) => Some(&d.files),
            _ => None,
        }
    }

    fn reset_focus(&mut self) {
        self.focus = Focus::Commits;
        self.file_selected = None;
    }

    /// Tab: de los commits a los archivos del commit elegido (marca el primero
    /// que se puede abrir) y de vuelta. Sin archivos abribles no hace nada.
    pub fn toggle_focus(&mut self) {
        match self.focus {
            Focus::Files => self.reset_focus(),
            Focus::Commits => {
                if let Some(i) = self.selected_files().and_then(|fs| fs.iter().position(DetailFile::openable)) {
                    self.focus = Focus::Files;
                    self.file_selected = Some(i);
                }
            }
        }
    }

    /// ↑ (−1) / ↓ (+1) en los archivos: salta los que no se pueden abrir y se
    /// detiene en los extremos, sin dar la vuelta.
    pub fn move_file(&mut self, delta: i64) {
        if self.focus != Focus::Files {
            return;
        }
        let (Some(files), Some(mut cur)) = (self.selected_files(), self.file_selected) else { return };
        for _ in 0..delta.unsigned_abs() {
            let next = if delta > 0 {
                (cur + 1..files.len()).find(|&i| files[i].openable())
            } else {
                (0..cur).rev().find(|&i| files[i].openable())
            };
            match next {
                Some(i) => cur = i,
                None => break,
            }
        }
        self.file_selected = Some(cur);
    }

    /// Enter con el foco en los archivos: abre el marcado.
    pub fn open_marked(&mut self) {
        if self.focus != Focus::Files {
            return;
        }
        let path = self.file_selected.and_then(|i| self.selected_files()?.get(i)).map(|f| f.path.clone());
        if let Some(p) = path {
            self.open(&p);
        }
    }

    pub fn set_details(&mut self, oid: String, details: Result<Details, String>) {
        self.details.insert(oid, details);
    }

    /// Pide abrir `path` del commit elegido contra su primer padre.
    pub fn open(&mut self, path: &str) {
        let Some(c) = self.selected_commit() else { return };
        let parent = match self.details.get(&c.info.oid) {
            Some(Ok(d)) => d.parent.clone(),
            _ => c.parents.first().cloned(),
        };
        let req = Request::OpenDiff { parent, commit: c.info.oid.clone(), path: path.to_string() };
        self.requests.push(req);
    }

    /// Abre el primer archivo del commit elegido que tenga cambios y se pueda
    /// abrir (Enter o doble clic).
    pub fn open_first(&mut self) {
        let Some(c) = self.selected_commit() else { return };
        let path = match self.details.get(&c.info.oid) {
            Some(Ok(d)) => d.files.iter().find(|f| f.openable()).map(|f| f.path.clone()),
            _ => c.files.first().map(|f| f.path.clone()),
        };
        if let Some(p) = path {
            self.open(&p);
        }
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::analysis::graph::{layout, tests::dag};
    use crate::core::domain::git_types::CommitInfo;

    fn report(spec: &str, with_files: bool) -> LogReport {
        let d = dag(spec);
        let commits = d
            .iter()
            .zip(layout(&d))
            .map(|((oid, parents), row)| LogCommit {
                info: CommitInfo {
                    oid: oid.clone(),
                    short_id: oid.clone(),
                    message: oid.clone(),
                    author: "t".into(),
                    timestamp: 0,
                },
                parents: parents.clone(),
                refs: Vec::new(),
                is_merge: parents.len() > 1,
                files: if with_files { vec![FileSummary::unknown(&format!("{oid}.sch"))] } else { Vec::new() },
                graph: Some(row),
                lvs: Vec::new(),
            })
            .collect();
        LogReport { commits, warnings: Vec::new() }
    }

    #[test]
    fn summaries_arrive_after_the_graph_and_keep_the_selection() {
        let mut m = HistoryModel::default();
        m.set_graph(report("c:b, b:a, a:", false));
        m.select(1);
        assert!(!m.summaries_ready && m.commits[1].files.is_empty());
        m.set_summaries(report("c:b, b:a, a:", true));
        assert!(m.summaries_ready);
        assert_eq!(m.commits[1].files[0].path, "b.sch");
        // Recargar (otro filtro, otra página) conserva el commit elegido.
        m.set_graph(report("d:c, c:b, b:a, a:", false));
        assert_eq!(m.selected_commit().unwrap().info.oid, "b");
    }

    #[test]
    fn keyboard_moves_the_selection_within_bounds() {
        let mut m = HistoryModel::default();
        m.move_selection(1);
        assert_eq!(m.selected, None);
        m.set_graph(report("c:b, b:a, a:", false));
        m.move_selection(1);
        assert_eq!(m.selected, Some(0));
        m.move_selection(5);
        assert_eq!(m.selected, Some(2));
        m.move_selection(-9);
        assert_eq!(m.selected, Some(0));
    }

    #[test]
    fn opening_a_file_asks_for_the_diff_against_the_first_parent() {
        let mut m = HistoryModel::default();
        m.set_graph(report("m:a f, a:r, f:r, r:", true));
        m.select(0);
        m.open("x.gds");
        assert_eq!(
            m.take_requests(),
            vec![Request::OpenDiff { parent: Some("a".into()), commit: "m".into(), path: "x.gds".into() }]
        );
        // El commit inicial se compara contra vacío.
        m.select(3);
        m.open_first();
        assert_eq!(m.take_requests(), vec![Request::OpenDiff { parent: None, commit: "r".into(), path: "r.sch".into() }]);
        assert!(m.take_requests().is_empty());
    }

    /// Detalle del commit `oid` con estos archivos: `(ruta, se puede abrir)`.
    fn with_files(m: &mut HistoryModel, oid: &str, files: &[(&str, bool)]) {
        let files = files
            .iter()
            .map(|&(path, openable)| {
                let mut summary = FileSummary::unknown(path);
                if openable {
                    summary.category = SummaryCategory::Semantic;
                }
                DetailFile { path: path.into(), status: None, summary: Some(summary) }
            })
            .collect();
        m.set_details(oid.into(), Ok(Details { parent: None, files }));
    }

    #[test]
    fn tab_needs_an_openable_file_to_move_to_the_files() {
        let mut m = HistoryModel::default();
        m.set_graph(report("b:a, a:", false));
        m.select(0);
        m.toggle_focus();
        assert_eq!(m.focus, Focus::Commits, "sin el detalle cargado");
        with_files(&mut m, "b", &[("x.txt", false)]);
        m.toggle_focus();
        assert_eq!(m.focus, Focus::Commits, "ningún archivo se puede abrir");
        with_files(&mut m, "b", &[("x.txt", false), ("top.gds", true), ("amp.sch", true)]);
        m.toggle_focus();
        assert_eq!((m.focus, m.file_selected), (Focus::Files, Some(1)), "el primero abrible");
        m.toggle_focus();
        assert_eq!((m.focus, m.file_selected), (Focus::Commits, None), "Tab otra vez vuelve");
    }

    #[test]
    fn up_and_down_move_over_the_openable_files_and_stop_at_the_ends() {
        let mut m = HistoryModel::default();
        m.set_graph(report("b:a, a:", false));
        m.select(0);
        with_files(&mut m, "b", &[("a.gds", true), ("b.txt", false), ("c.sch", true), ("d.txt", false)]);
        m.move_file(1);
        assert_eq!(m.file_selected, None, "con el foco en los commits no hace nada");
        m.toggle_focus();
        assert_eq!(m.file_selected, Some(0));
        m.move_file(1);
        assert_eq!(m.file_selected, Some(2), "salta el que no se abre");
        m.move_file(1);
        assert_eq!(m.file_selected, Some(2), "tope abajo: no da la vuelta");
        m.move_file(-1);
        assert_eq!(m.file_selected, Some(0));
        m.move_file(-4);
        assert_eq!(m.file_selected, Some(0), "tope arriba");
        m.move_file(9);
        assert_eq!(m.file_selected, Some(2), "varias pulsaciones en un cuadro");
    }

    #[test]
    fn enter_on_the_files_opens_the_marked_one() {
        let mut m = HistoryModel::default();
        m.set_graph(report("b:a, a:", false));
        m.select(0);
        with_files(&mut m, "b", &[("a.gds", true), ("c.sch", true)]);
        m.open_marked();
        assert!(m.take_requests().is_empty(), "con el foco en los commits Enter es open_first");
        m.toggle_focus();
        m.move_file(1);
        m.open_marked();
        assert_eq!(m.take_requests(), vec![Request::OpenDiff { parent: None, commit: "b".into(), path: "c.sch".into() }]);
    }

    #[test]
    fn changing_commit_returns_the_focus_to_the_commits() {
        let mut m = HistoryModel::default();
        m.set_graph(report("c:b, b:a, a:", false));
        m.select(0);
        with_files(&mut m, "c", &[("a.gds", true)]);
        m.toggle_focus();
        assert_eq!(m.focus, Focus::Files);
        // Recargar la lista con el mismo commit elegido no mueve el foco.
        m.set_graph(report("d:c, c:b, b:a, a:", false));
        assert_eq!((m.focus, m.file_selected), (Focus::Files, Some(0)));
        // Otro commit sí.
        m.select(2);
        assert_eq!((m.focus, m.file_selected), (Focus::Commits, None));
        // Con el foco en los archivos de `c`, ↓ de commits (otra tecla de la app) lo suelta.
        m.select(1);
        m.toggle_focus();
        assert_eq!(m.focus, Focus::Files);
        m.move_selection(1);
        assert_eq!((m.focus, m.file_selected), (Focus::Commits, None));
        // Si el commit elegido desaparece de la lista, tampoco queda el foco.
        m.select(1);
        m.toggle_focus();
        assert_eq!(m.focus, Focus::Files);
        m.set_graph(report("z:y, y:", false));
        assert_eq!((m.selected, m.focus), (None, Focus::Commits));
    }

    #[test]
    fn pages_and_filter() {
        let mut m = HistoryModel::default();
        assert_eq!(m.limit(), PAGE);
        m.load_more();
        assert_eq!(m.limit(), 2 * PAGE);
        m.set_filter("  *.gds ");
        assert_eq!((m.filter.as_str(), m.pages), ("*.gds", 1));
        m.set_graph(report("b:a, a:", false));
        assert!(!m.has_more());
    }
}
