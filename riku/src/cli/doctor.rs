//! Comando `riku doctor` — diagnóstico del entorno.
//!
//! Detección y presentación están separadas: `analyze` lee env vars y
//! filesystem y devuelve un `DoctorReport` puro; `print` lo formatea en stdout.
//! `run` orquesta ambos pasos.

use std::path::{Path, PathBuf};

use riku_kernel::ModuleInfo;

use crate::i18n::tr;

use crate::modules::xschem_pdk::{pdk_status, PdkStatus};

// ─── Modelo ──────────────────────────────────────────────────────────────────

pub(super) struct DoctorReport {
    pub repo_workdir: Option<PathBuf>,
    pub xschemrc: Option<PathBuf>,
    pub pdk: PdkStatus,
    pub tools: ToolsStatus,
    pub has_symbols: bool,
    pub drivers: Vec<ModuleInfo>,
}

pub(super) enum ToolsStatus {
    /// `TOOLS` no está en el entorno.
    NotConfigured,
    /// Configurado, pero `<TOOLS>/xschem/share/xschem/xschem_library/devices`
    /// no existe.
    Misconfigured(PathBuf),
    /// Ruta encontrada.
    Found(PathBuf),
}

// ─── Análisis ────────────────────────────────────────────────────────────────

fn analyze(repo: &Path) -> DoctorReport {
    let repo_workdir = git2::Repository::discover(repo)
        .ok()
        .and_then(|r| r.workdir().map(|p| p.to_path_buf()).or_else(|| Some(r.path().to_path_buf())));

    let xschemrc = locate_xschemrc();
    let pdk = pdk_status();
    let tools = tools_status();

    let has_symbols = xschemrc.is_some()
        || matches!(pdk, PdkStatus::Found(_))
        || matches!(tools, ToolsStatus::Found(_));

    let drivers = crate::modules::registry().modules().iter().map(|m| m.info()).collect();

    DoctorReport {
        repo_workdir,
        xschemrc,
        pdk,
        tools,
        has_symbols,
        drivers,
    }
}

fn locate_xschemrc() -> Option<PathBuf> {
    let local = PathBuf::from(".xschemrc");
    if local.exists() {
        return Some(local);
    }
    dirs::home_dir()
        .map(|h| h.join(".xschemrc"))
        .filter(|p| p.exists())
}

fn tools_status() -> ToolsStatus {
    let Some(t) = std::env::var("TOOLS").ok() else {
        return ToolsStatus::NotConfigured;
    };
    let devices = Path::new(&t).join("xschem/share/xschem/xschem_library/devices");
    if devices.exists() {
        ToolsStatus::Found(devices)
    } else {
        ToolsStatus::Misconfigured(devices)
    }
}

// ─── Presentación ────────────────────────────────────────────────────────────

fn print(report: &DoctorReport) {
    println!("\n{}\n", tr!("doctor.title"));

    println!("--- {} ---", tr!("doctor.repo"));
    match &report.repo_workdir {
        Some(p) => println!("  [ok]  {}", p.display()),
        None => println!("  [!]  {}", tr!("doctor.no_repo")),
    }
    match project_config(report) {
        Some((path, Ok(()))) => println!("  [ok]  {}", tr!("doctor.config_ok", file = path.display())),
        Some((path, Err(e))) => println!("  [!]  {}: {e}", path.display()),
        None => println!("  [--]  {}", tr!("doctor.config_none", file = crate::core::config::FILE)),
    }

    println!("\n--- PDK ---");
    print_xschemrc(&report.xschemrc);
    print_pdk(&report.pdk);
    print_tools(&report.tools);
    #[cfg(feature = "layout")]
    print_magic();
    if !report.has_symbols {
        println!("  [!]  {}", tr!("doctor.no_symbols"));
    }

    println!("\n--- {} ---", tr!("doctor.modules"));
    for info in &report.drivers {
        let status = if info.available { "[ok]" } else { "[x]" };
        println!("  {status}  {:10} {}", info.name, info.version);
    }

    println!("\n{}\n", tr!("doctor.ready"));
}

fn print_xschemrc(xschemrc: &Option<PathBuf>) {
    match xschemrc {
        Some(p) => println!("  [ok]  .xschemrc: {}", p.display()),
        None => println!("  [--]  {}", tr!("doctor.xschemrc_missing")),
    }
}

