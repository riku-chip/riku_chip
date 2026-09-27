//! Ejecutores de los comandos CLI.
//!
//! Cada función `run_*` implementa un comando concreto y no conoce al shell ni
//! al parser — solo recibe argumentos ya resueltos. Esto permite que tanto la
//! invocación directa (`riku diff ...`) como el REPL interno llamen al mismo
//! código sin duplicación.

use std::path::PathBuf;

use riku_kernel::DiffOptions;

use crate::core::analysis::commit_diff::analyze_diff_with_repo;
use crate::core::analysis::show::analyze_show;
use crate::core::domain::ports::GitRepository;
use crate::core::domain::models::FileChange;
use crate::core::git::git_service::GitService;
use crate::core::analysis::log;
use crate::core::analysis::status::{self, StatusOptions};
use crate::core::analysis::summary::DetailLevel;

use super::OutputFormat;
use super::format;
use super::gui;

// ─── Diff ────────────────────────────────────────────────────────────────────

/// `riku diff`: el mismo camino para todos los formatos. El driver se elige
/// en el registro (por extensión) y devuelve un `FileChange`; la CLI no sabe
/// qué formato es.
pub(super) fn run_diff(
    repo: PathBuf,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
    format: OutputFormat,
    cosmetic_threshold_um2: f64,
    use_cache: bool,
    expressions: Vec<String>,
) -> Result<Changes, String> {
    if matches!(format, OutputFormat::Visual) {
        return present_visual(&repo, commit_a, commit_b, file_path, &expressions).map(|_| Changes::Clean);
    }
    // Mismo flujo que log/status; el umbral cosmético y la cache los usa el
    // módulo de layouts, los demás los ignoran.
    let opts = DiffOptions { cosmetic_threshold: Some(cosmetic_threshold_um2), use_cache, expressions };
    let svc = GitService::open(&repo).map_err(|e| e.to_string())?;
    let mut report = analyze_diff_with_repo(&svc, commit_a, commit_b, file_path, &crate::modules::registry(), &opts)
        .map_err(|e| e.to_string())?;
    let warnings = std::mem::take(&mut report.warnings);
    print_diff(&report, &warnings, file_path, format)?;
    Ok(Changes::of(report.functional().next().is_some()))
}

/// Si un comando encontró cambios funcionales. Lo usan los códigos de salida
/// de `status` y de `diff`/`show --ci` (ver `cli::run`).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Changes {
    /// Sin cambios, o solo cosméticos.
    Clean,
    /// Al menos un cambio funcional.
    Functional,
}

impl Changes {
    fn of(functional: bool) -> Self {
        if functional { Changes::Functional } else { Changes::Clean }
    }
}

// ─── Show ────────────────────────────────────────────────────────────────────

/// `riku show`: los cambios de un commit respecto a su primer padre.
pub(super) fn run_show(
    repo: PathBuf,
    commit: &str,
    file_path: Option<&str>,
    format: OutputFormat,
    cosmetic_threshold_um2: f64,
    use_cache: bool,
    expressions: Vec<String>,
) -> Result<Changes, String> {
    let svc = GitService::open(&repo).map_err(|e| e.to_string())?;
    if matches!(format, OutputFormat::Visual) {
        let Some(file) = file_path else {
            return Err("show -f visual necesita un archivo: riku show <commit> <archivo> -f visual".into());
        };
        let changes = svc.commit_changes(commit).map_err(|e| e.to_string())?;
        let Some(parent) = changes.commit.parents.first() else {
            return Err(format!(
                "{commit} es el commit inicial: no hay versión anterior con la que comparar. Para verlo: riku open {file}"
            ));
        };
        return present_visual(&repo, parent, &changes.commit.info.oid, file, &expressions).map(|_| Changes::Clean);
    }
    if matches!(format, OutputFormat::JsonV1) {
        return Err("show no tiene salida json-v1; usa -f json (schema riku-show/v1)".into());
    }

    let opts = DiffOptions { cosmetic_threshold: Some(cosmetic_threshold_um2), use_cache, expressions };
    let report = analyze_show(&svc, commit, file_path, &crate::modules::registry(), &opts).map_err(|e| e.to_string())?;
    match format {
        OutputFormat::Json => format::show_json::print(&report, true)?,
        _ => format::show_text::print(&report)?,
    }
    Ok(Changes::of(report.has_functional_changes()))
}

