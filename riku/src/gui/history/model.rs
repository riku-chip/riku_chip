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

#[derive(Debug)]
pub struct HistoryModel {
    pub commits: Vec<LogCommit>,
    /// Llegaron los resúmenes por archivo (la segunda fase de la carga).
    pub summaries_ready: bool,
    pub selected: Option<usize>,
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
        self.selected = Some(next as usize);
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
                info: CommitInfo { oid: oid.clone(), short_id: oid.clone(), message: oid.clone(), author: "t".into(), timestamp: 0 },
                parents: parents.clone(),
                refs: Vec::new(),
                is_merge: parents.len() > 1,
                files: if with_files { vec![FileSummary::unknown(&format!("{oid}.sch"))] } else { Vec::new() },
                graph: Some(row),
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
