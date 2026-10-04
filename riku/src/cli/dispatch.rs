//! Dispatch unificado para `Commands`.
//!
//! Único sitio donde se desestructura cada variante de `Commands` para
//! ejecutarla. Tanto `cli::run` como el REPL del shell pasan por aquí, así
//! añadir un flag a un subcomando solo requiere tocar la definición de
//! clap (en `cli/mod.rs`) y el brazo correspondiente de `execute`.

use super::commands::{self, Changes};
use super::doctor;
use super::gui;
use super::{Commands, ImageFormat, ImageTheme, ListFormat, OutputFormat};
use crate::core::config::Overrides;

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

/// Pedido de imagen para `diff`/`show` con `-f png|svg` (`None` con otro formato).
fn image_request(
    format: &OutputFormat,
    output: Option<std::path::PathBuf>,
    cell: Option<String>,
    size: &str,
    theme: ImageTheme,
) -> Result<Option<crate::export::Request>, String> {
    let png = match format {
        OutputFormat::Png => true,
        OutputFormat::Svg => false,
        _ => return Ok(None),
    };
    let (width, height) = crate::export::parse_size(size)?;
    Ok(Some(crate::export::Request { png, output, width, height, dark: theme == ImageTheme::Dark, cell }))
}

impl Commands {
    /// `true` si la salida pedida es JSON (entonces los errores también).
    pub(super) fn wants_json(&self) -> bool {
        match self {
            Commands::Diff { format, .. } | Commands::Show { format, .. } => {
                matches!(format, OutputFormat::Json)
            }
            Commands::Log { format, json, .. } | Commands::Status { format, json, .. } => *json || *format == ListFormat::Json,
            Commands::Doctor { format, .. } => *format == ListFormat::Json,
            #[cfg(all(feature = "xschem", feature = "layout"))]
            Commands::Lvs { format, .. } => *format == ListFormat::Json,
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
                compact,
                ci: _,
                output,
                cell,
                size,
                theme,
            } => {
                let o = Overrides { cosmetic_threshold_um2, tolerance, expressions: exprs, no_cache };
                let img = image_request(&format, output, cell, &size, theme)?;
                commands::run_diff(repo, &targets, format, !compact, o, img).map(Outcome::from)
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
                compact,
                ci: _,
                output,
                cell,
                size,
                theme,
            } => {
                let o = Overrides { cosmetic_threshold_um2, tolerance, expressions: exprs, no_cache };
                let img = image_request(&format, output, cell, &size, theme)?;
                commands::run_show(repo, &commit, file_path.as_deref(), format, !compact, o, img).map(Outcome::from)
            }

            Commands::Render { file, rev, repo, format, exprs, output, cell, size, theme } => {
                let (width, height) = crate::export::parse_size(&size)?;
                let req = crate::export::Request {
                    png: format == ImageFormat::Png,
                    output,
                    width,
                    height,
                    dark: theme == ImageTheme::Dark,
                    cell,
                };
                let o = Overrides { expressions: exprs, ..Overrides::default() };
                commands::run_render(repo, &file, rev.as_deref(), req, o).map(|_| Outcome::Ok)
            }

            #[cfg(all(feature = "xschem", feature = "layout"))]
            Commands::Lvs { rev, repo, sch, layout, cell, format, ci: _, log, limit } => {
                let pair = sch.zip(layout);
                let json = format == ListFormat::Json;
                if log {
                    commands::run_lvs_log(repo, rev.as_deref().unwrap_or("HEAD"), limit, pair, cell, json)
                } else {
                    commands::run_lvs(repo, rev.as_deref(), pair, cell, json)
                }
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
                color,
                lvs,
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
                color,
                lvs,
            })
            .map(|_| Outcome::Ok),

            Commands::Doctor { repo, format } => doctor::run(repo, format == ListFormat::Json).map(|_| Outcome::Ok),

            Commands::Status { repo, include_unknown, format, json, compact, detail, full, paths, ci: _, lvs } => {
                commands::run_status(commands::StatusArgs {
                    repo,
                    include_unknown,
                    json: json || format == ListFormat::Json,
                    compact,
                    detail,
                    full,
                    paths,
                    lvs,
                })
                .map(Outcome::from)
            }

            Commands::Demo { name, dir, list } => super::demo::run(name, dir, list).map(|_| Outcome::Ok),

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
