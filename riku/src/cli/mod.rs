//! Capa CLI de Riku.
//!
//! - `commands`: ejecutores de cada comando (`run_diff`, `run_log`, ...).
//! - `shell`: REPL interactivo, capa delgada sobre `commands`.
//!
//! Este módulo solo define los tipos del parser (clap) y despacha al ejecutor
//! correspondiente. El shell reusa el mismo parser para garantizar paridad
//! absoluta entre los dos modos de uso.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};

use crate::i18n::tr;

mod commands;
mod dispatch;
pub(crate) mod doctor;
pub(crate) mod format;
mod gui;
mod shell;
mod shell_complete;

// ─── Tipos del parser ────────────────────────────────────────────────────────
//
// La ayuda sale de `riku/locales/cli.yml` (`help = tr!(…)`), así que va en el
// idioma de RIKU_LANG. Por eso los campos no llevan comentarios `///`: clap los
// tomaría como ayuda en español.

// Formato de `log`, `status` y `doctor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ListFormat {
    Text,
    Json,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum OutputFormat {
    Text,
    // JSON con cambios tipados (schema riku-diff/v2).
    Json,
    // JSON anterior (componentes con strings); se mantiene una versión.
    JsonV1,
    Visual,
    // Imagen (sin ventana): `riku diff … -f png`.
    Png,
    Svg,
}

/// Formato de `riku render`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ImageFormat {
    Png,
    Svg,
}

/// Tema de las imágenes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ImageTheme {
    Light,
    Dark,
}

