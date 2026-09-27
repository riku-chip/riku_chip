//! Ejecutores de los comandos CLI.
//!
//! Cada función `run_*` implementa un comando concreto y no conoce al shell ni
//! al parser — solo recibe argumentos ya resueltos. Esto permite que tanto la
//! invocación directa (`riku diff ...`) como el REPL interno llamen al mismo
//! código sin duplicación.

use std::path::PathBuf;

use crate::core::config::{self, Overrides};
use crate::i18n::tr;

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
    pretty: bool,
    overrides: Overrides,
    image: Option<crate::export::Request>,
) -> Result<Changes, String> {
    let modules = crate::modules::registry();
    let svc = GitService::open(&repo).map_err(|e| e.to_string())?;
    let workdir = svc.root().map(|p| p.to_path_buf());
    let (from, to, file) = resolve_targets(targets, &modules, workdir.as_deref(), &repo)?;
    // Como en Git: el archivo se nombra desde donde uno está.
    let file = file.map(|f| repo_file(&repo, workdir.as_deref(), &f));

    if let Some(req) = image {
        let file = file.ok_or_else(|| tr!("err.image_needs_file"))?;
        let opts = config::options_for(&repo, overrides)?;
        let path = export_between(&svc, workdir.as_deref(), &from, &to, &file, &req, &opts)?;
        println!("{}", path.display());
        return Ok(Changes::Clean);
    }

    if matches!(format, OutputFormat::Visual) {
        let Some(file) = file else {
            return Err(tr!("err.visual_needs_file"));
        };
        let exprs = config::options_for(&repo, overrides)?.expressions;
        return present_visual(&repo, from.token(), to.token(), &file, &exprs).map(|_| Changes::Clean);
    }
    // Flags > `.riku.toml` > cada módulo. El umbral cosmético y la cache los
    // usa el módulo de layouts; la tolerancia y las expresiones, el de
    // simulación. Los demás los ignoran.
    let opts = config::options_for(&repo, overrides)?;
    match file {
        Some(file) => {
            let mut report = diff_set::analyze_file(&svc, workdir.as_deref(), &from, &to, &file, &modules, &opts)
                .map_err(|e| e.to_string())?;
            let warnings = std::mem::take(&mut report.warnings);
            print_diff(&report, &warnings, &file, &from, &to, format, pretty)?;
            Ok(Changes::of_reports([&report]))
        }
        None => {
            if matches!(format, OutputFormat::JsonV1) {
                return Err(tr!("err.json_v1_one_file"));
            }
            let report = diff_set::analyze_all(&svc, workdir.as_deref(), &from, &to, &modules, &opts)
                .map_err(|e| e.to_string())?;
            match format {
                OutputFormat::Json => format::diff_set::print_json(&report, pretty)?,
                _ => format::diff_set::print_text(&report)?,
            }
            Ok(Changes::of_reports(report.files.iter().filter_map(|f| f.change.as_ref())))
        }
    }
}

/// Imagen del diff de `file` entre dos versiones.
fn export_between(
    svc: &GitService,
    workdir: Option<&std::path::Path>,
    from: &Side,
    to: &Side,
    file: &str,
    req: &crate::export::Request,
    opts: &riku_kernel::DiffOptions,
) -> Result<std::path::PathBuf, String> {
    let before = read_version(svc, workdir, from, file)?;
    let after = read_version(svc, workdir, to, file)?;
    if before.is_none() && after.is_none() {
        return Err(tr!("err.not_in_either", file = file, a = from.label(), b = to.label()));
    }
    let files = diff_set::sources(svc, workdir, from, to);
    let label = format!("{} → {}", short_label(from), short_label(to));
    crate::export::image(
        &crate::modules::registry(),
        file,
        Some(before.unwrap_or_default()),
        after.unwrap_or_default(),
        files,
        &label,
        req,
        opts,
    )
}

