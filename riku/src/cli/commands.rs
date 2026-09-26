//! Ejecutores de los comandos CLI.
//!
//! Cada función `run_*` implementa un comando concreto y no conoce al shell ni
//! al parser — solo recibe argumentos ya resueltos. Esto permite que tanto la
//! invocación directa (`riku diff ...`) como el REPL interno llamen al mismo
//! código sin duplicación.

use std::path::PathBuf;

use crate::adapters::registry::DriverConfig;
use crate::adapters::xschem_driver::XschemDriver;
use crate::core::analysis::diff_view::DiffView;
use crate::core::domain::models::FileChange;
use crate::core::analysis::log;
use crate::core::analysis::status::{self, StatusOptions};
use crate::core::analysis::summary::DetailLevel;

use super::OutputFormat;
use super::format;
use super::gui;

// ─── Diff ────────────────────────────────────────────────────────────────────

pub(super) fn run_diff(
    repo: PathBuf,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
    format: OutputFormat,
    cosmetic_threshold_um2: f64,
    use_cache: bool,
) -> Result<(), String> {
    // Ruta de layouts: diff geométrico con el umbral cosmético configurable.
    if is_gds_path(file_path) {
        return run_diff_gds(
            &repo,
            commit_a,
            commit_b,
            file_path,
            format,
            cosmetic_threshold_um2,
            use_cache,
        );
    }

    let driver = XschemDriver::new();
    let view = DiffView::from_commits(&repo, commit_a, commit_b, file_path, &driver, |b| {
        crate::adapters::xschem_driver::parse(b)
    })
    .map_err(|e| e.to_string())?;

    match format {
        OutputFormat::Visual => present_visual(&repo, commit_a, commit_b, file_path),
        other => print_diff(&view.report, &view.warnings, file_path, other),
    }
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

/// Layouts (GDSII u OASIS): diff geometrico en lugar del semantico.
fn is_gds_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".gds") || p.ends_with(".oas")
}

/// Variante de `run_diff` para archivos `.gds`. La GUI sigue usando el flujo
/// `present_visual` (que delega a riku-gui via args); aqui solo cubrimos las
/// salidas de texto y JSON, directamente desde el `FileChange` del driver.
fn run_diff_gds(
    repo: &PathBuf,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
    format: OutputFormat,
    cosmetic_threshold_um2: f64,
    use_cache: bool,
) -> Result<(), String> {
    if matches!(format, OutputFormat::Visual) {
        return present_visual(repo, commit_a, commit_b, file_path);
    }

    // Reusa analyze_diff_with_config (mismo flujo que log/status), inyectando
    // el threshold via DriverConfig. Asi GdsDriver clasifica `cosmetic` con
    // el valor del flag en vez del default 0.01.
    use crate::core::analysis::commit_diff::analyze_diff_with_config;
    use crate::core::git::git_service::GitService;
    let cfg = DriverConfig {
        cosmetic_threshold_um2,
        use_cache,
    };
    let svc = GitService::open(repo).map_err(|e| e.to_string())?;
    let mut report = analyze_diff_with_config(&svc, commit_a, commit_b, file_path, &cfg)
        .map_err(|e| e.to_string())?;
    let warnings = std::mem::take(&mut report.warnings);
    print_diff(&report, &warnings, file_path, format)
}

fn present_visual(
    repo: &PathBuf,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
) -> Result<(), String> {
    let repo_abs = repo.canonicalize().unwrap_or_else(|_| repo.clone());
    let extra_args: Vec<std::ffi::OsString> = vec![
        "--repo".into(),
        repo_abs.into_os_string(),
        "--commit-a".into(),
        commit_a.into(),
        "--commit-b".into(),
        commit_b.into(),
        file_path.into(),
    ];

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
    let report = log::analyze_with_options_path(&args.repo, &opts).map_err(|e| e.to_string())?;

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

/// Resultado funcional de `riku status`. Mapeo a exit codes en `cli::run`.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum StatusOutcome {
    /// Sin cambios semánticos.
    Clean,
    /// Hay al menos un cambio semántico.
    Dirty,
}

pub(super) fn run_status(args: StatusArgs) -> Result<StatusOutcome, String> {
    let level = DetailLevel::from_flags(args.detail, args.full);

    let opts = StatusOptions {
        level,
        paths: args.paths,
    };
    let report = status::analyze_with_options_path(&args.repo, &opts).map_err(|e| e.to_string())?;

    if args.json {
        format::status_json::print(&report, !args.compact)?;
    } else {
        format::status_text::print(&report, level, args.include_unknown);
    }

    Ok(if report.has_semantic_changes() {
        StatusOutcome::Dirty
    } else {
        StatusOutcome::Clean
    })
}

