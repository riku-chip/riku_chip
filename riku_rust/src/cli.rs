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
    let driver = crate::core::registry::get_driver_for(file_path)
        .ok_or_else(|| "No hay driver visual para este formato.".to_string())?;

    let content_b = GitRepository::get_blob(&svc, commit_b, file_path)
        .map_err(|e| e.to_string())?;
    let svg_path = driver
        .render(&content_b, file_path)
        .ok_or_else(|| "Render no disponible (herramienta EDA no instalada).".to_string())?;
    let sch_b = parse(&content_b);

    let sch_a = match GitRepository::get_blob(&svc, commit_a, file_path) {
        Ok(content_a) => Some(parse(&content_a)),
        Err(_) => None,
    };

    let diff_report = driver_report_to_diff_report(report);

    let svg_content = std::fs::read_to_string(&svg_path).map_err(|e| e.to_string())?;
    let annotated = annotate(&svg_content, &sch_b, &diff_report, sch_a.as_ref(), Some(&svg_path));

    let mut tmp = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut tmp, annotated.as_bytes()).map_err(|e| e.to_string())?;
    let out_path = tmp
        .into_temp_path()
        .keep()
        .map_err(|e| e.error.to_string())?;

    println!("SVG anotado: {}", out_path.display());
    open_file(&out_path)?;
    Ok(())
}

fn open_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let status = Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            return Ok(());
        }
        return Err("No se pudo abrir el archivo en Windows.".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .arg(path)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            return Ok(());
        }
        return Err("No se pudo abrir el archivo en macOS.".to_string());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let status = Command::new("xdg-open")
            .arg(path)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            return Ok(());
        }
        Err("No se pudo abrir el archivo con xdg-open.".to_string())
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

fn run_doctor(_repo: PathBuf) -> Result<(), String> {
    let mut any_missing = false;
    for driver in get_drivers() {
        let info = driver.info();
        let status = if info.available { "[ok]" } else { "[x]" };
        let version = if info.available {
            format!("  {}", info.version)
        } else {
            "  no encontrado".to_string()
        };
        println!("  {status}  {:<12}{version}", info.name);
        if !info.available {
            any_missing = true;
        }
    }

    if any_missing {
        return Err("Faltan herramientas externas.".to_string());
    }
    Ok(())
}
