use std::path::Path;

use thiserror::Error;

use crate::core::driver::RikuDriver;
use crate::core::git::git_service::{GitError, GitService};
use crate::core::models::{ChangeKind, ComponentDiff, DiffReport, Schematic};
use crate::core::ports::GitRepository;
use crate::core::rendering::styles::{AnnotationStyle, annotation_style};

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum DiffViewError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("no se pudo renderizar: {0}")]
    Render(String),
}

// ─── DiffView ────────────────────────────────────────────────────────────────

/// Vista de diff entre dos commits para un archivo de schematic.
///
/// Contiene todo lo necesario para que cualquier backend (CLI HTML, GUI egui)
/// presente el diff visualmente sin necesidad de re-parsear ni re-renderizar.
pub struct DiffView {
    /// SVG del estado anterior (commit_a), o None si el archivo es nuevo.
    pub svg_a: Option<String>,
    /// SVG del estado posterior (commit_b).
    pub svg_b: String,
    /// Schematic parseado del estado anterior.
    pub sch_a: Option<Schematic>,
    /// Schematic parseado del estado posterior.
    pub sch_b: Schematic,
    /// Reporte de diferencias semánticas.
    pub report: DiffReport,
    /// Advertencias generadas durante el análisis.
    pub warnings: Vec<String>,
}

impl DiffView {
    /// Construye un `DiffView` leyendo blobs de Git y delegando render y diff al driver.
    ///
    /// `commit_a` es el estado anterior, `commit_b` el posterior.
    /// Si el archivo no existe en `commit_a` (archivo nuevo), `svg_a` y `sch_a` son `None`.
    pub fn from_commits(
        repo_path: &Path,
        commit_a: &str,
        commit_b: &str,
        file_path: &str,
        driver: &dyn RikuDriver,
        parse_fn: impl Fn(&[u8]) -> Schematic,
    ) -> Result<Self, DiffViewError> {
        let svc = GitService::open(repo_path)?;
        Self::from_repo(&svc, commit_a, commit_b, file_path, driver, parse_fn)
    }

    /// Versión con repositorio y driver inyectados — facilita testing sin disco.
    pub fn from_repo<R: GitRepository + ?Sized>(
        repo: &R,
        commit_a: &str,
        commit_b: &str,
        file_path: &str,
        driver: &dyn RikuDriver,
        parse_fn: impl Fn(&[u8]) -> Schematic,
    ) -> Result<Self, DiffViewError> {
        let mut warnings = Vec::new();

        // ── Commit B (requerido) ──────────────────────────────────────────
        let content_b = repo.get_blob(commit_b, file_path).map_err(DiffViewError::Git)?;
        let sch_b = parse_fn(&content_b);
        let svg_b = driver
            .render(&content_b, file_path)
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .ok_or_else(|| DiffViewError::Render(format!("{file_path} (commit {commit_b})")))?;

        // ── Commit A (opcional — puede no existir si el archivo es nuevo) ─
        let (svg_a, sch_a, content_a) = match repo.get_blob(commit_a, file_path) {
            Ok(bytes) => {
                let sch = parse_fn(&bytes);
                let svg = driver
                    .render(&bytes, file_path)
                    .and_then(|p| std::fs::read_to_string(&p).ok());
                (svg, Some(sch), Some(bytes))
            }
            Err(GitError::BlobNotFound { .. }) => (None, None, None),
            Err(GitError::LargeBlob { path, size }) => {
                warnings.push(format!("{path} ({size} bytes) es demasiado grande; omitiendo."));
                (None, None, None)
            }
            Err(e) => return Err(DiffViewError::Git(e)),
        };

        // ── Diff semántico ────────────────────────────────────────────────
        let driver_report = driver.diff(
            content_a.as_deref().unwrap_or(&[]),
            &content_b,
            file_path,
        );
        warnings.extend(driver_report.warnings);
        let report = driver_report_to_diff_report(&driver_report.changes);

        Ok(Self { svg_a, svg_b, sch_a, sch_b, report, warnings })
    }
}

