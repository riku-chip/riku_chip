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
mod format;
mod gui;
mod shell;

// ─── Tipos del parser ────────────────────────────────────────────────────────

#[derive(Clone, Debug, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Visual,
}

#[derive(Parser, Debug)]
#[command(name = "riku", about = "Riku - VCS semantico para diseno de chips")]
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
    /// Abre un archivo .sch en el visor de escritorio.
    Open { file: Option<PathBuf> },
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run() -> ExitCode {
    let cli = Cli::parse();

    // `Status` tiene exit codes propios (0 limpio, 1 con cambios, 2 error).
    // El resto de comandos sigue la convención clásica (0 ok, 1 error).
    if let Some(Commands::Status {
        repo,
        include_unknown,
        json,
        compact,
        detail,
        full,
        paths,
    }) = cli.command
    {
        return match commands::run_status(commands::StatusArgs {
            repo,
            include_unknown,
            json,
            compact,
            detail,
            full,
            paths,
        }) {
            Ok(commands::StatusOutcome::Clean) => ExitCode::SUCCESS,
            Ok(commands::StatusOutcome::Dirty) => ExitCode::from(1),
            Err(err) => {
                eprintln!("{err}");
                ExitCode::from(2)
            }
        };
    }

    let result = match cli.command {
        None => shell::run_shell(),
        Some(Commands::Diff {
            commit_a,
            commit_b,
            file_path,
            repo,
            format,
        }) => commands::run_diff(repo, &commit_a, &commit_b, &file_path, format),
        Some(Commands::Log {
            file_path,
            repo,
            limit,
            semantic: _,
            json,
            compact,
            detail,
            full,
            paths,
            branch,
        }) => commands::run_log(commands::LogArgs {
            repo,
            file_path,
            limit,
            json,
            compact,
            detail,
            full,
            paths,
            branch,
        }),
        Some(Commands::Doctor { repo }) => commands::run_doctor(repo),
        Some(Commands::Status { .. }) => unreachable!(),
        Some(Commands::Open { file }) => gui::run(file),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}
