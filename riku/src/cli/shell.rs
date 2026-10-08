//! Shell interactivo (REPL) de Riku.
//!
//! Es una capa delgada sobre `commands`: reusa el parser clap del módulo CLI
//! para que un `diff ...` dentro del shell se comporte idéntico a un
//! `riku diff ...` en la terminal. El shell solo agrega navegación (`cd`,
//! `ls`), resolución de rutas relativas, un prompt con contexto (cwd + repo)
//! y autocompletado con Tab (`shell_complete`).

use crate::i18n::tr;
use std::path::PathBuf;

use clap::Parser;

use crate::modules::xschem_pdk::{pdk_status, PdkStatus};

use super::shell_complete::RikuHelper;
use super::{Cli, Commands};

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
        let next = if std::path::Path::new(target).is_absolute() { PathBuf::from(target) } else { self.cwd.join(target) };
        match next.canonicalize() {
            Ok(p) if p.is_dir() => {
                self.cwd = p.clone();
                self.repo = git2::Repository::discover(&p).ok();
                println!("  → {}", self.cwd.display());
            }
            _ => println!("  [!] {}", tr!("shell.not_dir", path = next.display())),
        }
    }

    fn ls(&self, target: Option<&str>) {
        let dir = match target {
            Some(t) => {
                let p = if std::path::Path::new(t).is_absolute() { PathBuf::from(t) } else { self.cwd.join(t) };
                match p.canonicalize() {
                    Ok(p) => p,
                    Err(_) => {
                        println!("  [!] {}", tr!("shell.not_found", path = t));
                        return;
                    }
                }
            }
            None => self.cwd.clone(),
        };

        let mut entries: Vec<_> = match std::fs::read_dir(&dir) {
            Ok(e) => e.filter_map(|e| e.ok()).collect(),
            Err(_) => {
                println!("  [!] {}", tr!("shell.unreadable", path = dir.display()));
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
        let known = crate::modules::registry().openable();
        for entry in &entries {
            let path = entry.path();
            let openable =
                path.extension().and_then(|e| e.to_str()).is_some_and(|e| known.iter().any(|o| e.eq_ignore_ascii_case(o)));
            if openable {
                let in_git =
                    self.repo.as_ref().map(|r| r.workdir().and_then(|wd| path.strip_prefix(wd).ok()).is_some()).unwrap_or(false);
                let tag = if in_git { "[git]" } else { "     " };
                println!("  {tag}  {}", entry.file_name().to_string_lossy());
                found = true;
            }
        }
        if !found {
            println!("  {}", tr!("shell.empty_dir"));
        }
        println!();
    }

    fn prompt(&self) -> String {
        let dir = self.cwd.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| self.cwd.display().to_string());
        let repo_mark = if self.repo.is_some() { " (git)" } else { "" };
        format!("riku {dir}{repo_mark}> ")
    }

    fn repo_path(&self) -> PathBuf {
        self.repo.as_ref().and_then(|r| r.workdir()).map(|p| p.to_path_buf()).unwrap_or_else(|| self.cwd.clone())
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

    /// Un archivo escrito desde el cwd del shell, como ruta del repo.
    fn resolve_file(&self, f: &str) -> String {
        crate::core::repo_path::to_repo_path(&self.cwd, &self.repo_path(), f)
    }
}

fn shell_status_line(ctx: &ShellContext) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let pdk = match pdk_status() {
        PdkStatus::Found(_) => format!("PDK: {} [ok]", std::env::var("PDK").unwrap_or_default()),
        PdkStatus::Misconfigured(_) | PdkStatus::NotConfigured => {
            tr!("shell.pdk_none")
        }
    };
    let repo_str = git2::Repository::discover(&ctx.cwd)
        .ok()
        .and_then(|r| r.workdir().map(|p| p.display().to_string()))
        .unwrap_or_else(|| tr!("shell.repo_none"));
    format!("  v{version}  ·  {pdk}  ·  {repo_str}")
}

pub(super) fn run_shell() -> Result<(), String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut ctx = ShellContext::new(cwd);

    super::banner::print(super::banner::detect());
    println!("{}", shell_status_line(&ctx));
    if ctx.repo.is_none() {
        println!("  [!] {}", tr!("shell.no_repo"));
    }
    println!("  {}\n", tr!("shell.hint"));

    let mut rl: rustyline::Editor<RikuHelper, rustyline::history::DefaultHistory> =
        rustyline::Editor::new().map_err(|e| e.to_string())?;
    rl.set_helper(Some(RikuHelper { cwd: ctx.cwd.clone() }));

    loop {
        let prompt = ctx.prompt();
        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof) => break,
            Err(e) => return Err(e.to_string()),
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let _ = rl.add_history_entry(&line);

        // Como una shell: comillas para rutas con espacios y expresiones
        // (`--expr "gain = v(out)/v(in)"`).
        let Some(words) = split_line(&line) else {
            println!("  {}", tr!("shell.bad_quotes"));
            continue;
        };
        let Some(cmd) = words.first() else { continue };
        let arg = words.get(1).map(String::as_str);

        match cmd.as_str() {
            "exit" | "quit" | "q" => break,
            "help" => print_shell_help(),
            "cd" => {
                ctx.cd(arg.unwrap_or("."));
                if let Some(h) = rl.helper_mut() {
                    h.cwd = ctx.cwd.clone();
                }
            }
            "ls" => ctx.ls(arg),
            _ => dispatch_shell_command(&mut ctx, words),
        }
    }

    println!("\n  {}\n", tr!("shell.bye"));
    Ok(())
}