/// `file` en una versión: `None` si no existe ahí; error si existe pero no
/// se puede leer (no se dibuja como si estuviera vacío).
fn read_version(
    svc: &GitService,
    workdir: Option<&std::path::Path>,
    side: &Side,
    file: &str,
) -> Result<Option<Vec<u8>>, String> {
    use crate::core::analysis::blob_io::Blob;
    match diff_set::read_side(svc, workdir, side, file).map_err(|e| e.to_string())? {
        Blob::Bytes(b) => Ok(Some(b)),
        Blob::Missing => Ok(None),
        Blob::Skipped(why) => Err(format!("{file} ({}): {why}", side.label())),
    }
}

/// Imagen de `file` en una sola versión.
fn export_one(
    svc: &GitService,
    workdir: Option<&std::path::Path>,
    side: &Side,
    file: &str,
    req: &crate::export::Request,
    opts: &riku_kernel::DiffOptions,
) -> Result<std::path::PathBuf, String> {
    let content = read_version(svc, workdir, side, file)?.ok_or_else(|| tr!("err.not_in", file = file, rev = side.label()))?;
    let files = diff_set::sources(svc, workdir, side, side);
    crate::export::image(&crate::modules::registry(), file, None, content, files, &short_label(side), req, opts)
}

/// `riku render ARCHIVO [--rev R]`: imagen de una versión (el disco si no hay `--rev`).
pub(super) fn run_render(
    repo: PathBuf,
    file: &str,
    rev: Option<&str>,
    req: crate::export::Request,
    overrides: Overrides,
) -> Result<(), String> {
    let opts = config::options_for(&repo, overrides)?;
    let path = match rev {
        // Del disco, con la ruta tal como se escribió (no hace falta un repo).
        None => {
            let disk = std::path::Path::new(file);
            let content = std::fs::read(disk).map_err(|e| format!("{file}: {e}"))?;
            // Las sub-celdas de Magic se buscan junto al archivo.
            let dir = disk.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
            let files = riku_kernel::DiffFiles::new(None, crate::core::git::files::workdir_files(Some(dir)));
            crate::export::image(&crate::modules::registry(), file, None, content, files, "worktree", &req, &opts)?
        }
        Some(r) => {
            let svc = GitService::open(&repo).map_err(|e| e.to_string())?;
            let workdir = svc.root().map(|p| p.to_path_buf());
            let file = repo_file(&repo, workdir.as_deref(), file);
            export_one(&svc, workdir.as_deref(), &Side::Rev(r.to_string()), &file, &req, &opts)?
        }
    };
    println!("{}", path.display());
    Ok(())
}

/// `file` como ruta del repo, nombrado desde `base` (ver
/// [`to_repo_path`](crate::core::repo_path::to_repo_path)); tal cual fuera
/// de un repo.
fn repo_file(base: &std::path::Path, workdir: Option<&std::path::Path>, file: &str) -> String {
    match workdir {
        Some(w) => crate::core::repo_path::to_repo_path(base, w, file),
        None => file.to_string(),
    }
}

/// Versión corta para títulos y nombres de archivo (`a3f2b1c`, `HEAD~1`, `worktree`).
fn short_label(side: &Side) -> String {
    match side {
        Side::Rev(r) if r.len() == 40 && r.chars().all(|c| c.is_ascii_hexdigit()) => r[..7].to_string(),
        other => other.label().to_string(),
    }
}

