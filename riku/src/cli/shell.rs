//! Shell interactivo (REPL) de Riku.
//!
//! Es una capa delgada sobre `commands`: reusa el parser clap del módulo CLI
//! para que un `diff ...` dentro del shell se comporte idéntico a un
//! `riku diff ...` en la terminal. El shell solo agrega navegación (`cd`,
//! `ls`), resolución de rutas relativas y un prompt con contexto (cwd + repo).

use std::path::PathBuf;

use clap::Parser;

use super::{Cli, Commands};

const LOGO: &str = r#"
    ██████╗ ██╗██╗  ██╗██╗   ██╗
    ██╔══██╗██║██║ ██╔╝██║   ██║
    ██████╔╝██║█████╔╝ ██║   ██║
    ██╔══██╗██║██╔═██╗ ██║   ██║
    ██║  ██║██║██║  ██╗╚██████╔╝
    ╚═╝  ╚═╝╚═╝╚═╝  ╚═╝ ╚═════╝
"#;

struct ShellContext {
    cwd: PathBuf,
    repo: Option<git2::Repository>,
}

impl ShellContext {
    fn new(start: PathBuf) -> Self {
        let repo = git2::Repository::discover(&start).ok();
        Self { cwd: start, repo }
    }

    fn cd(&mut self, target: &str) {
        let next = if std::path::Path::new(target).is_absolute() {
            PathBuf::from(target)
        } else {
            self.cwd.join(target)
        };
        match next.canonicalize() {
            Ok(p) if p.is_dir() => {
                self.cwd = p.clone();
                self.repo = git2::Repository::discover(&p).ok();
                println!("  → {}", self.cwd.display());
            }
            _ => println!("  [!] No existe o no es una carpeta: {}", next.display()),
        }
    }

    fn ls(&self, target: Option<&str>) {
        let dir = match target {
            Some(t) => {
                let p = if std::path::Path::new(t).is_absolute() {
                    PathBuf::from(t)
                } else {
                    self.cwd.join(t)
                };
                match p.canonicalize() {
                    Ok(p) => p,
                    Err(_) => {
                        println!("  [!] No existe: {t}");
                        return;
                    }
                }
            }
            None => self.cwd.clone(),
        };

        let mut entries: Vec<_> = match std::fs::read_dir(&dir) {
            Ok(e) => e.filter_map(|e| e.ok()).collect(),
            Err(_) => {
                println!("  [!] No se puede leer: {}", dir.display());
                return;
            }
        };
        entries.sort_by_key(|e| e.file_name());

        println!();
        let mut found = false;
        for entry in &entries {
            if entry.path().is_dir() {
                println!("  {:>2}  {}/", "", entry.file_name().to_string_lossy());
                found = true;
            }
        }
        for entry in &entries {
            let path = entry.path();
            if path.extension().map(|e| e == "sch").unwrap_or(false) {
                let in_git = self
                    .repo
                    .as_ref()
                    .map(|r| {
                        r.workdir()
                            .and_then(|wd| path.strip_prefix(wd).ok())
                            .is_some()
                    })
                    .unwrap_or(false);
                let tag = if in_git { "[git]" } else { "     " };
                println!("  {tag}  {}", entry.file_name().to_string_lossy());
                found = true;
            }
        }
        if !found {
            println!("  (sin archivos .sch ni subdirectorios)");
        }
        println!();
    }

    fn prompt(&self) -> String {
        let dir = self
            .cwd
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.cwd.display().to_string());
        let repo_mark = if self.repo.is_some() { " (git)" } else { "" };
        format!("riku {dir}{repo_mark}> ")
    }

    fn repo_path(&self) -> PathBuf {
        self.repo
            .as_ref()
            .and_then(|r| r.workdir())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.cwd.clone())
    }

    /// Si el usuario pasó `--repo .` (o no lo pasó), usa el repo del shell;
    /// si pasó una ruta explícita, respétala.
    fn resolve_repo(&self, requested: PathBuf) -> PathBuf {
        if requested == PathBuf::from(".") {
            self.repo_path()
        } else {
            requested
        }
    }

    fn resolve_file(&self, f: &str) -> String {
        let abs = if std::path::Path::new(f).is_absolute() {
            PathBuf::from(f)
        } else {
            self.cwd.join(f)
        };
        let repo = self.repo_path();
        abs.strip_prefix(&repo)
            .map(|rel| rel.to_string_lossy().to_string())
            .unwrap_or_else(|_| abs.to_string_lossy().to_string())
    }
}