// ─── Conversión de tipos ──────────────────────────────────────────────────────

/// Convierte las entradas del driver a `DiffReport` de dominio.
/// Separa componentes, nets y el flag is_move_all.
pub fn driver_report_to_diff_report(
    changes: &[crate::core::driver::DiffEntry],
) -> DiffReport {
    DiffReport {
        components: changes
            .iter()
            .filter(|c| !c.element.starts_with("net:") && c.element != "layout")
            .map(|c| ComponentDiff {
                name: c.element.clone(),
                kind: c.kind.clone(),
                cosmetic: c.cosmetic,
                position_changed: c.position_changed,
                before: c.before.clone(),
                after: c.after.clone(),
            })
            .collect(),
        nets_added: changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Added && c.element.starts_with("net:"))
            .map(|c| c.element.trim_start_matches("net:").to_string())
            .collect(),
        nets_removed: changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Removed && c.element.starts_with("net:"))
            .map(|c| c.element.trim_start_matches("net:").to_string())
            .collect(),
        is_move_all: changes
            .iter()
            .any(|c| c.element == "layout" && c.cosmetic),
    }
}

/// Cuenta cambios no cosméticos por tipo. Útil para el log --semantic.
pub fn summarize_changes(changes: &[crate::core::driver::DiffEntry]) -> (usize, usize, usize) {
    let added = changes.iter().filter(|c| c.kind == ChangeKind::Added && !c.cosmetic).count();
    let removed = changes.iter().filter(|c| c.kind == ChangeKind::Removed && !c.cosmetic).count();
    let modified = changes.iter().filter(|c| c.kind == ChangeKind::Modified && !c.cosmetic).count();
    (added, removed, modified)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::driver::{DiffEntry, DriverDiffReport};
    use crate::core::models::FileFormat;

    fn make_report(changes: Vec<DiffEntry>) -> DriverDiffReport {
        DriverDiffReport { file_type: FileFormat::Xschem, changes, ..Default::default() }
    }

    #[test]
    fn convierte_componentes_y_nets() {
        let report = make_report(vec![
            DiffEntry {
                kind: ChangeKind::Added,
                element: "R1".to_string(),
                before: None,
                after: Some([("value".to_string(), "10k".to_string())].into()),
                cosmetic: false,
                position_changed: false,
            },
            DiffEntry {
                kind: ChangeKind::Added,
                element: "net:Vdd".to_string(),
                before: None,
                after: None,
                cosmetic: false,
                position_changed: false,
            },
            DiffEntry {
                kind: ChangeKind::Modified,
                element: "layout".to_string(),
                before: None,
                after: None,
                cosmetic: true,
                position_changed: false,
            },
        ]);

        let diff = driver_report_to_diff_report(&report.changes);
        assert_eq!(diff.components.len(), 1);
        assert_eq!(diff.components[0].name, "R1");
        assert_eq!(diff.nets_added, vec!["Vdd"]);
        assert!(diff.is_move_all);
    }

    #[test]
    fn summarize_ignora_cosmeticos() {
        let report = make_report(vec![
            DiffEntry { kind: ChangeKind::Added, element: "R1".to_string(), before: None, after: None, cosmetic: true, position_changed: false },
            DiffEntry { kind: ChangeKind::Removed, element: "C1".to_string(), before: None, after: None, cosmetic: false, position_changed: false },
            DiffEntry { kind: ChangeKind::Modified, element: "M1".to_string(), before: None, after: None, cosmetic: false, position_changed: false },
        ]);
        assert_eq!(summarize_changes(&report.changes), (0, 1, 1));
    }

    #[test]
    fn annotation_style_added_no_es_cosmetico() {
        let s = annotation_style(&ChangeKind::Added, false);
        assert!(s.fill.contains("0,200,0"));
        assert!(s.stroke.contains("0,200,0"));
    }

    #[test]
    fn annotation_style_modified_cosmetico_es_gris() {
        let s = annotation_style(&ChangeKind::Modified, true);
        assert!(s.fill.contains("120,120,120"));
    }
}