/// Interpreta `[A] [B] [ARCHIVO]`. El último argumento es un archivo si
/// algún módulo conoce su extensión o si existe en el working tree; si no,
/// es un commit.
fn resolve_targets(
    targets: &[String],
    modules: &riku_kernel::Registry,
    workdir: Option<&std::path::Path>,
    base: &std::path::Path,
) -> Result<(Side, Side, Option<String>), String> {
    let is_file = |t: &str| {
        modules.for_path(t).is_some() || base.join(t).is_file() || workdir.is_some_and(|w| w.join(t).is_file())
    };
    let rev = |t: &str| Side::Rev(t.to_string());
    Ok(match targets {
        [] => (rev("HEAD"), Side::WorkTree, None),
        [f] if is_file(f) => (rev("HEAD"), Side::WorkTree, Some(f.clone())),
        [a] => (rev(a), Side::WorkTree, None),
        [a, f] if is_file(f) => (rev(a), Side::WorkTree, Some(f.clone())),
        [a, b] => (rev(a), rev(b), None),
        [a, b, f] => (rev(a), rev(b), Some(f.clone())),
        _ => return Err(tr!("err.too_many_targets")),
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
    pretty: bool,
    overrides: Overrides,
    image: Option<crate::export::Request>,
) -> Result<Changes, String> {
    let svc = GitService::open(&repo).map_err(|e| e.to_string())?;
    let opts = config::options_for(&repo, overrides)?;
    let file_path = file_path.map(|f| repo_file(&repo, svc.root(), f));
    let file_path = file_path.as_deref();
    if let Some(req) = image {
        let file = file_path.ok_or_else(|| tr!("err.image_needs_file"))?;
        let changes = svc.commit_changes(commit).map_err(|e| e.to_string())?;
        // El commit inicial se compara contra vacío (sin "antes").
        let from = changes.commit.parents.first().map(|p| Side::Rev(p.clone()));
        let to = Side::Rev(changes.commit.info.oid.clone());
        let workdir = svc.root().map(|p| p.to_path_buf());
        let path = match from {
            Some(from) => export_between(&svc, workdir.as_deref(), &from, &to, file, &req, &opts)?,
            None => export_one(&svc, workdir.as_deref(), &to, file, &req, &opts)?,
        };
        println!("{}", path.display());
        return Ok(Changes::Clean);
    }
    if matches!(format, OutputFormat::Visual) {
        let Some(file) = file_path else {
            return Err(tr!("err.show_visual_needs_file"));
        };
        let changes = svc.commit_changes(commit).map_err(|e| e.to_string())?;
        let Some(parent) = changes.commit.parents.first() else {
            return Err(tr!("err.initial_commit", commit = commit, file = file));
        };
        return present_visual(&repo, parent, &changes.commit.info.oid, file, &opts.expressions).map(|_| Changes::Clean);
    }
    if matches!(format, OutputFormat::JsonV1) {
        return Err(tr!("err.show_no_v1"));
    }

    let report = analyze_show(&svc, commit, file_path, &crate::modules::registry(), &opts).map_err(|e| e.to_string())?;
    match format {
        OutputFormat::Json => format::show_json::print(&report, pretty)?,
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
    pretty: bool,
) -> Result<(), String> {
    for w in warnings {
        eprintln!("[!] {w}");
    }
    match format {
        OutputFormat::Text => format::diff_text::print(report, file_path),
        OutputFormat::Json => format::diff_json::print(report, warnings, file_path, from.label(), to.label(), pretty),
        OutputFormat::JsonV1 => format::diff_json::print_v1(report, warnings, file_path, pretty),
        OutputFormat::Visual | OutputFormat::Png | OutputFormat::Svg => unreachable!("se atienden antes de imprimir"),
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
        let workdir = git2::Repository::discover(&args.repo).ok().and_then(|r| r.workdir().map(|w| w.to_path_buf()));
        paths.push(repo_file(&args.repo, workdir.as_deref(), &fp));
    }

    let opts = log::LogOptions {
        level,
        paths,
        limit: Some(args.limit),
        start: args.branch,
        graph: args.graph,
        skip_summaries: false,
        diff: config::options_for(&args.repo, Overrides::default())?,
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
        diff: config::options_for(&args.repo, Overrides::default())?,
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
        resolve_targets(&args, &crate::modules::registry(), workdir, std::path::Path::new(".")).unwrap()
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
        assert!(resolve_targets(&args, &crate::modules::registry(), None, std::path::Path::new(".")).is_err());
    }
}
