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
    /// Verifica que el entorno este correctamente configurado.
    Doctor {
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
    },
    /// Abre un archivo .sch en el visor de escritorio.
    Open { file: Option<PathBuf> },
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        None => shell::run_shell(),
        Some(Commands::Diff { commit_a, commit_b, file_path, repo, format }) => {
            commands::run_diff(repo, &commit_a, &commit_b, &file_path, format)
        }
        Some(Commands::Log { file_path, repo, limit, semantic }) => {
            commands::run_log(repo, file_path.as_deref(), limit, semantic)
        }
        Some(Commands::Doctor { repo }) => commands::run_doctor(repo),
        Some(Commands::Open { file }) => commands::run_gui(file),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}
