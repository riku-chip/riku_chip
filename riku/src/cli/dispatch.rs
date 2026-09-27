//! Dispatch unificado para `Commands`.
//!
//! Único sitio donde se desestructura cada variante de `Commands` para
//! ejecutarla. Tanto `cli::run` como el REPL del shell pasan por aquí, así
//! añadir un flag a un subcomando solo requiere tocar la definición de
//! clap (en `cli/mod.rs`) y el brazo correspondiente de `execute`.

use super::Commands;
use super::commands::{self, Changes};
use super::doctor;
use super::gui;

/// Resultado de ejecutar un comando, agnóstico al modo (CLI directa vs REPL).
/// El caller decide cómo mapearlo a exit codes (o ignorarlo, en el shell).
pub(super) enum Outcome {
    Ok,
    /// Sin cambios, o solo cosméticos (`status`, `diff`, `show`).
    Clean,
    /// Hay al menos un cambio funcional (`status`, `diff`, `show`).
    Functional,
}

impl From<Changes> for Outcome {
    fn from(c: Changes) -> Self {
        match c {
            Changes::Clean => Outcome::Clean,
            Changes::Functional => Outcome::Functional,
        }
    }
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
                cosmetic_threshold_um2,
                no_cache,
                exprs,
                ci: _,
            } => commands::run_diff(
                repo,
                &commit_a,
                &commit_b,
                &file_path,
                format,
                cosmetic_threshold_um2,
                !no_cache,
                exprs,
            )
            .map(Outcome::from),

            Commands::Show {
                commit,
                file_path,
                repo,
                format,
                cosmetic_threshold_um2,
                no_cache,
                exprs,
                ci: _,
            } => commands::run_show(repo, &commit, file_path.as_deref(), format, cosmetic_threshold_um2, !no_cache, exprs)
                .map(Outcome::from),

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
                graph,
                ascii,
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
                graph,
                ascii,
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
            .map(Outcome::from),

            Commands::Open { file } => gui::run(file).map(|_| Outcome::Ok),
            Commands::Gui { args } => gui::run_here(args).map(|_| Outcome::Ok),
        }
    }
}
