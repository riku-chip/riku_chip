use std::path::PathBuf;
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

use crate::core::analyzer::analyze_diff;
use crate::core::git_service::GitService;
use crate::core::models::{ChangeKind, ComponentDiff, DiffReport};
use crate::core::ports::GitRepository;
use crate::core::registry::get_drivers;
use crate::core::svg_annotator::annotate;
use crate::parsers::xschem::parse;
use crate::adapters::xschem_driver::XschemDriver;
use crate::core::driver::RikuDriver;

#[derive(Clone, Debug, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Visual,
}

#[derive(Parser, Debug)]
#[command(
    name = "riku",
    author = "Ariel Amado Frias Rojas",
    about = "Riku - VCS semantico para diseno de chips"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Muestra cambios semanticos entre dos commits para un archivo.
    Diff {
        commit_a: String,
        commit_b: String,
        file_path: String,
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        #[arg(short = 'f', long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
    },
    /// Lista commits del branch actual, opcionalmente filtrados por archivo.
    Log {
        file_path: Option<String>,
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
        #[arg(short = 's', long)]
        semantic: bool,
    },
    /// Verifica que herramientas externas esten disponibles.
    Doctor {
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
    },
    /// Renderiza un archivo .sch a SVG y lo abre.
    Render {
        file: PathBuf,
    },
}