#[derive(Parser, Debug)]
#[command(
    name = "riku",
    version,
    about = tr!("help.about"),
    long_about = tr!("help.long_about"),
    after_help = tr!("help.examples_main")
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
    #[arg(long, global = true, value_name = "N", help = tr!("help.jobs"))]
    pub(crate) jobs: Option<usize>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    #[command(about = tr!("help.diff"), after_help = tr!("help.examples_diff"))]
    Diff {
        #[arg(value_name = "A B FILE", num_args = 0..=3, help = tr!("help.diff_targets"))]
        targets: Vec<String>,
        #[arg(short, long, default_value = ".", help = tr!("help.repo"))]
        repo: PathBuf,
        #[arg(short = 'f', long, value_enum, default_value_t = OutputFormat::Text, help = tr!("help.format_diff"))]
        format: OutputFormat,
        #[arg(long = "cosmetic-threshold-um2", value_name = "UM2", help = tr!("help.cosmetic"))]
        cosmetic_threshold_um2: Option<f64>,
        #[arg(long, value_name = "TOL", value_parser = crate::core::config::parse_fraction, help = tr!("help.tolerance"))]
        tolerance: Option<f64>,
        #[arg(long = "no-cache", help = tr!("help.no_cache"))]
        no_cache: bool,
        #[arg(long = "expr", value_name = "EXPR", help = tr!("help.expr"))]
        exprs: Vec<String>,
        #[arg(long, help = tr!("help.compact"))]
        compact: bool,
        #[arg(long, help = tr!("help.ci"))]
        ci: bool,
        #[arg(short = 'o', long, value_name = "FILE", help = tr!("help.output"))]
        output: Option<PathBuf>,
        #[arg(long, value_name = "CELL", help = tr!("help.cell"))]
        cell: Option<String>,
        #[arg(long, value_name = "WxH", default_value = "1600x1000", help = tr!("help.size"))]
        size: String,
        #[arg(long, value_enum, default_value_t = ImageTheme::Light, help = tr!("help.theme"))]
        theme: ImageTheme,
    },
    #[command(about = tr!("help.show"), after_help = tr!("help.examples_show"))]
    Show {
        #[arg(help = tr!("help.show_commit"))]
        commit: String,
        #[arg(help = tr!("help.show_file"))]
        file_path: Option<String>,
        #[arg(short, long, default_value = ".", help = tr!("help.repo"))]
        repo: PathBuf,
        #[arg(short = 'f', long, value_enum, default_value_t = OutputFormat::Text, help = tr!("help.format_show"))]
        format: OutputFormat,
        #[arg(long = "cosmetic-threshold-um2", value_name = "UM2", help = tr!("help.cosmetic"))]
        cosmetic_threshold_um2: Option<f64>,
        #[arg(long, value_name = "TOL", value_parser = crate::core::config::parse_fraction, help = tr!("help.tolerance"))]
        tolerance: Option<f64>,
        #[arg(long = "no-cache", help = tr!("help.no_cache"))]
        no_cache: bool,
        #[arg(long = "expr", value_name = "EXPR", help = tr!("help.expr"))]
        exprs: Vec<String>,
        #[arg(long, help = tr!("help.compact"))]
        compact: bool,
        #[arg(long, help = tr!("help.ci"))]
        ci: bool,
        #[arg(short = 'o', long, value_name = "FILE", help = tr!("help.output"))]
        output: Option<PathBuf>,
        #[arg(long, value_name = "CELL", help = tr!("help.cell"))]
        cell: Option<String>,
        #[arg(long, value_name = "WxH", default_value = "1600x1000", help = tr!("help.size"))]
        size: String,
        #[arg(long, value_enum, default_value_t = ImageTheme::Light, help = tr!("help.theme"))]
        theme: ImageTheme,
    },
    #[command(about = tr!("help.render"), after_help = tr!("help.examples_render"))]
    Render {
        #[arg(help = tr!("help.render_file", exts = crate::modules::registry().openable_text()))]
        file: String,
        #[arg(long, value_name = "REV", help = tr!("help.render_rev"))]
        rev: Option<String>,
        #[arg(short, long, default_value = ".", help = tr!("help.repo"))]
        repo: PathBuf,
        #[arg(short = 'f', long, value_enum, default_value_t = ImageFormat::Png, help = tr!("help.format_image"))]
        format: ImageFormat,
        #[arg(long = "expr", value_name = "EXPR", help = tr!("help.expr"))]
        exprs: Vec<String>,
        #[arg(short = 'o', long, value_name = "FILE", help = tr!("help.output"))]
        output: Option<PathBuf>,
        #[arg(long, value_name = "CELL", help = tr!("help.cell"))]
        cell: Option<String>,
        #[arg(long, value_name = "WxH", default_value = "1600x1000", help = tr!("help.size"))]
        size: String,
        #[arg(long, value_enum, default_value_t = ImageTheme::Light, help = tr!("help.theme"))]
        theme: ImageTheme,
    },
    #[command(about = tr!("help.log"), after_help = tr!("help.examples_log"))]
    Log {
        #[arg(help = tr!("help.log_file"))]
        file_path: Option<String>,
        #[arg(short, long, default_value = ".", help = tr!("help.repo"))]
        repo: PathBuf,
        #[arg(short = 'n', long, default_value_t = 20, help = tr!("help.limit"))]
        limit: usize,
        // Sin efecto: el log siempre es semántico (compatibilidad).
        #[arg(short = 's', long, hide = true)]
        semantic: bool,
        #[arg(short = 'f', long, value_enum, default_value_t = ListFormat::Text, help = tr!("help.format_list"))]
        format: ListFormat,
        // Igual que `-f json` (compatibilidad).
        #[arg(long, hide = true)]
        json: bool,
        #[arg(long, help = tr!("help.compact"))]
        compact: bool,
        #[arg(long, conflicts_with = "full", help = tr!("help.detail"))]
        detail: bool,
        #[arg(long, help = tr!("help.full"))]
        full: bool,
        #[arg(long = "paths", value_name = "PAT", help = tr!("help.paths"))]
        paths: Vec<String>,
        #[arg(long, value_name = "REF", help = tr!("help.branch"))]
        branch: Option<String>,
        #[arg(long, help = tr!("help.graph"))]
        graph: bool,
        #[arg(long, requires = "graph", help = tr!("help.ascii"))]
        ascii: bool,
    },
    #[command(about = tr!("help.doctor"))]
    Doctor {
        #[arg(short, long, default_value = ".", help = tr!("help.repo"))]
        repo: PathBuf,
        #[arg(short = 'f', long, value_enum, default_value_t = ListFormat::Text, help = tr!("help.format_list"))]
        format: ListFormat,
    },
    #[command(about = tr!("help.status"), after_help = tr!("help.examples_status"))]
    Status {
        #[arg(short, long, default_value = ".", help = tr!("help.repo"))]
        repo: PathBuf,
        #[arg(long, help = tr!("help.include_unknown"))]
        include_unknown: bool,
        #[arg(short = 'f', long, value_enum, default_value_t = ListFormat::Text, help = tr!("help.format_list"))]
        format: ListFormat,
        // Igual que `-f json` (compatibilidad).
        #[arg(long, hide = true)]
        json: bool,
        #[arg(long, help = tr!("help.compact"))]
        compact: bool,
        #[arg(long, conflicts_with = "full", help = tr!("help.detail"))]
        detail: bool,
        #[arg(long, help = tr!("help.full"))]
        full: bool,
        #[arg(long = "paths", value_name = "PAT", help = tr!("help.paths"))]
        paths: Vec<String>,
        #[arg(long, help = tr!("help.status_ci"))]
        ci: bool,
    },
    #[command(about = tr!("help.completions"))]
    Completions {
        #[arg(help = tr!("help.shell"))]
        shell: clap_complete::Shell,
    },
    #[command(about = tr!("help.open", exts = crate::modules::registry().openable_text()))]
    Open { file: Option<PathBuf> },
    #[command(about = tr!("help.gui"), trailing_var_arg = true, allow_hyphen_values = true)]
    Gui {
        args: Vec<String>,
    },
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run() -> ExitCode {
    use dispatch::Outcome;

    // Idioma de la salida y de la ayuda: RIKU_LANG (en, es); inglés por defecto.
    crate::i18n::set(&crate::i18n::initial(None));
    let cli = Cli::parse();
    configure_threads(cli.jobs);
    let Some(cmd) = cli.command else {
        // El shell es interactivo: sin terminal (un script, un agente) no hay
        // quién escriba, así que se muestra la ayuda en vez de esperar.
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            use clap::CommandFactory;
            let _ = Cli::command().print_help();
            return ExitCode::SUCCESS;
        }
        return shell_to_exit(shell::run_shell());
    };
    let json = cmd.wants_json();

    // `status` (siempre) y `diff`/`show` con `--ci` usan los códigos de CI:
    // 0 sin cambios o solo cosméticos, 1 cambios funcionales, 2 error. El
    // resto sigue la convención clásica (0 ok, 1 error). Se decide antes del
    // `execute`, que consume el comando.
    let ci_codes = match &cmd {
        Commands::Status { .. } => true,
        Commands::Diff { ci, .. } | Commands::Show { ci, .. } => *ci,
        _ => false,
    };
    match cmd.execute() {
        Ok(Outcome::Ok | Outcome::Clean) => ExitCode::SUCCESS,
        Ok(Outcome::Functional) => ExitCode::from(if ci_codes { 1 } else { 0 }),
        // Un archivo que no se pudo comparar: lo demás ya se imprimió, pero
        // el resultado no es confiable (un chequeo de CI no debe pasar).
        Ok(Outcome::Failed) => ExitCode::from(if ci_codes { 2 } else { 1 }),
        Err(err) => {
            // Con salida JSON, el error también es JSON (en stdout) para que
            // quien lo lee no tenga que distinguir texto de datos.
            if json {
                println!("{}", serde_json::json!({ "schema": ERROR_SCHEMA, "error": err }));
            } else {
                eprintln!("{err}");
            }
            ExitCode::from(if ci_codes { 2 } else { 1 })
        }
    }
}

/// Esquema del error cuando la salida pedida es JSON.
pub const ERROR_SCHEMA: &str = "riku-error/v1";

/// Un solo pool de hilos para todo el proceso: `--jobs N`, si no
/// `RIKU_JOBS`, si no los núcleos disponibles (lo que elige `rayon`). Los
/// diffs anidados (log → commits → celdas → pedazos) lo comparten sin crear
/// más hilos que núcleos. `RIKU_JOBS=1` deja todo en un hilo.
fn configure_threads(jobs: Option<usize>) {
    let n = jobs.or_else(|| std::env::var("RIKU_JOBS").ok()?.trim().parse().ok()).filter(|&n| n > 0);
    if let Some(n) = n {
        // Solo falla si el pool ya existe (no pasa: es lo primero que se hace).
        let _ = rayon::ThreadPoolBuilder::new().num_threads(n).build_global();
    }
}

fn shell_to_exit(r: Result<(), String>) -> ExitCode {
    r.map(|_| ExitCode::SUCCESS).unwrap_or_else(|err| {
        eprintln!("{err}");
        ExitCode::from(1)
    })
}