fn print_pdk(pdk: &PdkStatus) {
    match pdk {
        PdkStatus::Found(p) => println!("  [ok]  $PDK_ROOT/$PDK → {}", p.display()),
        PdkStatus::Misconfigured(p) => println!("  [!]  {}", tr!("doctor.pdk_missing", path = p.display())),
        PdkStatus::NotConfigured => {
            let root = crate::modules::xschem_pdk::pdk_root();
            let installed = root.as_deref().map(crate::modules::xschem_pdk::installed_pdks).unwrap_or_default();
            match (root, installed.is_empty()) {
                (Some(r), false) => println!(
                    "  [ok]  {}",
                    tr!("doctor.pdk_detected", root = r.display(), list = installed.join(", "))
                ),
                _ => println!("  [--]  {}", tr!("doctor.pdk_none")),
            }
        }
    }
}

/// Librerías `.mag` de los PDK instalados: de ahí salen las celdas que un
/// layout de Magic usa y no están en el repo.
#[cfg(feature = "layout")]
fn print_magic() {
    let libs = riku_mod_layout::mag::pdk_libraries();
    if libs.is_empty() {
        println!("  [--]  {}", tr!("doctor.magic_none"));
    } else {
        let list: Vec<String> = libs.iter().map(|(tech, n)| format!("{tech} ({n})")).collect();
        println!("  [ok]  {}", tr!("doctor.magic_libs", list = list.join(", ")));
    }
}

fn print_tools(tools: &ToolsStatus) {
    match tools {
        ToolsStatus::Found(p) => println!("  [ok]  $TOOLS → {}", p.display()),
        ToolsStatus::Misconfigured(p) => println!("  [!]  {}", tr!("doctor.tools_missing", path = p.display())),
        ToolsStatus::NotConfigured => println!("  [--]  {}", tr!("doctor.tools_none")),
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub(super) fn run(repo: PathBuf, json: bool) -> Result<(), String> {
    let report = analyze(&repo);
    if json {
        return print_json(&report);
    }
    print(&report);
    Ok(())
}

/// `riku doctor -f json` (schema `riku-doctor/v1`): lo que un script o un
/// agente necesita para saber qué puede comparar este `riku`.
fn print_json(r: &DoctorReport) -> Result<(), String> {
    let path = |p: &PathBuf| p.display().to_string();
    let (pdk_state, pdk_path) = match &r.pdk {
        PdkStatus::Found(p) => ("found", Some(path(p))),
        PdkStatus::Misconfigured(p) => ("missing", Some(path(p))),
        PdkStatus::NotConfigured => ("not_configured", None),
    };
    let (tools_state, tools_path) = match &r.tools {
        ToolsStatus::Found(p) => ("found", Some(path(p))),
        ToolsStatus::Misconfigured(p) => ("missing", Some(path(p))),
        ToolsStatus::NotConfigured => ("not_configured", None),
    };
    let modules: Vec<_> = r
        .drivers
        .iter()
        .map(|m| {
            serde_json::json!({
                "name": m.name,
                "format": m.format,
                "extensions": m.extensions,
                "available": m.available,
                "version": m.version,
            })
        })
        .collect();
    let payload = serde_json::json!({
        "schema": "riku-doctor/v1",
        "version": env!("CARGO_PKG_VERSION"),
        "repo": r.repo_workdir.as_ref().map(path),
        "config": project_config(r).map(|(p, res)| serde_json::json!({
            "path": path(&p),
            "error": res.err(),
        })),
        "xschemrc": r.xschemrc.as_ref().map(path),
        "pdk": { "state": pdk_state, "path": pdk_path },
        "tools": { "state": tools_state, "path": tools_path },
        "symbols": r.has_symbols,
        "modules": modules,
    });
    let text = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;
    println!("{text}");
    Ok(())
}

/// `.riku.toml` del repo, si existe, y si se puede leer.
fn project_config(r: &DoctorReport) -> Option<(PathBuf, Result<(), String>)> {
    let root = r.repo_workdir.as_deref()?;
    let file = root.join(crate::core::config::FILE);
    file.is_file().then(|| {
        let res = crate::core::config::load(Some(root)).map(|_| ());
        (file, res)
    })
}
