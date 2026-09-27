//! Comando `riku doctor` — diagnóstico del entorno.
//!
//! Detección y presentación están separadas: `analyze` lee env vars y
//! filesystem y devuelve un `DoctorReport` puro; `print` lo formatea en stdout.
//! `run` orquesta ambos pasos.

use std::path::{Path, PathBuf};

use riku_kernel::ModuleInfo;

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
    println!("\nRiku Doctor — Diagnóstico del Entorno\n");

    println!("--- Repositorio Git ---");
    match &report.repo_workdir {
        Some(p) => println!("  [ok]  {}", p.display()),
        None => println!("  [!]  No detectado — diff/log no funcionarán"),
    }

    println!("\n--- PDK ---");
    print_xschemrc(&report.xschemrc);
    print_pdk(&report.pdk);
    print_tools(&report.tools);
    #[cfg(feature = "layout")]
    print_magic();
    if !report.has_symbols {
        println!(
            "  [!]  Sin fuente de símbolos — los componentes se renderizarán como cajas vacías"
        );
    }

    println!("\n--- Drivers ---");
    for info in &report.drivers {
        let status = if info.available { "[ok]" } else { "[x]" };
        println!("  {status}  {:10} {}", info.name, info.version);
    }

    println!("\nEntorno listo.\n");
}

fn print_xschemrc(xschemrc: &Option<PathBuf>) {
    match xschemrc {
        Some(p) => println!("  [ok]  .xschemrc: {}", p.display()),
        None => println!("  [--]  .xschemrc: no encontrado"),
    }
}

fn print_pdk(pdk: &PdkStatus) {
    match pdk {
        PdkStatus::Found(p) => println!("  [ok]  $PDK_ROOT/$PDK → {}", p.display()),
        PdkStatus::Misconfigured(p) => println!(
            "  [!]  $PDK_ROOT/$PDK configurado pero ruta no encontrada: {}",
            p.display()
        ),
        PdkStatus::NotConfigured => {
            let root = crate::modules::xschem_pdk::pdk_root();
            let installed = root.as_deref().map(crate::modules::xschem_pdk::installed_pdks).unwrap_or_default();
            match (root, installed.is_empty()) {
                (Some(r), false) => println!(
                    "  [ok]  $PDK no definida: se detecta por los símbolos de cada esquemático
        (instalados en {}: {}). Para fijarlo: export PDK=<nombre>",
                    r.display(),
                    installed.join(", ")
                ),
                _ => println!("  [--]  $PDK_ROOT / $PDK: no configurados y no hay PDKs en /foss/pdks"),
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
        println!("  [--]  Magic: sin librerías .mag en el PDK (las celdas usadas deben estar en el repo o en $RIKU_MAG_PATH)");
    } else {
        let list: Vec<String> = libs.iter().map(|(tech, n)| format!("{tech} ({n})")).collect();
        println!("  [ok]  Magic: librerías .mag en {}", list.join(", "));
    }
}

fn print_tools(tools: &ToolsStatus) {
    match tools {
        ToolsStatus::Found(p) => println!("  [ok]  $TOOLS → {}", p.display()),
        ToolsStatus::Misconfigured(p) => println!(
            "  [!]  $TOOLS configurado pero devices no encontrado: {}",
            p.display()
        ),
        ToolsStatus::NotConfigured => println!("  [--]  $TOOLS: no configurado"),
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub(super) fn run(repo: PathBuf) -> Result<(), String> {
    let report = analyze(&repo);
    print(&report);
    Ok(())
}