pub fn run() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Diff {
            commit_a,
            commit_b,
            file_path,
            repo,
            format,
        } => run_diff(repo, &commit_a, &commit_b, &file_path, format),
        Commands::Log {
            file_path,
            repo,
            limit,
            semantic,
        } => run_log(repo, file_path.as_deref(), limit, semantic),
        Commands::Doctor { repo } => run_doctor(repo),
        Commands::Render { file } => run_render(file),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run_diff(
    repo: PathBuf,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
    format: OutputFormat,
) -> Result<(), String> {
    let report = analyze_diff(&repo, commit_a, commit_b, file_path)
        .map_err(|e| e.to_string())?;

    for warning in &report.warnings {
        eprintln!("[!] {warning}");
    }

    match format {
        OutputFormat::Json => {
            let payload = json!({
                "file_type": report.file_type,
                "warnings": report.warnings,
                "changes": report.changes,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        OutputFormat::Visual => run_visual(repo, commit_a, commit_b, file_path, &report),
        OutputFormat::Text => {
            if report.is_empty() {
                println!("Sin cambios semanticos.");
                return Ok(());
            }

            println!("Archivo: {file_path}  ({})", report.file_type);
            println!("Cambios: {}", report.changes.len());
            println!();
            for change in &report.changes {
                let cosmetic = if change.cosmetic { "  [cosmetico]" } else { "" };
                println!("  {:<10} {}{cosmetic}", change.kind, change.element);
            }
            Ok(())
        }
    }
}

fn run_visual(
    repo: PathBuf,
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
    report: &crate::core::driver::DriverDiffReport,
) -> Result<(), String> {
    let svc = GitService::open(&repo).map_err(|e| e.to_string())?;
    let driver = XschemDriver::new();

    let content_b = GitRepository::get_blob(&svc, commit_b, file_path)
        .map_err(|e| e.to_string())?;
    let sch_b = parse(&content_b);
    let svg_b_path = driver
        .render(&content_b, file_path)
        .ok_or_else(|| "Render del commit B no disponible.".to_string())?;
    let svg_b = std::fs::read_to_string(&svg_b_path).map_err(|e| e.to_string())?;

    let (svg_a, sch_a) = match GitRepository::get_blob(&svc, commit_a, file_path) {
        Ok(content_a) => {
            let sch = parse(&content_a);
            let svg_path = driver.render(&content_a, file_path);
            let svg = svg_path.and_then(|p| std::fs::read_to_string(&p).ok());
            (svg, Some(sch))
        }
        Err(_) => (None, None),
    };

    let diff_report = driver_report_to_diff_report(report);

    let annotated_b = annotate(&svg_b, &sch_b, &diff_report, sch_a.as_ref());
    let panel_a = svg_a
        .as_deref()
        .map(|svg| {
            // Para el panel A mostramos el esquemático anterior sin anotaciones de cambio,
            // solo marcamos en gris los componentes que desaparecerán.
            let removed_report = DiffReport {
                components: diff_report
                    .components
                    .iter()
                    .filter(|c| c.kind == ChangeKind::Removed)
                    .cloned()
                    .collect(),
                nets_added: vec![],
                nets_removed: diff_report.nets_removed.clone(),
                is_move_all: false,
            };
            if let Some(sch_a) = sch_a.as_ref() {
                annotate(svg, sch_a, &removed_report, None)
            } else {
                svg.to_string()
            }
        })
        .unwrap_or_else(|| "<p style='color:#888'>Archivo nuevo — no existe en el commit anterior</p>".to_string());

    let html = build_diff_html(commit_a, commit_b, file_path, &panel_a, &annotated_b);

    let mut tmp = tempfile::Builder::new()
        .suffix(".html")
        .tempfile()
        .map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut tmp, html.as_bytes()).map_err(|e| e.to_string())?;
    let out_path = tmp
        .into_temp_path()
        .keep()
        .map_err(|e| e.error.to_string())?;

    println!("Diff visual: {}", out_path.display());
    open_file(&out_path)?;
    Ok(())
}

fn build_diff_html(
    commit_a: &str,
    commit_b: &str,
    file_path: &str,
    panel_a: &str,
    panel_b: &str,
) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="es">
<head>
<meta charset="utf-8">
<title>riku diff — {file_path}</title>
<style>
  * {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{ background: #1a1a1a; color: #ccc; font-family: monospace; display: flex; flex-direction: column; height: 100vh; }}
  header {{ padding: 8px 16px; background: #111; border-bottom: 1px solid #333; font-size: 13px; }}
  header span {{ color: #888; }}
  .panels {{ display: flex; flex: 1; overflow: hidden; gap: 2px; padding: 2px; }}
  .panel {{ flex: 1; display: flex; flex-direction: column; background: #111; border: 1px solid #333; overflow: hidden; }}
  .panel-label {{ padding: 4px 10px; font-size: 11px; color: #888; background: #0d0d0d; border-bottom: 1px solid #222; }}
  .panel-label b {{ color: #ccc; }}
  .panel-body {{ flex: 1; overflow: auto; display: flex; align-items: center; justify-content: center; padding: 8px; }}
  .panel-body svg {{ max-width: 100%; max-height: 100%; }}
  .legend {{ padding: 6px 16px; background: #111; border-top: 1px solid #333; font-size: 11px; display: flex; gap: 16px; }}
  .dot {{ display: inline-block; width: 10px; height: 10px; border-radius: 2px; margin-right: 4px; }}
</style>
</head>
<body>
<header><span>riku diff</span> &nbsp;·&nbsp; {file_path} &nbsp;·&nbsp; <span>{commit_a}</span> → <span>{commit_b}</span></header>
<div class="panels">
  <div class="panel">
    <div class="panel-label">ANTES &nbsp;<b>{commit_a}</b></div>
    <div class="panel-body">{panel_a}</div>
  </div>
  <div class="panel">
    <div class="panel-label">DESPUÉS &nbsp;<b>{commit_b}</b></div>
    <div class="panel-body">{panel_b}</div>
  </div>
</div>
<div class="legend">
  <span><span class="dot" style="background:rgba(0,200,0,0.7)"></span>Añadido</span>
  <span><span class="dot" style="background:rgba(200,0,0,0.7)"></span>Removido</span>
  <span><span class="dot" style="background:rgba(255,180,0,0.7)"></span>Modificado</span>
  <span><span class="dot" style="background:rgba(120,120,120,0.7)"></span>Cosmético</span>
</div>
</body>
</html>"#
    )
}

fn run_render(file: PathBuf) -> Result<(), String> {
    let content = std::fs::read(&file).map_err(|e| e.to_string())?;
    let driver = XschemDriver::new();
    let path_hint = file.to_string_lossy();
    let svg_path = driver
        .render(&content, &path_hint)
        .ok_or_else(|| "No se pudo renderizar el archivo.".to_string())?;
    println!("SVG: {}", svg_path.display());
    open_file(&svg_path)?;
    Ok(())
}

fn open_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
        match Command::new("xdg-open")
            .env("DISPLAY", &display)
            .arg(path)
            .spawn()
        {
            Ok(_) => {}
            Err(_) => {
                eprintln!("[!] No se pudo abrir el SVG automaticamente.");
                eprintln!("    Abrelo manualmente: {}", path.display());
            }
        }
        Ok(())
    }
}

fn run_log(
    repo: PathBuf,
    file_path: Option<&str>,
    limit: usize,
    semantic: bool,
) -> Result<(), String> {
    let svc = GitService::open(&repo).map_err(|e| e.to_string())?;
    let commits = GitRepository::get_commits(&svc, file_path).map_err(|e| e.to_string())?;

    if commits.is_empty() {
        println!("Sin commits encontrados.");
        return Ok(());
    }

    for (idx, commit) in commits.iter().take(limit).enumerate() {
        println!(
            "{}  {:<20}  {}",
            commit.short_id,
            commit.author,
            commit.message.chars().take(60).collect::<String>()
        );

        if semantic && file_path.is_some() && idx + 1 < commits.len() {
            if let Some(file_path) = file_path {
                if let Ok(report) = analyze_diff(&repo, &commits[idx + 1].oid, &commit.oid, file_path) {
                    let (added, removed, modified) = summarize_changes(&report);
                    if added + removed + modified > 0 {
                        let mut parts = Vec::new();
                        if added > 0 {
                            parts.push(format!("+{added}"));
                        }
                        if removed > 0 {
                            parts.push(format!("-{removed}"));
                        }
                        if modified > 0 {
                            parts.push(format!("~{modified}"));
                        }
                        if !parts.is_empty() {
                            println!("           {}", parts.join("  "));
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn driver_report_to_diff_report(report: &crate::core::driver::DriverDiffReport) -> DiffReport {
    DiffReport {
        components: report
            .changes
            .iter()
            .filter(|c| !c.element.starts_with("net:") && c.element != "layout")
            .map(|c| ComponentDiff {
                name: c.element.clone(),
                kind: c.kind.clone(),
                cosmetic: c.cosmetic,
                before: c.before.clone(),
                after: c.after.clone(),
            })
            .collect(),
        nets_added: report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Added && c.element.starts_with("net:"))
            .map(|c| c.element.trim_start_matches("net:").to_string())
            .collect(),
        nets_removed: report
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Removed && c.element.starts_with("net:"))
            .map(|c| c.element.trim_start_matches("net:").to_string())
            .collect(),
        is_move_all: report
            .changes
            .iter()
            .any(|c| c.element == "layout" && c.cosmetic),
    }
}

fn summarize_changes(report: &crate::core::driver::DriverDiffReport) -> (usize, usize, usize) {
    let added = report
        .changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Added && !c.cosmetic)
        .count();
    let removed = report
        .changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Removed && !c.cosmetic)
        .count();
    let modified = report
        .changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Modified && !c.cosmetic)
        .count();
    (added, removed, modified)
}

#[cfg(test)]
mod tests {
    use super::{driver_report_to_diff_report, summarize_changes};
    use crate::core::driver::{DiffEntry, DriverDiffReport};
    use crate::core::models::{ChangeKind, FileFormat};

    #[test]
    fn translates_driver_report_to_diff_report() {
        let report = DriverDiffReport {
            file_type: FileFormat::Xschem,
            changes: vec![
                DiffEntry {
                    kind: ChangeKind::Added,
                    element: "R1".to_string(),
                    before: None,
                    after: Some(std::iter::once(("value".to_string(), "10k".to_string())).collect()),
                    cosmetic: false,
                },
                DiffEntry {
                    kind: ChangeKind::Added,
                    element: "net:Vdd".to_string(),
                    before: None,
                    after: None,
                    cosmetic: false,
                },
                DiffEntry {
                    kind: ChangeKind::Modified,
                    element: "layout".to_string(),
                    before: None,
                    after: None,
                    cosmetic: true,
                },
            ],
            visual_a: None,
            visual_b: None,
            warnings: vec![],
        };

        let diff = driver_report_to_diff_report(&report);
        assert_eq!(diff.components.len(), 1);
        assert_eq!(diff.components[0].name, "R1");
        assert_eq!(diff.nets_added, vec!["Vdd".to_string()]);
        assert!(diff.is_move_all);
    }

    #[test]
    fn counts_only_non_cosmetic_changes() {
        let report = DriverDiffReport {
            file_type: FileFormat::Xschem,
            changes: vec![
                DiffEntry {
                    kind: ChangeKind::Added,
                    element: "R1".to_string(),
                    before: None,
                    after: None,
                    cosmetic: true,
                },
                DiffEntry {
                    kind: ChangeKind::Removed,
                    element: "C1".to_string(),
                    before: None,
                    after: None,
                    cosmetic: false,
                },
                DiffEntry {
                    kind: ChangeKind::Modified,
                    element: "M1".to_string(),
                    before: None,
                    after: None,
                    cosmetic: false,
                },
            ],
            visual_a: None,
            visual_b: None,
            warnings: vec![],
        };

        assert_eq!(summarize_changes(&report), (0, 1, 1));
    }
}

fn run_doctor(repo: PathBuf) -> Result<(), String> {
    println!("\n🔍 Riku Doctor - Diagnóstico del Entorno\n");

    let mut any_error = false;

    // 1. Verificación del PDK
    println!("--- Entorno PDK ---");
    let pdk_root = std::env::var("PDK_ROOT").ok();
    let pdk_name = std::env::var("PDK").ok();

    match (&pdk_root, &pdk_name) {
        (Some(root), Some(pdk)) => {
            let path = std::path::Path::new(root).join(pdk);
            if path.exists() {
                println!("  [ok]  PDK detectado: {} en {}", pdk, root);
                let xschem_pdk = path.join("libs.tech/xschem");
                if xschem_pdk.exists() {
                    println!("  [ok]  Símbolos Xschem: encontrados");
                } else {
                    println!("  [!]  Símbolos Xschem: NO encontrados en {}", xschem_pdk.display());
                }
            } else {
                println!("  [x]  PDK configurado ({}) pero la ruta NO existe: {}", pdk, path.display());
                any_error = true;
            }
        }
        _ => {
            println!("  [x]  PDK no configurado (falta $PDK_ROOT o $PDK)");
            any_error = true;
        }
    }

    // 2. Verificación del Espacio de Trabajo
    println!("\n--- Espacio de Trabajo ---");
    match git2::Repository::discover(&repo) {
        Ok(_) => println!("  [ok]  Repositorio Git: Detectado"),
        Err(_) => {
            println!("  [!]  Repositorio Git: No detectado (Riku diff/log no funcionarán)");
        }
    }

    // 3. Verificación de Capacidades (Drivers)
    println!("\n--- Capacidades de Riku ---");
    for driver in get_drivers() {
        let info = driver.info();
        let status = if info.available { "[ok]" } else { "[x]" };
        println!("  {status}  {:10} {}", info.name, info.version);
    }

    // 4. Verificación de Caché
    println!("\n--- Sistema ---");
    let cache = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("riku");
    if std::fs::create_dir_all(&cache).is_ok() {
        println!("  [ok]  Directorio de caché: {}", cache.display());
    } else {
        println!("  [x]  Error al acceder a la caché: {}", cache.display());
        any_error = true;
    }

    println!("");
    if any_error {
        return Err("Se detectaron problemas críticos en el entorno.".to_string());
    }
    
    println!("✅ Entorno listo para diseño de chips.\n");
    Ok(())
}