fn print_shell_help() {
    let row = |cmd: &str, key: &str| println!("    {cmd:<46}{}", tr!(key));
    println!();
    println!("  {}", tr!("shell.h_nav"));
    println!("    {:<46}{}", "ls [path]", tr!("shell.h_ls", exts = crate::modules::registry().openable_text()));
    row("cd <path>", "shell.h_cd");
    println!();
    println!("  {}", tr!("shell.h_git"));
    row("status [--detail] [-f json]", "shell.h_status");
    row("diff [A] [B] [file] [-f json]", "shell.h_diff");
    row("diff ... -f visual", "shell.h_visual");
    row("show <commit> [file]", "shell.h_show");
    row("log [file] [-n N] [--graph] [--detail]", "shell.h_log");
    println!();
    println!("  {}", tr!("shell.h_viewer"));
    row("open [file]", "shell.h_open");
    println!();
    println!("  {}", tr!("shell.h_env"));
    row("doctor [-f json]", "shell.h_doctor");
    row("exit", "shell.h_exit");
    println!();
    println!("  {}", tr!("shell.h_more"));
    println!();
}

/// Palabras de una línea con las reglas de una shell POSIX (comillas simples
/// y dobles, `\` para escapar). `None` si una comilla quedó abierta.
fn split_line(line: &str) -> Option<Vec<String>> {
    shlex::split(line)
}

fn dispatch_shell_command(ctx: &mut ShellContext, words: Vec<String>) {
    let args = std::iter::once("riku".to_string()).chain(words);

    match Cli::try_parse_from(args) {
        Ok(parsed) => {
            let Some(mut cmd) = parsed.command else {
                println!("  {}", tr!("shell.already"));
                return;
            };
            resolve_for_shell(&mut cmd, ctx);
            // `Outcome::Clean/Functional` se descarta a propósito — el shell no usa
            // exit codes; cambios pendientes se reflejan en la salida del
            // propio comando.
            if let Err(e) = cmd.execute() {
                eprintln!("  {}", tr!("shell.error", error = e));
            }
        }
        Err(e) => {
            println!("  {}", e.to_string().lines().next().unwrap_or(&tr!("shell.unknown_cmd")));
        }
    }
}

#[cfg(test)]
mod split_tests {
    use super::split_line;

    #[test]
    fn comillas_como_una_shell() {
        let w = split_line(r#"diff v1 v2 tb.raw --expr "gain = v(out)/v(in)""#).unwrap();
        assert_eq!(w, ["diff", "v1", "v2", "tb.raw", "--expr", "gain = v(out)/v(in)"]);
        assert_eq!(split_line("open 'mi diseño/amp.sch'").unwrap(), ["open", "mi diseño/amp.sch"]);
        assert_eq!(split_line(r"cd mi\ carpeta").unwrap(), ["cd", "mi carpeta"]);
        assert!(split_line(r#"diff "sin cerrar"#).is_none());
    }
}

// ─── Shell-specific path resolution ─────────────────────────────────────────

/// Aplica las resoluciones del shell antes de ejecutar: `--repo .` se
/// reemplaza por el repo activo del REPL, y los path relativos se rebasan
/// al cwd del shell. No toca flags ni semántica del comando.
///
/// Vive como función libre (y no como `impl Commands`) para no contaminar el
/// tipo del parser con conocimiento del REPL.
fn resolve_for_shell(cmd: &mut Commands, ctx: &ShellContext) {
    match cmd {
        Commands::Diff { repo, targets, .. } => {
            *repo = ctx.resolve_repo(std::mem::take(repo));
            // El último argumento, si es un archivo (extensión conocida o
            // existe desde el cwd del shell), pasa a ruta del repo.
            if let Some(last) = targets.last_mut() {
                let modules = crate::modules::registry();
                if modules.for_path(last).is_some() || ctx.cwd.join(&*last).is_file() {
                    *last = ctx.resolve_file(last);
                }
            }
        }
        Commands::Show { repo, file_path, .. } => {
            *repo = ctx.resolve_repo(std::mem::take(repo));
            if let Some(f) = file_path.as_mut() {
                *f = ctx.resolve_file(f);
            }
        }
        Commands::Log { repo, file_path, .. } => {
            *repo = ctx.resolve_repo(std::mem::take(repo));
            if let Some(f) = file_path.as_mut() {
                *f = ctx.resolve_file(f);
            }
        }
        Commands::Doctor { repo, .. } => {
            *repo = ctx.resolve_repo(std::mem::take(repo));
        }
        #[cfg(all(feature = "xschem", feature = "layout"))]
        Commands::Lvs { repo, sch, layout, .. } => {
            *repo = ctx.resolve_repo(std::mem::take(repo));
            for f in [sch, layout].into_iter().flatten() {
                *f = ctx.resolve_file(f);
            }
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
        Commands::Render { repo, file, .. } => {
            *repo = ctx.resolve_repo(std::mem::take(repo));
            *file = ctx.resolve_file(file);
        }
        Commands::Demo { dir, .. } => {
            if let Some(d) = dir.as_mut().filter(|d| d.is_relative()) {
                *d = ctx.cwd.join(&*d);
            }
        }
        Commands::Gui { .. } | Commands::Completions { .. } | Commands::About => {}
    }
}
