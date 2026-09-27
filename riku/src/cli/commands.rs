//! Ejecutores de los comandos CLI.
//!
//! Cada función `run_*` implementa un comando concreto y no conoce al shell ni
//! al parser — solo recibe argumentos ya resueltos. Esto permite que tanto la
//! invocación directa (`riku diff ...`) como el REPL interno llamen al mismo
//! código sin duplicación.

use std::path::PathBuf;

use riku_kernel::DiffOptions;

use crate::core::analysis::diff_set::{self, Side};
use crate::core::analysis::show::analyze_show;
use crate::core::domain::ports::{GitRepository, RepoRoot};
use crate::core::domain::models::FileChange;
use crate::core::git::git_service::GitService;
use crate::core::analysis::log;
use crate::core::analysis::status::{self, StatusOptions};
use crate::core::analysis::summary::{DetailLevel, SummaryCategory};

use super::OutputFormat;
use super::format;
use super::gui;

// ─── Diff ────────────────────────────────────────────────────────────────────

/// `riku diff [A] [B] [ARCHIVO]`: el mismo camino para todos los formatos
/// (el módulo sale del registro por la extensión). Ver [`diff_set`] para
/// las formas que acepta.
pub(super) fn run_diff(
    repo: PathBuf,
    targets: &[String],
    format: OutputFormat,
    cosmetic_threshold_um2: f64,
    use_cache: bool,
    expressions: Vec<String>,
) -> Result<Changes, String> {
    let modules = crate::modules::registry();
    let svc = GitService::open(&repo).map_err(|e| e.to_string())?;
    let workdir = svc.root().map(|p| p.to_path_buf());
    let (from, to, file) = resolve_targets(targets, &modules, workdir.as_deref())?;

    if matches!(format, OutputFormat::Visual) {
        let Some(file) = file else {
            return Err("diff -f visual necesita un archivo: riku diff [A] [B] <archivo> -f visual".into());
        };
        return present_visual(&repo, from.token(), to.token(), &file, &expressions).map(|_| Changes::Clean);
    }
    // El umbral cosmético y la cache los usa el módulo de layouts; las
    // expresiones, el de simulación. Los demás los ignoran.
    let opts = DiffOptions { cosmetic_threshold: Some(cosmetic_threshold_um2), use_cache, expressions };
    match file {
        Some(file) => {
            let mut report = diff_set::analyze_file(&svc, workdir.as_deref(), &from, &to, &file, &modules, &opts)
                .map_err(|e| e.to_string())?;
            let warnings = std::mem::take(&mut report.warnings);
            print_diff(&report, &warnings, &file, &from, &to, format)?;
            Ok(Changes::of_reports([&report]))
        }
        None => {
            if matches!(format, OutputFormat::JsonV1) {
                return Err("-f json-v1 solo sirve para un archivo; usa -f json (schema riku-diff-set/v1)".into());
            }
            let report = diff_set::analyze_all(&svc, workdir.as_deref(), &from, &to, &modules, &opts)
                .map_err(|e| e.to_string())?;
            match format {
                OutputFormat::Json => format::diff_set::print_json(&report)?,
                _ => format::diff_set::print_text(&report)?,
            }
            Ok(Changes::of_reports(report.files.iter().filter_map(|f| f.change.as_ref())))
        }
    }
}

/// Interpreta `[A] [B] [ARCHIVO]`. El último argumento es un archivo si
/// algún módulo conoce su extensión o si existe en el working tree; si no,
/// es un commit.
fn resolve_targets(
    targets: &[String],
    modules: &riku_kernel::Registry,
    workdir: Option<&std::path::Path>,
) -> Result<(Side, Side, Option<String>), String> {
    let is_file = |t: &str| modules.for_path(t).is_some() || workdir.is_some_and(|w| w.join(t).is_file());
    let rev = |t: &str| Side::Rev(t.to_string());
    Ok(match targets {
        [] => (rev("HEAD"), Side::WorkTree, None),
        [f] if is_file(f) => (rev("HEAD"), Side::WorkTree, Some(f.clone())),
        [a] => (rev(a), Side::WorkTree, None),
        [a, f] if is_file(f) => (rev(a), Side::WorkTree, Some(f.clone())),
        [a, b] => (rev(a), rev(b), None),
        [a, b, f] => (rev(a), rev(b), Some(f.clone())),
        _ => return Err("diff acepta a lo más 3 argumentos: [A] [B] [ARCHIVO]".into()),
    })
}

