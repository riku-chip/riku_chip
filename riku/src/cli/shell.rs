//! Shell interactivo (REPL) de Riku.
//!
//! Es una capa delgada sobre `commands`: reusa el parser clap del módulo CLI
//! para que un `diff ...` dentro del shell se comporte idéntico a un
//! `riku diff ...` en la terminal. El shell solo agrega navegación (`cd`,
//! `ls`), resolución de rutas relativas y un prompt con contexto (cwd + repo).

use std::path::PathBuf;

use clap::Parser;

use super::{Cli, Commands};
use super::commands;

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
                    Err(_) => { println!("  [!] No existe: {t}"); return; }
                }
            }
            None => self.cwd.clone(),
        };

        let mut entries: Vec<_> = match std::fs::read_dir(&dir) {
            Ok(e) => e.filter_map(|e| e.ok()).collect(),
            Err(_) => { println!("  [!] No se puede leer: {}", dir.display()); return; }
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
                let in_git = self.repo.as_ref().map(|r| {
                    r.workdir()
                        .and_then(|wd| path.strip_prefix(wd).ok())
                        .is_some()
                }).unwrap_or(false);
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
        let dir = self.cwd.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.cwd.display().to_string());
        let repo_mark = if self.repo.is_some() { " (git)" } else { "" };
        format!("riku {dir}{repo_mark}> ")
    }

    fn repo_path(&self) -> PathBuf {
        self.repo.as_ref()
            .and_then(|r| r.workdir())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.cwd.clone())
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
            let sym = std::path::Path::new(&root).join(&pdk).join("libs.tech/xschem");
            if sym.exists() { format!("PDK: {pdk} [ok]") } else { "PDK: no detectado".to_string() }
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
            Err(rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof) => break,
            Err(e) => return Err(e.to_string()),
        };

        let line = line.trim().to_string();
        if line.is_empty() { continue; }
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
    println!("    log [archivo.sch] [--semantic] [--limit <n>]  historial de commits");
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
            let repo_path = ctx.repo_path();
            let result = match parsed.command {
                None => {
                    println!("  Ya estás en el shell.");
                    Ok(())
                }
                Some(Commands::Diff { commit_a, commit_b, file_path, repo: r, format }) => {
                    let effective_repo = if r == PathBuf::from(".") { repo_path } else { r };
                    let effective_file = ctx.resolve_file(&file_path);
                    commands::run_diff(effective_repo, &commit_a, &commit_b, &effective_file, format)
                }
                Some(Commands::Log { file_path, repo: r, limit, semantic }) => {
                    let effective_repo = if r == PathBuf::from(".") { repo_path } else { r };
                    let effective_file = file_path.as_deref().map(|f| ctx.resolve_file(f));
                    commands::run_log(effective_repo, effective_file.as_deref(), limit, semantic)
                }
                Some(Commands::Doctor { repo: r }) => {
                    commands::run_doctor(if r == PathBuf::from(".") { repo_path } else { r })
                }
                Some(Commands::Open { file }) => {
                    let effective = file.map(|f| {
                        if f.components().count() == 1 { ctx.cwd.join(&f) } else { f }
                    });
                    commands::run_gui(effective)
                }
            };
            if let Err(e) = result {
                eprintln!("  Error: {e}");
            }
        }
        Err(e) => {
            println!("  {}", e.to_string().lines().next().unwrap_or("comando no reconocido"));
        }
    }
}
