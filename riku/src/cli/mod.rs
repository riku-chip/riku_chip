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

mod commands;
mod dispatch;
mod doctor;
mod format;
mod gui;
mod shell;
mod shell_complete;

// ─── Tipos del parser ────────────────────────────────────────────────────────

#[derive(Clone, Debug, ValueEnum)]
pub enum OutputFormat {
    Text,
    /// JSON con cambios tipados (schema riku-diff/v2).
    Json,
    /// JSON anterior (componentes con strings); se mantiene una versión.
    JsonV1,
    Visual,
}

#[derive(Parser, Debug)]
#[command(name = "riku", version, about = "Riku - VCS semantico para diseno de chips")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Muestra cambios semanticos entre dos commits para un archivo.
    Diff {
        commit_a: String,
        commit_b: String,
        file_path: String,
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        #[arg(short = 'f', long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
        /// Umbral en micrometros cuadrados para clasificar un cambio GDS como
        /// cosmetico (sub-DRC). Default 0.01 µm² (debajo del piso DRC en
        /// PDKs como sky130/gf180). Ignorado por drivers no-GDS.
        #[arg(long = "cosmetic-threshold-um2", default_value_t = 0.01)]
        cosmetic_threshold_um2: f64,
        /// No usar ni guardar la cache de diffs de layouts grandes
        /// (también: RIKU_NO_CACHE=1).
        #[arg(long = "no-cache")]
        no_cache: bool,
        /// Código de salida para CI: 0 sin cambios o solo cosméticos,
        /// 1 cambios funcionales, 2 error.
        #[arg(long)]
        ci: bool,
    },
    /// Muestra los cambios semanticos de un commit respecto a su padre.
    Show {
        /// Commit (hash, rama, HEAD~2…).
        commit: String,
        /// Solo este archivo; sin él, todos los que cambió el commit.
        file_path: Option<String>,
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        /// text, json (schema riku-show/v1) o visual (necesita el archivo).
        #[arg(short = 'f', long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
        /// Umbral cosmetico de layouts en µm² (como en `diff`).
        #[arg(long = "cosmetic-threshold-um2", default_value_t = 0.01)]
        cosmetic_threshold_um2: f64,
        /// No usar ni guardar la cache de diffs de layouts grandes.
        #[arg(long = "no-cache")]
        no_cache: bool,
        /// Código de salida para CI: 0 sin cambios o solo cosméticos,
        /// 1 cambios funcionales, 2 error.
        #[arg(long)]
        ci: bool,
    },
    /// Lista commits con resumen semantico por archivo.
    Log {
        /// Path posicional opcional. Equivalente a `--paths <PAT>`.
        file_path: Option<String>,
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
        /// Conservado por compatibilidad. Sin efecto: el log siempre es
        /// semantico ahora.
        #[arg(short = 's', long, hide = true)]
        semantic: bool,
        /// Salida en JSON estable (schema riku-log/v1).
        #[arg(long)]
        json: bool,
        /// JSON compacto (una linea); por defecto pretty-printed.
        #[arg(long)]
        compact: bool,
        /// Eleva el detalle: agrega entrada por componente/net cambiada.
        #[arg(long, conflicts_with = "full")]
        detail: bool,
        /// Imprime el reporte completo del driver por archivo.
        #[arg(long)]
        full: bool,
        /// Filtra por glob (puede repetirse). Ej: --paths 'amp_*.sch'.
        #[arg(long = "paths", value_name = "PAT")]
        paths: Vec<String>,
        /// Empieza desde otra ref/oid en lugar de HEAD.
        #[arg(long, value_name = "REF")]
        branch: Option<String>,
    },
    /// Verifica que el entorno este correctamente configurado.
    Doctor {
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
    },
    /// Muestra cambios semanticos en el working tree respecto a HEAD.
    Status {
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        /// Lista tambien archivos sin driver (no reconocidos por Riku).
        #[arg(long)]
        include_unknown: bool,
        /// Salida en JSON estable (schema riku-status/v1).
        #[arg(long)]
        json: bool,
        /// JSON compacto (una linea); por defecto pretty-printed.
        #[arg(long)]
        compact: bool,
        /// Eleva el detalle: agrega entrada por componente/net cambiada.
        #[arg(long, conflicts_with = "full")]
        detail: bool,
        /// Imprime el reporte completo del driver por archivo.
        #[arg(long)]
        full: bool,
        /// Filtra por glob (puede repetirse). Ej: --paths 'amp_*.sch'.
        #[arg(long = "paths", value_name = "PAT")]
        paths: Vec<String>,
    },
    /// Abre un archivo .sch, .gds u .oas en el visor de escritorio.
    Open { file: Option<PathBuf> },
    /// Abre el visor en este proceso: `riku gui [archivo] [--cell CELDA]`.
    /// `open` y `diff -f visual` lo usan por debajo.
    #[command(trailing_var_arg = true, allow_hyphen_values = true)]
    Gui {
        args: Vec<String>,
    },
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run() -> ExitCode {
    use dispatch::Outcome;

    let Some(cmd) = Cli::parse().command else {
        return shell_to_exit(shell::run_shell());
    };

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
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(if ci_codes { 2 } else { 1 })
        }
    }
}

fn shell_to_exit(r: Result<(), String>) -> ExitCode {
    r.map(|_| ExitCode::SUCCESS).unwrap_or_else(|err| {
        eprintln!("{err}");
        ExitCode::from(1)
    })
}