/// Si un comando encontró cambios funcionales. Lo usan los códigos de salida
/// de `status` y de `diff`/`show --ci` (ver `cli::run`).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Changes {
    /// Sin cambios, o solo cosméticos.
    Clean,
    /// Al menos un cambio funcional.
    Functional,
    /// Algún archivo no se pudo comparar.
    Failed,
}

impl Changes {
    fn of(functional: bool) -> Self {
        if functional { Changes::Functional } else { Changes::Clean }
    }

    /// Un error en cualquier archivo pesa más que los cambios.
    fn of_reports<'a>(reports: impl IntoIterator<Item = &'a FileChange>) -> Self {
        let mut out = Changes::Clean;
        for r in reports {
            if r.error.is_some() {
                return Changes::Failed;
            }
            if r.functional().next().is_some() {
                out = Changes::Functional;
            }
        }
        out
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
    Ok(Changes::of_reports(report.files.iter().filter_map(|f| f.change.as_ref())))
}

/// Imprime un diff en el formato pedido (texto, JSON v2 o JSON v1).
fn print_diff(
    report: &FileChange,
    warnings: &[String],
    file_path: &str,
    from: &Side,
    to: &Side,
    format: OutputFormat,
) -> Result<(), String> {
    for w in warnings {
        eprintln!("[!] {w}");
    }
    match format {
        OutputFormat::Text => format::diff_text::print(report, file_path),
        OutputFormat::Json => format::diff_json::print(report, warnings, file_path, from.label(), to.label()),
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
    pub graph: bool,
    pub ascii: bool,
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
        graph: args.graph,
        skip_summaries: false,
    };
    let report = log::analyze_with_options_path(&args.repo, &opts, &crate::modules::registry()).map_err(|e| e.to_string())?;

    if args.json {
        format::log_json::print(&report, !args.compact)?;
    } else if args.graph {
        format::log_graph::print(&report, level, format::log_graph::Style::detect(args.ascii));
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

    if report.count_by_category(SummaryCategory::Error) > 0 {
        return Ok(Changes::Failed);
    }
    Ok(Changes::of(report.has_semantic_changes()))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(args: &[&str], workdir: Option<&std::path::Path>) -> (Side, Side, Option<String>) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        resolve_targets(&args, &crate::modules::registry(), workdir).unwrap()
    }

    fn rev(r: &str) -> Side {
        Side::Rev(r.into())
    }

    #[test]
    fn diff_targets_like_git() {
        assert_eq!(resolve(&[], None), (rev("HEAD"), Side::WorkTree, None));
        assert_eq!(resolve(&["amp.sch"], None), (rev("HEAD"), Side::WorkTree, Some("amp.sch".into())));
        assert_eq!(resolve(&["main"], None), (rev("main"), Side::WorkTree, None));
        assert_eq!(resolve(&["v1", "amp.sch"], None), (rev("v1"), Side::WorkTree, Some("amp.sch".into())));
        assert_eq!(resolve(&["v1", "v2"], None), (rev("v1"), rev("v2"), None));
        assert_eq!(resolve(&["v1", "v2", "amp.sch"], None), (rev("v1"), rev("v2"), Some("amp.sch".into())));
    }

    #[test]
    fn an_existing_file_without_module_is_still_a_file() {
        let dir = std::env::temp_dir().join(format!("riku-targets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notas.txt"), "x").unwrap();
        assert_eq!(resolve(&["notas.txt"], Some(&dir)), (rev("HEAD"), Side::WorkTree, Some("notas.txt".into())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn too_many_targets_is_an_error() {
        let args: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert!(resolve_targets(&args, &crate::modules::registry(), None).is_err());
    }
}
