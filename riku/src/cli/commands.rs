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
use crate::core::analysis::log;
use crate::core::analysis::show::analyze_show;
use crate::core::analysis::status::{self, StatusOptions};
use crate::core::analysis::summary::{DetailLevel, SummaryCategory};
use crate::core::domain::models::FileChange;
use crate::core::domain::ports::{GitRepository, RepoRoot};
use crate::core::git::git_service::GitService;

use super::format;
use super::gui;
use super::OutputFormat;

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
        // Sin archivo: el visor abre la lista de todos los que cambiaron.
        let exprs = config::options_for(&repo, overrides)?.expressions;
        return present_visual(&repo, from.token(), to.token(), file.as_deref(), &exprs).map(|_| Changes::Clean);
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
            let report =
                diff_set::analyze_all(&svc, workdir.as_deref(), &from, &to, &modules, &opts).map_err(|e| e.to_string())?;
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
fn read_version(svc: &GitService, workdir: Option<&std::path::Path>, side: &Side, file: &str) -> Result<Option<Vec<u8>>, String> {
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
    let is_file =
        |t: &str| modules.for_path(t).is_some() || base.join(t).is_file() || workdir.is_some_and(|w| w.join(t).is_file());
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
        if functional {
            Changes::Functional
        } else {
            Changes::Clean
        }
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
        // Sin archivo: la lista de todo lo que cambió el commit.
        let changes = svc.commit_changes(commit).map_err(|e| e.to_string())?;
        let Some(parent) = changes.commit.parents.first() else {
            return Err(match file_path {
                Some(file) => tr!("err.initial_commit", commit = commit, file = file),
                None => tr!("err.show_visual_needs_file"),
            });
        };
        return present_visual(&repo, parent, &changes.commit.info.oid, file_path, &opts.expressions).map(|_| Changes::Clean);
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
        OutputFormat::Visual | OutputFormat::Png | OutputFormat::Svg => unreachable!("se atienden antes de imprimir"),
    }
}

fn present_visual(
    repo: &PathBuf,
    commit_a: &str,
    commit_b: &str,
    file_path: Option<&str>,
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
    ];
    extra_args.extend(file_path.map(Into::into));
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
    pub color: Option<format::color::ColorMode>,
    pub lvs: bool,
}

pub(super) fn run_log(args: LogArgs) -> Result<(), String> {
    // El modo vale solo para este comando: el shell interactivo sigue vivo después.
    format::color::set_mode(args.color.unwrap_or_default());
    let result = run_log_inner(args);
    format::color::set_mode(format::color::ColorMode::Auto);
    result
}

