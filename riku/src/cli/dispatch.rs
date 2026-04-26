//! Dispatch unificado para `Commands`.
//!
//! Único sitio donde se desestructura cada variante de `Commands` para
//! ejecutarla. Tanto `cli::run` como el REPL del shell pasan por aquí, así
//! añadir un flag a un subcomando solo requiere tocar la definición de
//! clap (en `cli/mod.rs`) y el brazo correspondiente de `execute`.

use super::Commands;
use super::commands;
use super::doctor;
use super::gui;

/// Resultado de ejecutar un comando, agnóstico al modo (CLI directa vs REPL).
/// El caller decide cómo mapearlo a exit codes (o ignorarlo, en el shell).
pub(super) enum Outcome {
    Ok,
    StatusClean,
    StatusDirty,
}

impl Commands {
    pub(super) fn execute(self) -> Result<Outcome, String> {
        match self {
            Commands::Diff {
                commit_a,
                commit_b,
                file_path,
                repo,
                format,
            } => commands::run_diff(repo, &commit_a, &commit_b, &file_path, format)
                .map(|_| Outcome::Ok),

            Commands::Log {
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
            } => commands::run_log(commands::LogArgs {
                repo,
                file_path,
                limit,
                json,
                compact,
                detail,
                full,
                paths,
                branch,
            })
            .map(|_| Outcome::Ok),

            Commands::Doctor { repo } => doctor::run(repo).map(|_| Outcome::Ok),

            Commands::Status {
                repo,
                include_unknown,
                json,
                compact,
                detail,
                full,
                paths,
            } => commands::run_status(commands::StatusArgs {
                repo,
                include_unknown,
                json,
                compact,
                detail,
                full,
                paths,
            })
            .map(|outcome| match outcome {
                commands::StatusOutcome::Clean => Outcome::StatusClean,
                commands::StatusOutcome::Dirty => Outcome::StatusDirty,
            }),

            Commands::Open { file } => gui::run(file).map(|_| Outcome::Ok),
        }
    }
}
