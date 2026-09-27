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
pub(crate) mod format;
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
    /// Hilos para el trabajo pesado (diff de layouts, carga del visor). Por
    /// defecto, los núcleos disponibles (también: RIKU_JOBS). Se fija al
    /// arrancar; en el shell no cambia.
    #[arg(long, global = true, value_name = "N")]
    pub(crate) jobs: Option<usize>,
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
        /// Señal calculada a comparar además de las del archivo (puede
        /// repetirse), con la sintaxis de ngspice: `--expr "v(out)/v(in)"`,
        /// `--expr "gain = db(v(out))"`, `--expr "max(v(out))"`. Solo para
        /// resultados de simulación (.raw); ver docs/spice.md.
        #[arg(long = "expr", value_name = "EXPR")]
        exprs: Vec<String>,
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
        /// Señal calculada a comparar además de las del archivo (puede
        /// repetirse), con la sintaxis de ngspice: `--expr "v(out)/v(in)"`,
        /// `--expr "gain = db(v(out))"`, `--expr "max(v(out))"`. Solo para
        /// resultados de simulación (.raw); ver docs/spice.md.
        #[arg(long = "expr", value_name = "EXPR")]
        exprs: Vec<String>,
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
        /// Dibuja el grafo de ramas y merges a la izquierda (orden
        /// topológico). Con --json agrega el lugar de cada commit en el grafo.
        #[arg(long)]
        graph: bool,
        /// Grafo con caracteres ASCII en vez de Unicode (también: RIKU_ASCII=1).
        #[arg(long, requires = "graph")]
        ascii: bool,
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
    /// Abre un archivo .sch, .gds, .oas o .mag en el visor de escritorio.
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

    let cli = Cli::parse();
    configure_threads(cli.jobs);
    let Some(cmd) = cli.command else {
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