/// Imprime un diff en el formato pedido (texto, JSON v2 o JSON v1).
fn print_diff(report: &FileChange, warnings: &[String], file_path: &str, format: OutputFormat) -> Result<(), String> {
    for w in warnings {
        eprintln!("[!] {w}");
    }
    match format {
        OutputFormat::Text => format::diff_text::print(report, file_path),
        OutputFormat::Json => format::diff_json::print(report, warnings, file_path),
        OutputFormat::JsonV1 => format::diff_json::print_v1(report, warnings, file_path),
        OutputFormat::Visual => unreachable!("el visor se abre antes de imprimir"),
    }
}

fn present_visual(
    repo: &PathBuf,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
    expressions: &[String],
) -> Result<(), String> {
    let repo_abs = repo.canonicalize().unwrap_or_else(|_| repo.clone());
    let mut extra_args: Vec<std::ffi::OsString> = vec![
        "--repo".into(),
        repo_abs.into_os_string(),
        "--commit-a".into(),
        commit_a.into(),
        "--commit-b".into(),
        commit_b.into(),
        file_path.into(),
    ];
    for e in expressions {
        extra_args.push("--expr".into());
        extra_args.push(e.into());
    }

    gui::run_with_args(extra_args)
}

// ─── Log ─────────────────────────────────────────────────────────────────────

pub(super) struct LogArgs {
    pub repo: PathBuf,
    pub file_path: Option<String>,
    pub limit: usize,
    pub json: bool,
    pub compact: bool,
    pub detail: bool,
    pub full: bool,
    pub paths: Vec<String>,
    pub branch: Option<String>,
}

pub(super) fn run_log(args: LogArgs) -> Result<(), String> {
    let level = DetailLevel::from_flags(args.detail, args.full);

    // El path posicional se mapea a un patrón exacto en `paths` (compatibilidad
    // con el comportamiento legado y atajo común).
    let mut paths = args.paths;
    if let Some(fp) = args.file_path {
        paths.push(fp);
    }

    let opts = log::LogOptions {
        level,
        paths,
        limit: Some(args.limit),
        start: args.branch,
    };
    let report = log::analyze_with_options_path(&args.repo, &opts, &crate::modules::registry()).map_err(|e| e.to_string())?;

    if args.json {
        format::log_json::print(&report, !args.compact)?;
    } else {
        format::log_text::print(&report, level);
    }
    Ok(())
}

// ─── Status ──────────────────────────────────────────────────────────────────

/// Argumentos de `run_status`, agrupados para mantener la firma estable a
/// medida que se añadan flags.
pub(super) struct StatusArgs {
    pub repo: PathBuf,
    pub include_unknown: bool,
    pub json: bool,
    pub compact: bool,
    pub detail: bool,
    pub full: bool,
    pub paths: Vec<String>,
}

pub(super) fn run_status(args: StatusArgs) -> Result<Changes, String> {
    let level = DetailLevel::from_flags(args.detail, args.full);

    let opts = StatusOptions {
        level,
        paths: args.paths,
    };
    let report = status::analyze_with_options_path(&args.repo, &opts, &crate::modules::registry()).map_err(|e| e.to_string())?;

    if args.json {
        format::status_json::print(&report, !args.compact)?;
    } else {
        format::status_text::print(&report, level, args.include_unknown);
    }

    Ok(Changes::of(report.has_semantic_changes()))
}

