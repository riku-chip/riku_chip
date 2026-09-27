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

/// Formato de `log`, `status` y `doctor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ListFormat {
    Text,
    /// JSON estable, con `schema` versionado.
    Json,
}

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
#[command(
    name = "riku",
    version,
    about = "Riku: diff semántico de diseños de chips sobre Git",
    long_about = "Riku compara versiones de un diseño de chip guardado en Git: \
                  esquemáticos de Xschem (.sch), layouts (.gds, .oas, .mag) y \
                  simulaciones de ngspice (.raw). Sin comando abre un shell interactivo.",
    after_help = MAIN_EXAMPLES
)]
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
    /// Compara dos versiones (commits o el working tree), de un archivo o de todos.
    #[command(after_help = DIFF_EXAMPLES)]
    Diff {
        /// `[A] [B] [ARCHIVO]`. Sin B, se compara contra el working tree (el
        /// disco); sin A, contra HEAD; sin ARCHIVO, todos los archivos que
        /// cambiaron.
        #[arg(value_name = "A B ARCHIVO", num_args = 0..=3)]
        targets: Vec<String>,
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        #[arg(short = 'f', long, value_enum, default_value_t = OutputFormat::Text)]
        format: OutputFormat,
        /// Umbral en µm² para clasificar un cambio de layout como cosmético
        /// (sub-DRC). Por defecto 0.01 µm² (debajo del piso DRC de PDKs como
        /// sky130 y gf180), o el de `.riku.toml`. Los demás formatos lo ignoran.
        #[arg(long = "cosmetic-threshold-um2", value_name = "UM2")]
        cosmetic_threshold_um2: Option<f64>,
        /// Tolerancia de formas de onda: fracción del rango de cada señal
        /// (`0.005`) o porcentaje (`0.5%`). Por defecto 0.1 %, o la de
        /// `.riku.toml`.
        #[arg(long, value_name = "TOL", value_parser = crate::core::config::parse_fraction)]
        tolerance: Option<f64>,
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
    /// Muestra los cambios semánticos de un commit respecto a su padre.
    #[command(after_help = SHOW_EXAMPLES)]
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
        /// Umbral cosmético de layouts en µm² (como en `diff`).
        #[arg(long = "cosmetic-threshold-um2", value_name = "UM2")]
        cosmetic_threshold_um2: Option<f64>,
        /// Tolerancia de formas de onda: fracción del rango de cada señal
        /// (`0.005`) o porcentaje (`0.5%`). Por defecto 0.1 %, o la de
        /// `.riku.toml`.
        #[arg(long, value_name = "TOL", value_parser = crate::core::config::parse_fraction)]
        tolerance: Option<f64>,
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
    /// Lista commits con un resumen semántico por archivo.
    #[command(after_help = LOG_EXAMPLES)]
    Log {
        /// Path posicional opcional. Equivalente a `--paths <PAT>`.
        file_path: Option<String>,
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
        /// Conservado por compatibilidad. Sin efecto: el log siempre es
        /// semántico.
        #[arg(short = 's', long, hide = true)]
        semantic: bool,
        /// text o json (schema riku-log/v1).
        #[arg(short = 'f', long, value_enum, default_value_t = ListFormat::Text)]
        format: ListFormat,
        /// Igual que `-f json` (se mantiene por compatibilidad).
        #[arg(long, hide = true)]
        json: bool,
        /// JSON compacto (una línea); por defecto, con sangría.
        #[arg(long)]
        compact: bool,
        /// Más detalle: una entrada por componente, net o señal cambiada.
        #[arg(long, conflicts_with = "full")]
        detail: bool,
        /// Imprime el reporte completo del driver por archivo.
        #[arg(long)]
        full: bool,
        /// Filtra por glob (se puede repetir). Ej.: --paths 'amp_*.sch'.
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
    /// Verifica el entorno y lista los formatos que este `riku` sabe comparar.
    Doctor {
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        /// text o json (schema riku-doctor/v1): repo, PDK y módulos de formato.
        #[arg(short = 'f', long, value_enum, default_value_t = ListFormat::Text)]
        format: ListFormat,
    },
    /// Resume los cambios del working tree respecto a HEAD.
    #[command(after_help = STATUS_EXAMPLES)]
    Status {
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,
        /// Lista también los archivos que ningún módulo reconoce.
        #[arg(long)]
        include_unknown: bool,
        /// text o json (schema riku-status/v1).
        #[arg(short = 'f', long, value_enum, default_value_t = ListFormat::Text)]
        format: ListFormat,
        /// Igual que `-f json` (se mantiene por compatibilidad).
        #[arg(long, hide = true)]
        json: bool,
        /// JSON compacto (una línea); por defecto, con sangría.
        #[arg(long)]
        compact: bool,
        /// Más detalle: una entrada por componente, net o señal cambiada.
        #[arg(long, conflicts_with = "full")]
        detail: bool,
        /// Imprime el reporte completo del driver por archivo.
        #[arg(long)]
        full: bool,
        /// Filtra por glob (se puede repetir). Ej.: --paths 'amp_*.sch'.
        #[arg(long = "paths", value_name = "PAT")]
        paths: Vec<String>,
        /// Sin efecto: `status` siempre usa los códigos de CI (0 limpio,
        /// 1 cambios funcionales, 2 error). Se acepta por uniformidad.
        #[arg(long)]
        ci: bool,
    },
    /// Imprime el autocompletado para una shell: `riku completions bash > ~/.local/share/bash-completion/completions/riku`.
    Completions {
        /// bash, zsh, fish, powershell o elvish.
        shell: clap_complete::Shell,
    },
    /// Abre un archivo .sch, .gds, .oas, .mag o .raw en el visor de escritorio.
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

const MAIN_EXAMPLES: &str = "\
Ejemplos:
  riku status                      qué cambió en el disco respecto a HEAD
  riku diff                        el detalle de esos cambios
  riku diff HEAD~1 HEAD amp.sch    un archivo entre dos commits
  riku show HEAD -f json           el último commit, en JSON
  riku log -n 10 --graph           historial con el grafo de ramas
  riku doctor -f json              formatos soportados y entorno

Salida para scripts y agentes: -f json en todos los comandos (con `schema`);
códigos 0 sin cambios, 1 cambios funcionales, 2 error con --ci (status siempre).";

const DIFF_EXAMPLES: &str = "\
Ejemplos:
  riku diff                          working tree contra HEAD, todos los archivos
  riku diff amp.sch                  ese archivo, working tree contra HEAD
  riku diff main                     working tree contra main
  riku diff HEAD~1 HEAD              todo lo que cambió entre dos commits
  riku diff HEAD~1 HEAD top.gds -f json
  riku diff v1 v2 tb.raw --expr \"gain = v(out)/v(in)\"
  riku diff HEAD amp.sch -f visual   abrir el visor (A = HEAD, B = disco)

Un argumento es un ARCHIVO si Riku conoce su extensión o existe en el disco;
si no, es un commit (hash, rama, tag, HEAD~2).";

const SHOW_EXAMPLES: &str = "\
Ejemplos:
  riku show HEAD                   el último commit contra su padre
  riku show a3f2b1c amp.sch        solo ese archivo
  riku show HEAD -f json --ci      JSON y código 1 si hay cambios funcionales";

const LOG_EXAMPLES: &str = "\
Ejemplos:
  riku log                         los últimos 20 commits
  riku log amp.sch -n 5 --detail   un archivo, con cada componente cambiado
  riku log --graph                 con el grafo de ramas y merges
  riku log -f json --compact       JSON en una línea";

const STATUS_EXAMPLES: &str = "\
Ejemplos:
  riku status                      resumen por archivo
  riku status --detail             con cada componente, net o señal
  riku status -f json              JSON (schema riku-status/v1)

Código de salida: 0 sin cambios o solo cosméticos, 1 cambios funcionales, 2 error.";

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
