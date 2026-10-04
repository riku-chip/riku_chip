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

    let has_symbols = xschemrc.is_some() || matches!(pdk, PdkStatus::Found(_)) || matches!(tools, ToolsStatus::Found(_));

    let drivers = crate::modules::registry().modules().iter().map(|m| m.info()).collect();

    DoctorReport { repo_workdir, xschemrc, pdk, tools, has_symbols, drivers }
}

fn locate_xschemrc() -> Option<PathBuf> {
    let local = PathBuf::from(".xschemrc");
    if local.exists() {
        return Some(local);
    }
    dirs::home_dir().map(|h| h.join(".xschemrc")).filter(|p| p.exists())
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

/// Cómo está un punto del diagnóstico.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    Ok,
    Warn,
    /// No configurado (no es un error).
    Absent,
    /// Módulo no disponible.
    Missing,
}

impl Mark {
    fn tag(self) -> &'static str {
        match self {
            Mark::Ok => "[ok]",
            Mark::Warn => "[!]",
            Mark::Absent => "[--]",
            Mark::Missing => "[x]",
        }
    }
}

/// Una sección del diagnóstico: título y sus puntos. La CLI la imprime y
/// el visor la muestra (el mismo contenido).
pub(crate) struct Section {
    pub title: String,
    pub items: Vec<(Mark, String)>,
}

/// El diagnóstico de `repo` en secciones.
pub(crate) fn sections(repo: &Path) -> Vec<Section> {
    let report = analyze(repo);
    let mut out = Vec::new();

    let mut items = vec![match &report.repo_workdir {
        Some(p) => (Mark::Ok, p.display().to_string()),
        None => (Mark::Warn, tr!("doctor.no_repo")),
    }];
    items.push(match project_config(&report) {
        Some((path, Ok(()))) => (Mark::Ok, tr!("doctor.config_ok", file = path.display())),
        Some((path, Err(e))) => (Mark::Warn, format!("{}: {e}", path.display())),
        None => (Mark::Absent, tr!("doctor.config_none", file = crate::core::config::FILE)),
    });
    out.push(Section { title: tr!("doctor.repo"), items });

    let mut items = vec![xschemrc_item(&report.xschemrc), pdk_item(&report.pdk), tools_item(&report.tools)];
    #[cfg(feature = "layout")]
    items.push(magic_item());
    if !report.has_symbols {
        items.push((Mark::Warn, tr!("doctor.no_symbols")));
    }
    out.push(Section { title: "PDK".to_string(), items });

    let items = report
        .drivers
        .iter()
        .map(|info| {
            let mark = if info.available { Mark::Ok } else { Mark::Missing };
            (mark, format!("{:10} {}", info.name, info.version))
        })
        .collect();
    out.push(Section { title: tr!("doctor.modules"), items });

    // `riku lvs` usa Netgen; el resto de Riku no.
    #[cfg(all(feature = "xschem", feature = "layout"))]
    {
        let items = ["netgen"]
            .into_iter()
            .map(|t| match crate::lvs::find_tool(t) {
                Some(p) => (Mark::Ok, format!("{t:10} {}", p.display())),
                None => (Mark::Absent, tr!("doctor.lvs_missing", tool = t)),
            })
            .collect();
        out.push(Section { title: "LVS".to_string(), items });
    }
    out
}

fn print(sections: &[Section]) {
    println!(
        "
{}
",
        tr!("doctor.title")
    );
    for (i, section) in sections.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("--- {} ---", section.title);
        for (mark, text) in &section.items {
            println!("  {}  {text}", mark.tag());
        }
    }
    println!(
        "
{}
",
        tr!("doctor.ready")
    );
}

fn xschemrc_item(xschemrc: &Option<PathBuf>) -> (Mark, String) {
    match xschemrc {
        Some(p) => (Mark::Ok, format!(".xschemrc: {}", p.display())),
        None => (Mark::Absent, tr!("doctor.xschemrc_missing")),
    }
}

fn pdk_item(pdk: &PdkStatus) -> (Mark, String) {
    match pdk {
        PdkStatus::Found(p) => (Mark::Ok, format!("$PDK_ROOT/$PDK → {}", p.display())),
        PdkStatus::Misconfigured(p) => (Mark::Warn, tr!("doctor.pdk_missing", path = p.display())),
        PdkStatus::NotConfigured => {
            let root = crate::modules::xschem_pdk::pdk_root();
            let installed = root.as_deref().map(crate::modules::xschem_pdk::installed_pdks).unwrap_or_default();
            match (root, installed.is_empty()) {
                (Some(r), false) => (Mark::Ok, tr!("doctor.pdk_detected", root = r.display(), list = installed.join(", "))),
                _ => (Mark::Absent, tr!("doctor.pdk_none")),
            }
        }
    }
}

/// Librerías `.mag` de los PDK instalados: de ahí salen las celdas que un
/// layout de Magic usa y no están en el repo.
#[cfg(feature = "layout")]
fn magic_item() -> (Mark, String) {
    let libs = riku_mod_layout::mag::pdk_libraries();
    if libs.is_empty() {
        (Mark::Absent, tr!("doctor.magic_none"))
    } else {
        let list: Vec<String> = libs.iter().map(|(tech, n)| format!("{tech} ({n})")).collect();
        (Mark::Ok, tr!("doctor.magic_libs", list = list.join(", ")))
    }
}

fn tools_item(tools: &ToolsStatus) -> (Mark, String) {
    match tools {
        ToolsStatus::Found(p) => (Mark::Ok, format!("$TOOLS → {}", p.display())),
        ToolsStatus::Misconfigured(p) => (Mark::Warn, tr!("doctor.tools_missing", path = p.display())),
        ToolsStatus::NotConfigured => (Mark::Absent, tr!("doctor.tools_none")),
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub(super) fn run(repo: PathBuf, json: bool) -> Result<(), String> {
    if json {
        return print_json(&analyze(&repo));
    }
    print(&sections(&repo));
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
        "lvs": lvs_tools(),
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

/// Dónde están las herramientas de `riku lvs` (`null` si faltan).
fn lvs_tools() -> serde_json::Value {
    #[cfg(all(feature = "xschem", feature = "layout"))]
    {
        let at = |t: &str| crate::lvs::find_tool(t).map(|p| p.display().to_string());
        serde_json::json!({ "netgen": at("netgen") })
    }
    #[cfg(not(all(feature = "xschem", feature = "layout")))]
    serde_json::Value::Null
}
