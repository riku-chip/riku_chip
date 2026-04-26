//! Ejecutores de los comandos CLI.
//!
//! Cada función `run_*` implementa un comando concreto y no conoce al shell ni
//! al parser — solo recibe argumentos ya resueltos. Esto permite que tanto la
//! invocación directa (`riku diff ...`) como el REPL interno llamen al mismo
//! código sin duplicación.

use std::path::PathBuf;

use crate::adapters::registry::get_drivers;
use crate::adapters::xschem_driver::XschemDriver;
use crate::core::analysis::diff_view::DiffView;
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
) -> Result<(), String> {
    let driver = XschemDriver::new();
    let view = DiffView::from_commits(&repo, commit_a, commit_b, file_path, &driver, |b| {
        crate::adapters::xschem_driver::parse(b)
    })
    .map_err(|e| e.to_string())?;

    for w in &view.warnings {
        eprintln!("[!] {w}");
    }

    match format {
        OutputFormat::Text => format::diff_text::print(&view, file_path),
        OutputFormat::Json => format::diff_json::print(&view, file_path),
        OutputFormat::Visual => present_visual(&repo, commit_a, commit_b, file_path),
    }
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

// ─── Doctor ──────────────────────────────────────────────────────────────────

pub(super) fn run_doctor(repo: PathBuf) -> Result<(), String> {
    println!("\nRiku Doctor — Diagnóstico del Entorno\n");
    #[allow(unused_mut)]
    let mut any_error = false;

    println!("--- Repositorio Git ---");
    match git2::Repository::discover(&repo) {
        Ok(r) => println!("  [ok]  {}", r.workdir().unwrap_or(r.path()).display()),
        Err(_) => println!("  [!]  No detectado — diff/log no funcionarán"),
    }

    println!("\n--- PDK ---");
    let pdk_root = std::env::var("PDK_ROOT").ok();
    let pdk_name = std::env::var("PDK").ok();
    let tools = std::env::var("TOOLS").ok();

    let xschemrc_local = std::path::PathBuf::from(".xschemrc");
    let xschemrc_home = dirs::home_dir().map(|h| h.join(".xschemrc"));
    let xschemrc = if xschemrc_local.exists() {
        Some(xschemrc_local)
    } else {
        xschemrc_home.filter(|p| p.exists())
    };

    match &xschemrc {
        Some(p) => println!("  [ok]  .xschemrc: {}", p.display()),
        None => println!("  [--]  .xschemrc: no encontrado"),
    }

    match (&pdk_root, &pdk_name) {
        (Some(root), Some(pdk)) => {
            let sym = std::path::Path::new(root)
                .join(pdk)
                .join("libs.tech/xschem");
            if sym.exists() {
                println!("  [ok]  $PDK_ROOT/$PDK → {}", sym.display());
            } else {
                println!(
                    "  [!]  $PDK_ROOT/$PDK configurado pero ruta no encontrada: {}",
                    sym.display()
                );
            }
        }
        _ => println!("  [--]  $PDK_ROOT / $PDK: no configurados"),
    }

    match &tools {
        Some(t) => {
            let devices =
                std::path::Path::new(t).join("xschem/share/xschem/xschem_library/devices");
            if devices.exists() {
                println!("  [ok]  $TOOLS → {}", devices.display());
            } else {
                println!(
                    "  [!]  $TOOLS configurado pero devices no encontrado: {}",
                    devices.display()
                );
            }
        }
        None => println!("  [--]  $TOOLS: no configurado"),
    }

    let has_symbols = xschemrc.is_some()
        || pdk_root
            .as_ref()
            .zip(pdk_name.as_ref())
            .map(|(r, p)| {
                std::path::Path::new(r)
                    .join(p)
                    .join("libs.tech/xschem")
                    .exists()
            })
            .unwrap_or(false)
        || tools
            .as_ref()
            .map(|t| {
                std::path::Path::new(t)
                    .join("xschem/share/xschem/xschem_library/devices")
                    .exists()
            })
            .unwrap_or(false);

    if !has_symbols {
        println!(
            "  [!]  Sin fuente de símbolos — los componentes se renderizarán como cajas vacías"
        );
    }

    println!("\n--- Drivers ---");
    for driver in get_drivers() {
        let info = driver.info();
        let status = if info.available { "[ok]" } else { "[x]" };
        println!("  {status}  {:10} {}", info.name, info.version);
    }

    println!();
    if any_error {
        return Err("Se detectaron problemas críticos en el entorno.".to_string());
    }
    println!("Entorno listo.\n");
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

