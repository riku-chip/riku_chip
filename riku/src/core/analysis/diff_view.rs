use std::path::Path;

use thiserror::Error;

use crate::core::analysis::blob_io;
use crate::core::domain::driver::RikuDriver;
use crate::core::domain::git_types::GitError;
use crate::core::domain::models::FileChange;
use crate::core::domain::ports::GitRepository;
use crate::core::git::git_service::GitService;

// Esquemático parseado de Xschem: DiffView todavía es propia de ese formato
// (pasa al módulo de Xschem en la fase 2 de la migración).
use xschem_viewer::semantic::SemanticSchematic as Schematic;

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
    /// Cambios entre las dos versiones.
    pub report: FileChange,
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
        let content_b = repo
            .get_blob(commit_b, file_path)
            .map_err(DiffViewError::Git)?;
        let sch_b = parse_fn(&content_b);
        let svg_b = driver
            .render(&content_b, file_path)
            .ok_or_else(|| DiffViewError::Render(format!("{file_path} (commit {commit_b})")))?;

        // ── Commit A (opcional — puede no existir si el archivo es nuevo) ─
        let bytes_a = blob_io::read_blob_lenient(repo, commit_a, file_path, &mut warnings)
            .map_err(DiffViewError::Git)?;
        let (svg_a, sch_a, content_a) = match bytes_a {
            Some(bytes) => {
                let sch = parse_fn(&bytes);
                let svg = driver.render(&bytes, file_path);
                (svg, Some(sch), Some(bytes))
            }
            None => (None, None, None),
        };

        // ── Diff semántico ────────────────────────────────────────────────
        let mut report = driver.diff(content_a.as_deref().unwrap_or(&[]), &content_b, file_path);
        warnings.append(&mut report.warnings);

        Ok(Self {
            svg_a,
            svg_b,
            sch_a,
            sch_b,
            report,
            warnings,
        })
    }
}