fn run_log_inner(args: LogArgs) -> Result<(), String> {
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
    let mut report = log::analyze_with_options_path(&args.repo, &opts, &crate::modules::registry()).map_err(|e| e.to_string())?;
    if args.lvs {
        lvs_into_log(&mut report, &args.repo, &opts.paths, level)?;
    }

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
/// `log --lvs`: el LVS de cada commit (con la caché). Nunca hace fallar el `log`.
#[cfg(all(feature = "xschem", feature = "layout"))]
fn lvs_into_log(report: &mut log::LogReport, repo: &std::path::Path, paths: &[String], level: DetailLevel) -> Result<(), String> {
    let runs = crate::lvs::annotate::annotate_log(report, repo, paths, level);
    if runs > 0 {
        eprintln!("{}", tr!("lvs.new_runs", count = runs));
    }
    Ok(())
}

#[cfg(not(all(feature = "xschem", feature = "layout")))]
fn lvs_into_log(_: &mut log::LogReport, _: &std::path::Path, _: &[String], _: DetailLevel) -> Result<(), String> {
    Err(tr!("lvs.not_built"))
}

/// `status --lvs`: el working tree contra `HEAD`.
#[cfg(all(feature = "xschem", feature = "layout"))]
fn lvs_into_status(
    report: &mut status::StatusReport,
    repo: &std::path::Path,
    paths: &[String],
    level: DetailLevel,
) -> Result<(), String> {
    let runs = crate::lvs::annotate::annotate_status(report, repo, paths, level)?;
    if runs > 0 {
        eprintln!("{}", tr!("lvs.new_runs", count = runs));
    }
    Ok(())
}

#[cfg(not(all(feature = "xschem", feature = "layout")))]
fn lvs_into_status(_: &mut status::StatusReport, _: &std::path::Path, _: &[String], _: DetailLevel) -> Result<(), String> {
    Err(tr!("lvs.not_built"))
}

pub(super) struct StatusArgs {
    pub repo: PathBuf,
    pub include_unknown: bool,
    pub json: bool,
    pub compact: bool,
    pub detail: bool,
    pub full: bool,
    pub paths: Vec<String>,
    pub lvs: bool,
}

pub(super) fn run_status(args: StatusArgs) -> Result<Changes, String> {
    let level = DetailLevel::from_flags(args.detail, args.full);

    let opts = StatusOptions { level, paths: args.paths, diff: config::options_for(&args.repo, Overrides::default())? };
    let mut report =
        status::analyze_with_options_path(&args.repo, &opts, &crate::modules::registry()).map_err(|e| e.to_string())?;
    if args.lvs {
        lvs_into_status(&mut report, &args.repo, &opts.paths, level)?;
    }

    if args.json {
        format::status_json::print(&report, !args.compact)?;
    } else {
        format::status_text::print(&report, level, args.include_unknown);
    }

    // Con `--lvs`, el código de salida es el del LVS (ver `lvs_types::status_outcome`).
    if args.lvs {
        let (code, new) = crate::core::analysis::lvs_types::status_outcome(&report.lvs);
        if new {
            eprintln!("[!] {}", tr!("lvs.status_new"));
        }
        return Ok(match code {
            0 => Changes::Clean,
            1 => Changes::Functional,
            _ => Changes::Failed,
        });
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

/// `riku lvs`: el layout contra el esquemático en una versión (ver
/// [`crate::lvs`]). Sin `--sch`/`--layout`, los pares de `.riku.toml` o los
/// de igual nombre.
#[cfg(all(feature = "xschem", feature = "layout"))]
pub(super) fn run_lvs(
    repo: PathBuf,
    rev: Option<&str>,
    pair: Option<(String, String)>,
    cell: Option<String>,
    json: bool,
) -> Result<super::dispatch::Outcome, String> {
    use super::dispatch::Outcome;
    use crate::lvs::{self, Pair, Tree, Verdict};

    let tools = lvs::tools()?;
    let (root, configured) = lvs_configured(&repo, pair, &cell)?;
    let tree = match rev {
        None => Tree::disk(&root),
        Some(r) => Tree::commit(&repo, r)?,
    };
    let (found, ambiguous) = lvs::pairs_checked(&tree.root, &configured);
    for w in &ambiguous {
        eprintln!("[!] {w}");
    }
    let pairs: Vec<Pair> = found.into_iter().map(|p| Pair { cell: p.cell.or_else(|| cell.clone()), ..p }).collect();
    if pairs.is_empty() {
        return Err(tr!("lvs.none_found"));
    }
    let version = rev.unwrap_or("worktree");
    let results: Vec<Result<lvs::Report, (Pair, String)>> =
        pairs.iter().map(|p| lvs::run(&tree, p, &tools).map_err(|e| (p.clone(), e))).collect();

    if json {
        let items: Vec<serde_json::Value> = results
            .iter()
            .map(|r| match r {
                Ok(rep) => serde_json::to_value(rep).unwrap_or_default(),
                Err((p, e)) => serde_json::json!({ "schematic": p.schematic, "layout": p.layout, "error": e }),
            })
            .collect();
        super::format::print_enveloped(
            &serde_json::json!({ "schema": lvs::SCHEMA, "version": version, "results": items }),
            true,
        )?;
    } else {
        for r in &results {
            match r {
                Ok(rep) => print_lvs(rep, version),
                Err((p, e)) => println!(
                    "LVS  {} ↔ {}
  {}
",
                    p.schematic,
                    p.layout,
                    tr!("lvs.error", error = e)
                ),
            }
        }
    }
    Ok(if results.iter().any(Result::is_err) {
        Outcome::Failed
    } else if results.iter().any(|r| r.as_ref().is_ok_and(|rep| rep.comparison.result != Verdict::Match)) {
        Outcome::Functional
    } else {
        Outcome::Clean
    })
}

/// `riku lvs --map`: el LVS manual de cada par (ver [`crate::lvs::manual`]).
/// `suggest` agrega los vínculos que se deducen y `update` reescribe las
/// posiciones después de un movimiento: los dos escriben el archivo, solo
/// en el disco.
#[cfg(all(feature = "xschem", feature = "layout"))]
pub(super) fn run_lvs_map(
    repo: PathBuf,
    rev: Option<&str>,
    pair: Option<(String, String)>,
    cell: Option<String>,
    json: bool,
    suggest: bool,
    update: bool,
) -> Result<super::dispatch::Outcome, String> {
    use super::dispatch::Outcome;
    use crate::lvs::{self, manual, Pair, Tree};

    if (suggest || update) && rev.is_some() {
        return Err(tr!("lvs_map.disk_only"));
    }
    let (root, configured) = lvs_configured(&repo, pair, &cell)?;
    let tree = match rev {
        None => Tree::disk(&root),
        Some(r) => Tree::commit(&repo, r)?,
    };
    let (found, ambiguous) = lvs::pairs_checked(&tree.root, &configured);
    for w in &ambiguous {
        eprintln!("[!] {w}");
    }
    let pairs: Vec<Pair> = found.into_iter().map(|p| Pair { cell: p.cell.or_else(|| cell.clone()), ..p }).collect();
    if pairs.is_empty() {
        return Err(tr!("lvs.none_found"));
    }
    let version = rev.unwrap_or("worktree");
    let (mut failed, mut pending) = (false, false);
    let mut items = Vec::new();
    for p in &pairs {
        let mut s = match manual::load(&tree, p, rev.map(|_| root.as_path())) {
            Ok(s) => s,
            Err(e) => {
                failed = true;
                if json {
                    items.push(serde_json::json!({ "schematic": p.schematic, "layout": p.layout, "error": e }));
                } else {
                    println!("{}\n  {}\n", tr!("lvs_map.title", schematic = p.schematic, layout = p.layout, cell = "?", version = version), tr!("lvs.error", error = e));
                }
                continue;
            }
        };
        let mut notes = Vec::new();
        if suggest {
            let new = manual::suggest(&s.map, &s.schematic, &s.layout);
            let names: Vec<&str> = new.iter().map(|b| b.schematic.as_str()).collect();
            notes.push(tr!("lvs_map.suggested", count = new.len(), names = names.join(", ")));
            s.map.binds.extend(new);
        }
        let c = manual::check(&s.map, &s.schematic, &s.layout);
        if update {
            s.map = c.updated.clone();
        }
        if suggest || update {
            let path = root.join(&s.map_path);
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::write(&path, s.map.to_text()).map_err(|e| format!("{}: {e}", path.display()))?;
            notes.push(tr!("lvs_map.saved", file = s.map_path));
            s.exists = true;
        }
        pending |= !c.clean();
        if json {
            let mut v = manual::check_json(&c, &s);
            v["schematic"] = p.schematic.clone().into();
            v["layout"] = p.layout.clone().into();
            items.push(v);
        } else {
            print_lvs_map(p, &s, &c, version, &notes);
        }
    }
    if json {
        super::format::print_enveloped(&serde_json::json!({ "schema": manual::CHECK_SCHEMA, "version": version, "results": items }), true)?;
    }
    Ok(if failed {
        Outcome::Failed
    } else if pending {
        Outcome::Functional
    } else {
        Outcome::Clean
    })
}

/// Un resultado de `riku lvs --map` en texto.
#[cfg(all(feature = "xschem", feature = "layout"))]
fn print_lvs_map(p: &crate::lvs::Pair, s: &crate::lvs::manual::Session, c: &crate::lvs::manual::Check, version: &str, notes: &[String]) {
    println!("{}", tr!("lvs_map.title", schematic = p.schematic, layout = p.layout, cell = s.cell, version = version));
    for n in notes {
        println!("  {n}");
    }
    if !s.exists {
        println!("  {}", tr!("lvs_map.no_file", file = s.map_path));
    } else if s.from_disk {
        println!("  {}", tr!("lvs_map.from_disk", file = s.map_path));
    }
    let fingers: usize = c.bound.iter().map(|(_, f)| f.len()).sum();
    println!(
        "  {}",
        tr!("lvs_map.progress", sch = c.bound.len(), sch_total = s.schematic.len(), lay = fingers, lay_total = s.layout.len())
    );
    if let Some(m) = c.moved {
        let mirror = if m.orient >= 4 { tr!("lvs_map.mirrored") } else { String::new() };
        let (dx, dy) = (format!("{:.3}", m.dx), format!("{:.3}", m.dy));
        println!("  {}", tr!("lvs_map.moved", angle = (m.orient % 4) as u32 * 90, mirror = mirror, dx = dx, dy = dy, count = m.count));
    }
    if !c.by_connectivity.is_empty() {
        println!("  {}", tr!("lvs_map.by_connectivity", names = c.by_connectivity.join(", ")));
    }
    for (d, what) in &c.models {
        println!("  {}", tr!("lvs_map.model", device = d, what = what));
    }
    for (d, what) in &c.params {
        println!("  {}", tr!("lvs_map.param", device = d, what = what));
    }
    for (n, ns) in &c.shorts {
        println!("  {}", tr!("lvs_map.short", net = n, nets = ns.join(", ")));
    }
    for (n, ns) in &c.opens {
        println!("  {}", tr!("lvs_map.open", net = n, nets = ns.join(", ")));
    }
    for (d, r) in &c.lost {
        let at = format!("({:.3}, {:.3})", r.at[0], r.at[1]);
        println!("  {}", tr!("lvs_map.lost", device = d, model = r.model, at = at));
    }
    for d in &c.unknown {
        println!("  {}", tr!("lvs_map.unknown", device = d));
    }
    if !c.unbound_schematic.is_empty() {
        println!("  {}", tr!("lvs_map.unbound_sch", names = c.unbound_schematic.join(", ")));
    }
    if !c.unbound_layout.is_empty() {
        let list: Vec<String> = c
            .unbound_layout
            .iter()
            .map(|&i| {
                let d = &s.layout[i];
                format!("{} ({:.3}, {:.3})", d.model.rsplit("__").next().unwrap_or(&d.model), d.at.0, d.at.1)
            })
            .collect();
        println!("  {}", tr!("lvs_map.unbound_lay", list = list.join(", ")));
    }
    println!("  {}\n", if c.clean() { tr!("lvs_map.clean") } else { tr!("lvs_map.pending") });
}

/// Un resultado de `riku lvs` en texto.
#[cfg(all(feature = "xschem", feature = "layout"))]
fn print_lvs(r: &crate::lvs::Report, version: &str) {
    use crate::lvs::Verdict;
    let c = &r.comparison;
    println!("LVS  {} ↔ {} ({}) · {version} · {}", r.pair.schematic, r.pair.layout, r.layout_cell, r.pdk);
    let result = match c.result {
        Verdict::Match => tr!("lvs.result_match"),
        Verdict::PropertyErrors => tr!("lvs.result_properties"),
        Verdict::Mismatch => tr!("lvs.result_mismatch"),
    };
    println!("  {result}");
    let count = |m: &std::collections::BTreeMap<String, u64>| m.values().sum::<u64>();
    println!(
        "  {}",
        tr!(
            "lvs.counts",
            dev_l = count(&c.devices.layout),
            dev_s = count(&c.devices.schematic),
            net_l = c.nets.layout,
            net_s = c.nets.schematic
        )
    );
    if !c.properties.is_empty() {
        println!("  {}", tr!("lvs.properties", count = c.properties.len()));
        for p in &c.properties {
            let diffs: Vec<String> = p
                .values
                .iter()
                .filter(|v| v.layout != v.schematic)
                .map(|v| format!("{} {} ≠ {}", v.name, v.schematic, v.layout))
                .collect();
            println!("    {} ↔ {} ({}): {}", p.schematic, p.layout, p.model, diffs.join(", "));
        }
    }
    let show = |label: String, groups: &[crate::lvs::Sides<Vec<String>>]| {
        if groups.is_empty() {
            return;
        }
        println!("  {label}");
        for g in groups {
            println!("    {} {} · {} {}", tr!("lvs.side_schematic"), list(&g.schematic), tr!("lvs.side_layout"), list(&g.layout));
        }
    };
    show(tr!("lvs.unmatched_nets"), &c.unmatched_nets);
    show(tr!("lvs.unmatched_devices"), &c.unmatched_devices);
    if c.pins.layout.len() != c.pins.schematic.len() {
        println!("  {}", tr!("lvs.pins", layout = list(&c.pins.layout), schematic = list(&c.pins.schematic)));
    }
    for w in &r.warnings {
        println!("  [!] {w}");
    }
    if !c.summary.is_empty() {
        println!("  Netgen: {}", c.summary.join(" "));
    }
    println!();
}

#[cfg(all(feature = "xschem", feature = "layout"))]
fn list(items: &[String]) -> String {
    if items.is_empty() {
        "—".into()
    } else {
        items.join(", ")
    }
}

/// La raíz del repo (o `repo`, si no es uno) y los pares pedidos: `--sch` y
/// `--layout`, o los de `.riku.toml` (vacío: los de igual nombre).
#[cfg(all(feature = "xschem", feature = "layout"))]
fn lvs_configured(
    repo: &std::path::Path,
    pair: Option<(String, String)>,
    cell: &Option<String>,
) -> Result<(PathBuf, Vec<crate::lvs::Pair>), String> {
    use crate::lvs::Pair;
    let root = git2::Repository::discover(repo)
        .ok()
        .and_then(|r| r.workdir().map(|w| w.to_path_buf()))
        .unwrap_or_else(|| repo.to_path_buf());
    let configured = match pair {
        Some((s, l)) => vec![Pair {
            schematic: repo_file(repo, Some(&root), &s),
            layout: repo_file(repo, Some(&root), &l),
            cell: cell.clone(),
        }],
        None => config::load(Some(&root))?
            .lvs
            .into_iter()
            .map(|c| Pair { schematic: c.schematic, layout: c.layout, cell: c.cell.or_else(|| cell.clone()) })
            .collect(),
    };
    Ok((root, configured))
}

/// `riku lvs --log`: el LVS de cada par en los últimos commits, y dónde dejó
/// de coincidir o volvió a coincidir.
#[cfg(all(feature = "xschem", feature = "layout"))]
pub(super) fn run_lvs_log(
    repo: PathBuf,
    from: &str,
    limit: usize,
    pair: Option<(String, String)>,
    cell: Option<String>,
    json: bool,
) -> Result<super::dispatch::Outcome, String> {
    use super::dispatch::Outcome;
    use crate::lvs::{self, Pair, StepResult, Transition, Tree, Verdict};

    let tools = lvs::tools()?;
    let (_, configured) = lvs_configured(&repo, pair, &cell)?;
    // Los pares se buscan en el commit de partida.
    let pairs: Vec<Pair> = {
        let tree = Tree::commit(&repo, from)?;
        let (found, ambiguous) = lvs::pairs_checked(&tree.root, &configured);
        for w in &ambiguous {
            eprintln!("[!] {w}");
        }
        found.into_iter().map(|p| Pair { cell: p.cell.or_else(|| cell.clone()), ..p }).collect()
    };
    if pairs.is_empty() {
        return Err(tr!("lvs.none_found"));
    }
    let mut cache = lvs::Cache::new();
    let history = lvs::history(&repo, from, limit.max(1), &pairs, &tools, &mut cache)?;
    if cache.runs > 0 {
        eprintln!("{}", tr!("lvs.new_runs", count = cache.runs));
    }

    if json {
        let items: Vec<serde_json::Value> = history
            .iter()
            .map(|(p, steps)| serde_json::json!({ "schematic": p.schematic, "layout": p.layout, "cell": p.cell, "commits": steps }))
            .collect();
        super::format::print_enveloped(&serde_json::json!({ "schema": "riku-lvs-log/v1", "from": from, "pairs": items }), true)?;
    } else {
        for (p, steps) in &history {
            println!("LVS  {} ↔ {}  ({})", p.schematic, p.layout, tr!("lvs.log_range", count = steps.len(), from = from));
            for s in steps {
                let state = match &s.result {
                    StepResult::Missing => tr!("lvs.state_missing"),
                    StepResult::Error { error } => tr!("lvs.state_error", error = error),
                    StepResult::Done { report, .. } => match report.comparison.result {
                        Verdict::Match => tr!("lvs.state_match"),
                        Verdict::PropertyErrors => tr!("lvs.state_properties", count = report.comparison.properties.len()),
                        Verdict::Mismatch => tr!("lvs.state_mismatch"),
                    },
                };
                let mark = match s.transition {
                    Some(Transition::Broke) => format!("  ← {}", tr!("lvs.broke")),
                    Some(Transition::Worse) => format!("  ← {}", tr!("lvs.worse")),
                    Some(Transition::Better) => format!("  ← {}", tr!("lvs.better")),
                    Some(Transition::Fixed) => format!("  ← {}", tr!("lvs.fixed")),
                    None => String::new(),
                };
                let date = crate::text::format_timestamp(s.time);
                let summary: String = s.summary.chars().take(40).collect();
                println!("  {}  {:16}  {:40}  {state}{mark}", s.commit, date, summary);
                if let Some(d) = &s.delta {
                    for line in super::format::lvs_text::delta_lines(d, Some(3), "             ") {
                        println!("{line}");
                    }
                }
            }
            println!();
        }
    }
    // El estado de la punta: como `riku lvs` en ese commit.
    let tip = history.iter().filter_map(|(_, steps)| steps.first().and_then(|s| s.result.verdict()));
    Ok(if history.iter().any(|(_, s)| matches!(s.first().map(|x| &x.result), Some(StepResult::Error { .. }))) {
        Outcome::Failed
    } else if tip.into_iter().any(|v| v != Verdict::Match) {
        Outcome::Functional
    } else {
        Outcome::Clean
    })
}