fn shell_status_line(ctx: &ShellContext) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let pdk = match (std::env::var("PDK_ROOT").ok(), std::env::var("PDK").ok()) {
        (Some(root), Some(pdk)) => {
            let sym = std::path::Path::new(&root)
                .join(&pdk)
                .join("libs.tech/xschem");
            if sym.exists() {
                format!("PDK: {pdk} [ok]")
            } else {
                "PDK: no detectado".to_string()
            }
        }
        _ => "PDK: no detectado".to_string(),
    };
    let repo_str = git2::Repository::discover(&ctx.cwd)
        .ok()
        .and_then(|r| r.workdir().map(|p| p.display().to_string()))
        .unwrap_or_else(|| "repo: no encontrado".to_string());
    format!("  v{version}  ·  {pdk}  ·  {repo_str}")
}

pub(super) fn run_shell() -> Result<(), String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut ctx = ShellContext::new(cwd);

    print!("{LOGO}");
    println!("{}", shell_status_line(&ctx));
    if ctx.repo.is_none() {
        println!("  [!] No se detectó repositorio Git. Usa 'cd <ruta>' para navegar a uno.");
    }
    println!("  'help' para ver los comandos. 'exit' para salir.\n");

    let mut rl = rustyline::DefaultEditor::new().map_err(|e| e.to_string())?;

    loop {
        let prompt = ctx.prompt();
        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(
                rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
            ) => break,
            Err(e) => return Err(e.to_string()),
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let _ = rl.add_history_entry(&line);

        let mut parts = line.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();

        match cmd {
            "exit" | "quit" | "q" => break,
            "help" => print_shell_help(),
            "cd" => ctx.cd(if rest.is_empty() { "." } else { rest }),
            "ls" => ctx.ls(if rest.is_empty() { None } else { Some(rest) }),
            _ => dispatch_shell_command(&mut ctx, &line),
        }
    }

    println!("\n  Hasta luego.\n");
    Ok(())
}

fn print_shell_help() {
    println!();
    println!("  Navegación:");
    println!("    ls [ruta]                                     listar archivos .sch");
    println!("    cd <ruta>                                     cambiar directorio");
    println!();
    println!("  Git:");
    println!("    status [--detail|--full] [--json [--compact]] [--paths PAT]");
    println!(
        "                                                    cambios semánticos en working tree"
    );
    println!(
        "    log [archivo.sch] [--detail|--full] [--json [--compact]] [--paths PAT] [--branch REF]"
    );
    println!(
        "                                                    historial con resumen semántico por commit"
    );
    println!("    diff <commit_a> <commit_b> <archivo.sch>      diff semántico");
    println!("    diff ... --format visual                      diff visual en HTML");
    println!();
    println!("  Visor:");
    println!("    open [archivo.sch]                            abrir visor de escritorio");
    println!();
    println!("  Entorno:");
    println!("    doctor                                        verificar PDK y repo");
    println!("    exit                                          salir");
    println!();
}

fn dispatch_shell_command(ctx: &mut ShellContext, line: &str) {
    let mut args = vec!["riku"];
    args.extend(line.split_whitespace());

    match Cli::try_parse_from(&args) {
        Ok(parsed) => {
            let Some(mut cmd) = parsed.command else {
                println!("  Ya estás en el shell.");
                return;
            };
            cmd.resolve_paths(ctx);
            // `Outcome::Status*` se descarta a propósito — el shell no usa
            // exit codes; cambios pendientes se reflejan en la salida del
            // propio comando.
            if let Err(e) = cmd.execute() {
                eprintln!("  Error: {e}");
            }
        }
        Err(e) => {
            println!(
                "  {}",
                e.to_string()
                    .lines()
                    .next()
                    .unwrap_or("comando no reconocido")
            );
        }
    }
}

// ─── Shell-specific path resolution ─────────────────────────────────────────

impl Commands {
    /// Aplica las resoluciones del shell antes de ejecutar: `--repo .` se
    /// reemplaza por el repo activo del REPL, y los path relativos se rebasan
    /// al cwd del shell. No toca flags ni semántica del comando.
    fn resolve_paths(&mut self, ctx: &ShellContext) {
        match self {
            Commands::Diff {
                repo, file_path, ..
            } => {
                *repo = ctx.resolve_repo(std::mem::take(repo));
                *file_path = ctx.resolve_file(file_path);
            }
            Commands::Log {
                repo, file_path, ..
            } => {
                *repo = ctx.resolve_repo(std::mem::take(repo));
                if let Some(f) = file_path.as_mut() {
                    *f = ctx.resolve_file(f);
                }
            }
            Commands::Doctor { repo } => {
                *repo = ctx.resolve_repo(std::mem::take(repo));
            }
            Commands::Status { repo, .. } => {
                *repo = ctx.resolve_repo(std::mem::take(repo));
            }
            Commands::Open { file } => {
                if let Some(f) = file.as_mut() {
                    if f.components().count() == 1 {
                        *f = ctx.cwd.join(&*f);
                    }
                }
            }
        }
    }
}
