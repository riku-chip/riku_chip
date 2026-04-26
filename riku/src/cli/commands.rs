//! Ejecutores de los comandos CLI.
//!
//! Cada función `run_*` implementa un comando concreto y no conoce al shell ni
//! al parser — solo recibe argumentos ya resueltos. Esto permite que tanto la
//! invocación directa (`riku diff ...`) como el REPL interno llamen al mismo
//! código sin duplicación.

use std::path::PathBuf;
use std::process::Command;

use serde_json::json;

use crate::adapters::xschem_driver::XschemDriver;
use crate::core::analysis::diff_view::DiffView;
use crate::core::analysis::log;
use crate::core::analysis::status::{self, StatusOptions};
use crate::core::analysis::summary::DetailLevel;
use crate::core::domain::models::ChangeKind;
use crate::adapters::registry::get_drivers;

use super::format;
use super::OutputFormat;

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
        OutputFormat::Text => present_text(&view, file_path),
        OutputFormat::Json => present_json(&view, file_path),
        OutputFormat::Visual => present_visual(&repo, commit_a, commit_b, file_path),
    }
}

fn present_text(view: &DiffView, file_path: &str) -> Result<(), String> {
    if view.report.is_empty() {
        println!("Sin cambios semanticos.");
        return Ok(());
    }

    let semantic: Vec<_> = view.report.components.iter().filter(|c| !c.cosmetic).collect();
    let cosmetic: Vec<_> = view.report.components.iter().filter(|c| c.cosmetic).collect();

    println!("Archivo : {file_path}");
    println!("Cambios : {}", semantic.len());
    if !cosmetic.is_empty() {
        println!("Cosméticos: {} (solo posición)", cosmetic.len());
    }
    println!();

    for c in &semantic {
        let is_rename = c.kind == ChangeKind::Modified && c.name.contains(" → ");
        let marker = if is_rename { "r" } else {
            match c.kind {
                ChangeKind::Added    => "+",
                ChangeKind::Removed  => "-",
                ChangeKind::Modified => "~",
            }
        };
        println!("  {marker} {}", c.name);

        if let (Some(before), Some(after)) = (&c.before, &c.after) {
            let all_keys: std::collections::BTreeSet<_> =
                before.keys().chain(after.keys()).collect();
            for key in all_keys {
                if matches!(key.as_str(), "x" | "y" | "rotation" | "mirror") {
                    continue;
                }
                match (before.get(key), after.get(key)) {
                    (Some(a), Some(b)) if a != b => {
                        println!("      {key}: {a} → {b}");
                    }
                    (None, Some(b)) => {
                        println!("      {key}: (nuevo) → {b}");
                    }
                    (Some(a), None) => {
                        println!("      {key}: {a} → (eliminado)");
                    }
                    _ => {}
                }
            }
        } else if c.kind == ChangeKind::Added {
            if let Some(after) = &c.after {
                if let Some(sym) = after.get("symbol") {
                    println!("      símbolo: {sym}");
                }
            }
        }
    }

    if !view.report.nets_added.is_empty() {
        println!();
        for net in &view.report.nets_added {
            println!("  + net:{net}");
        }
    }
    if !view.report.nets_removed.is_empty() {
        if view.report.nets_added.is_empty() { println!(); }
        for net in &view.report.nets_removed {
            println!("  - net:{net}");
        }
    }

    Ok(())
}

