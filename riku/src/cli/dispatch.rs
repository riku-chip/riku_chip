//! Dispatch unificado para `Commands`.
//!
//! Único sitio donde se desestructura cada variante de `Commands` para
//! ejecutarla. Tanto `cli::run` como el REPL del shell pasan por aquí, así
//! añadir un flag a un subcomando solo requiere tocar la definición de
//! clap (en `cli/mod.rs`) y el brazo correspondiente de `execute`.

use super::commands::{self, Changes};
use crate::core::config::Overrides;
use super::doctor;
use super::gui;
use super::{Commands, ListFormat, OutputFormat};

/// Resultado de ejecutar un comando, agnóstico al modo (CLI directa vs REPL).
/// El caller decide cómo mapearlo a exit codes (o ignorarlo, en el shell).
pub(super) enum Outcome {
    Ok,
    /// Sin cambios, o solo cosméticos (`status`, `diff`, `show`).
    Clean,
    /// Hay al menos un cambio funcional (`status`, `diff`, `show`).
    Functional,
    /// Algún archivo no se pudo comparar (el resto se imprimió igual).
    Failed,
}

impl From<Changes> for Outcome {
    fn from(c: Changes) -> Self {
        match c {
            Changes::Clean => Outcome::Clean,
            Changes::Functional => Outcome::Functional,
            Changes::Failed => Outcome::Failed,
        }
    }
}

impl Commands {
    /// `true` si la salida pedida es JSON (entonces los errores también).
    pub(super) fn wants_json(&self) -> bool {
        match self {
            Commands::Diff { format, .. } | Commands::Show { format, .. } => {
                matches!(format, OutputFormat::Json | OutputFormat::JsonV1)
            }
            Commands::Log { format, json, .. } | Commands::Status { format, json, .. } => {
                *json || *format == ListFormat::Json
            }
            Commands::Doctor { format, .. } => *format == ListFormat::Json,
            _ => false,
        }
    }

    pub(super) fn execute(self) -> Result<Outcome, String> {
        match self {
            Commands::Diff {
                targets,
                repo,
                format,
                cosmetic_threshold_um2,
                tolerance,
                no_cache,
                exprs,
                ci: _,
            } => {
                let o = Overrides { cosmetic_threshold_um2, tolerance, expressions: exprs, no_cache };
                commands::run_diff(repo, &targets, format, o).map(Outcome::from)
            }

            Commands::Show {
                commit,
                file_path,
                repo,
                format,
                cosmetic_threshold_um2,
                tolerance,
                no_cache,
                exprs,
                ci: _,
            } => {
                let o = Overrides { cosmetic_threshold_um2, tolerance, expressions: exprs, no_cache };
                commands::run_show(repo, &commit, file_path.as_deref(), format, o).map(Outcome::from)
            }

            Commands::Log {
                file_path,
                repo,
                limit,
                semantic: _,
                format,
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
                json: json || format == ListFormat::Json,
                compact,
                detail,
                full,
                paths,
                branch,
                graph,
                ascii,
            })
            .map(|_| Outcome::Ok),

            Commands::Doctor { repo, format } => doctor::run(repo, format == ListFormat::Json).map(|_| Outcome::Ok),

            Commands::Status {
                repo,
                include_unknown,
                format,
                json,
                compact,
                detail,
                full,
                paths,
                ci: _,
            } => commands::run_status(commands::StatusArgs {
                repo,
                include_unknown,
                json: json || format == ListFormat::Json,
                compact,
                detail,
                full,
                paths,
            })
            .map(Outcome::from),

            Commands::Completions { shell } => {
                use clap::CommandFactory;
                let mut cmd = super::Cli::command();
                clap_complete::generate(shell, &mut cmd, "riku", &mut std::io::stdout());
                Ok(Outcome::Ok)
            }

            Commands::Open { file } => gui::run(file).map(|_| Outcome::Ok),
            Commands::Gui { args } => gui::run_here(args).map(|_| Outcome::Ok),
        }
    }
}