fn present_json(view: &DiffView, file_path: &str) -> Result<(), String> {
    let payload = json!({
        "file": file_path,
        "warnings": view.warnings,
        "components": view.report.components,
        "nets_added": view.report.nets_added,
        "nets_removed": view.report.nets_removed,
        "is_move_all": view.report.is_move_all,
    });
    println!("{}", serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?);
    Ok(())
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

    run_gui_with_args(extra_args)
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
    let level = if args.full {
        DetailLevel::Completo
    } else if args.detail {
        DetailLevel::Detalle
    } else {
        DetailLevel::Resumen
    };

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
    let report = log::analyze_with_options_path(&args.repo, &opts)
        .map_err(|e| e.to_string())?;

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
            let sym = std::path::Path::new(root).join(pdk).join("libs.tech/xschem");
            if sym.exists() {
                println!("  [ok]  $PDK_ROOT/$PDK → {}", sym.display());
            } else {
                println!("  [!]  $PDK_ROOT/$PDK configurado pero ruta no encontrada: {}", sym.display());
            }
        }
        _ => println!("  [--]  $PDK_ROOT / $PDK: no configurados"),
    }

    match &tools {
        Some(t) => {
            let devices = std::path::Path::new(t)
                .join("xschem/share/xschem/xschem_library/devices");
            if devices.exists() {
                println!("  [ok]  $TOOLS → {}", devices.display());
            } else {
                println!("  [!]  $TOOLS configurado pero devices no encontrado: {}", devices.display());
            }
        }
        None => println!("  [--]  $TOOLS: no configurado"),
    }

    let has_symbols = xschemrc.is_some()
        || pdk_root.as_ref().zip(pdk_name.as_ref()).map(|(r, p)| {
            std::path::Path::new(r).join(p).join("libs.tech/xschem").exists()
        }).unwrap_or(false)
        || tools.as_ref().map(|t| {
            std::path::Path::new(t).join("xschem/share/xschem/xschem_library/devices").exists()
        }).unwrap_or(false);

    if !has_symbols {
        println!("  [!]  Sin fuente de símbolos — los componentes se renderizarán como cajas vacías");
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
    let level = if args.full {
        DetailLevel::Completo
    } else if args.detail {
        DetailLevel::Detalle
    } else {
        DetailLevel::Resumen
    };

    let opts = StatusOptions { level, paths: args.paths };
    let report = status::analyze_with_options_path(&args.repo, &opts)
        .map_err(|e| e.to_string())?;

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

// ─── GUI ─────────────────────────────────────────────────────────────────────

pub(super) fn run_gui(file: Option<PathBuf>) -> Result<(), String> {
    let args: Vec<std::ffi::OsString> =
        file.into_iter().map(|p| p.into_os_string()).collect();
    run_gui_with_args(args)
}

fn run_gui_with_args(args: Vec<std::ffi::OsString>) -> Result<(), String> {
    if let Some(bin) = locate_gui_binary() {
        let status = Command::new(bin).args(&args).status().map_err(|e| e.to_string())?;
        return if status.success() { Ok(()) } else {
            Err(format!("riku-gui finalizó con error: {status}"))
        };
    }

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "No se pudo resolver la raíz del workspace.".to_string())?;

    let mut cargo = Command::new("cargo");
    cargo.args(["run", "--package", "riku-gui", "--bin", "riku-gui"])
        .current_dir(workspace_root);
    if !args.is_empty() {
        cargo.arg("--").args(&args);
    }

    let status = cargo.status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else {
        Err(format!("No se pudo iniciar riku-gui: {status}"))
    }
}

fn locate_gui_binary() -> Option<PathBuf> {
    let bin_name = format!("riku-gui{}", std::env::consts::EXE_SUFFIX);

    if let Ok(path) = std::env::var("RIKU_GUI_BIN") {
        let candidate = PathBuf::from(&path);
        if candidate.exists() { return Some(candidate); }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Ok(dir) = dir.canonicalize() {
                let sibling = dir.join(&bin_name);
                if sibling.exists() { return Some(sibling); }
                for ancestor in [dir.parent(), dir.parent().and_then(|p| p.parent())] {
                    if let Some(p) = ancestor {
                        for profile in ["release", "debug"] {
                            let candidate = p.join(profile).join(&bin_name);
                            if candidate.exists() { return Some(candidate); }
                        }
                    }
                }
            }
        }
    }

    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for profile in ["release", "debug"] {
        let candidate = manifest_dir.parent()
            .map(|p| p.join("target").join(profile).join(&bin_name));
        if let Some(c) = candidate {
            if c.exists() { return Some(c); }
        }
    }

    None
}
