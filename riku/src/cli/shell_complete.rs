//! Autocompletado del shell interactivo (Tab).
//!
//! `complete` es una función pura sobre (línea, cursor, cwd) para poder
//! testearla sin terminal. Qué completa según la posición:
//! - primera palabra: comandos del shell y subcomandos de la CLI;
//! - una palabra que empieza con `-`: flags del subcomando (sacados de clap);
//! - después de `cd`: carpetas;
//! - después de cualquier otro comando: ramas, tags y commits recientes del
//!   repo, carpetas y archivos que Riku sabe abrir.

use std::path::{Path, PathBuf};

use clap::CommandFactory;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

use super::Cli;

/// Comandos propios del shell (además de los subcomandos de la CLI).
const SHELL_COMMANDS: &[&str] = &["cd", "ls", "help", "exit"];
/// Commits recientes ofrecidos como candidatos.
const RECENT_COMMITS: usize = 20;

/// Candidatos para la palabra bajo el cursor. Retorna la posición donde
/// empieza esa palabra (lo que se reemplaza) y los candidatos ordenados.
pub(super) fn complete(line: &str, pos: usize, cwd: &Path) -> (usize, Vec<String>) {
    let head = &line[..pos];
    // El separador puede ocupar más de un byte (un espacio no separable).
    let start = head.char_indices().rfind(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8());
    let word = &head[start..];
    let before: Vec<&str> = head[..start].split_whitespace().collect();

    let mut out: Vec<String> = match before.first() {
        None => command_names().into_iter().filter(|c| c.starts_with(word)).collect(),
        Some(_) if word.starts_with('-') => flags(before[0]).into_iter().filter(|f| f.starts_with(word)).collect(),
        Some(&"cd") => paths(cwd, word, true),
        Some(_) => {
            let mut v = paths(cwd, word, false);
            if !word.contains('/') {
                v.extend(git_refs(cwd).into_iter().filter(|r| r.starts_with(word)));
            }
            v
        }
    };
    out.sort();
    out.dedup();
    (start, out)
}

fn command_names() -> Vec<String> {
    let mut v: Vec<String> = SHELL_COMMANDS.iter().map(|s| s.to_string()).collect();
    v.extend(Cli::command().get_subcommands().map(|c| c.get_name().to_string()));
    v
}

/// `--flag` y `-f` del subcomando `cmd`, según su definición en clap.
fn flags(cmd: &str) -> Vec<String> {
    let root = Cli::command();
    let Some(sub) = root.find_subcommand(cmd) else { return Vec::new() };
    let mut v = vec!["--help".to_string()];
    for a in sub.get_arguments() {
        if let Some(l) = a.get_long() {
            v.push(format!("--{l}"));
        }
        if let Some(s) = a.get_short() {
            v.push(format!("-{s}"));
        }
    }
    v
}

/// Entradas de la carpeta implicada por `word` (relativa a `cwd`), con `/`
/// final en las carpetas. Sin `dirs_only`, solo archivos abribles.
fn paths(cwd: &Path, word: &str, dirs_only: bool) -> Vec<String> {
    let (dir_part, name_part) = match word.rfind('/') {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word),
    };
    let dir: PathBuf = if Path::new(dir_part).is_absolute() { PathBuf::from(dir_part) } else { cwd.join(dir_part) };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    // Lo que saben abrir los módulos de este ejecutable.
    let known = crate::modules::registry().openable();
    entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with(name_part) || (name.starts_with('.') && !name_part.starts_with('.')) {
                return None;
            }
            let is_dir = e.path().is_dir();
            let openable =
                e.path().extension().and_then(|x| x.to_str()).is_some_and(|x| known.iter().any(|o| x.eq_ignore_ascii_case(o)));
            match (is_dir, dirs_only) {
                (true, _) => Some(format!("{dir_part}{name}/")),
                (false, false) if openable => Some(format!("{dir_part}{name}")),
                _ => None,
            }
        })
        .collect()
}

/// Ramas locales, tags, `HEAD` y los hashes cortos de los commits recientes.
fn git_refs(cwd: &Path) -> Vec<String> {
    let Ok(repo) = git2::Repository::discover(cwd) else { return Vec::new() };
    let mut v = vec!["HEAD".to_string()];
    if let Ok(branches) = repo.branches(Some(git2::BranchType::Local)) {
        v.extend(branches.filter_map(Result::ok).filter_map(|(b, _)| b.name().ok().flatten().map(str::to_string)));
    }
    if let Ok(tags) = repo.tag_names(None) {
        v.extend(tags.iter().filter_map(|t| t.ok().flatten()).map(str::to_string));
    }
    if let Ok(mut walk) = repo.revwalk() {
        if walk.push_head().is_ok() {
            v.extend(walk.filter_map(Result::ok).take(RECENT_COMMITS).map(|oid| oid.to_string()[..7].to_string()));
        }
    }
    v
}

/// Helper de rustyline: solo completa; `cwd` sigue al `cd` del shell.
pub(super) struct RikuHelper {
    pub(super) cwd: PathBuf,
}

impl Completer for RikuHelper {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        let (start, words) = complete(line, pos, &self.cwd);
        // Al completar una palabra entera se agrega un espacio; en carpetas no,
        // para seguir escribiendo la ruta.
        let pairs = words
            .into_iter()
            .map(|w| {
                let replacement = if w.ends_with('/') { w.clone() } else { format!("{w} ") };
                Pair { display: w, replacement }
            })
            .collect();
        Ok((start, pairs))
    }
}

impl Hinter for RikuHelper {
    type Hint = String;
}
impl Highlighter for RikuHelper {}
impl Validator for RikuHelper {}
impl Helper for RikuHelper {}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str, cwd: &Path) -> Vec<String> {
        complete(line, line.len(), cwd).1
    }

    #[test]
    fn first_word_completes_commands() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(words("di", tmp.path()), vec!["diff"]);
        let all = words("", tmp.path());
        for c in ["cd", "ls", "help", "exit", "diff", "log", "status", "doctor", "open"] {
            assert!(all.contains(&c.to_string()), "{c} en {all:?}");
        }
    }

    #[test]
    fn a_non_ascii_space_separates_words() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(words("status\u{a0}--fo", tmp.path()), vec!["--format"]);
        assert_eq!(complete("log\u{a0}", 5, tmp.path()).0, 5);
    }

    #[test]
    fn dash_completes_flags_of_the_subcommand() {
        let tmp = tempfile::tempdir().unwrap();
        let f = words("diff HEAD~1 HEAD x.gds --fo", tmp.path());
        assert_eq!(f, vec!["--format"]);
        assert!(words("diff a b c --", tmp.path()).contains(&"--cosmetic-threshold-um2".to_string()));
    }

    #[test]
    fn cd_offers_only_directories_and_other_commands_openable_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("layouts")).unwrap();
        std::fs::create_dir(tmp.path().join(".oculta")).unwrap();
        for f in ["chip.gds", "chip.oas", "amp.sch", "notas.txt"] {
            std::fs::write(tmp.path().join(f), b"x").unwrap();
        }
        std::fs::write(tmp.path().join("layouts/top.gds"), b"x").unwrap();
        assert_eq!(words("cd ", tmp.path()), vec!["layouts/"]);
        assert_eq!(words("open ", tmp.path()), vec!["amp.sch", "chip.gds", "chip.oas", "layouts/"]);
        assert_eq!(words("open layouts/t", tmp.path()), vec!["layouts/top.gds"]);
        // La posición de reemplazo es el inicio de la palabra, no de la línea.
        assert_eq!(complete("open ch", 7, tmp.path()).0, 5);
    }

    #[test]
    fn refs_and_recent_commits_come_from_the_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(tmp.path()).unwrap();
        let sig = git2::Signature::now("Riku", "riku@example.com").unwrap();
        let tree = repo.find_tree(repo.index().unwrap().write_tree().unwrap()).unwrap();
        let oid = repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[]).unwrap();
        let commit = repo.find_commit(oid).unwrap();
        repo.branch("feature-amp", &commit, false).unwrap();
        repo.tag_lightweight("v1.0", commit.as_object(), false).unwrap();

        let w = words("diff ", tmp.path());
        for r in ["HEAD", "feature-amp", "v1.0", &oid.to_string()[..7]] {
            assert!(w.contains(&r.to_string()), "{r} en {w:?}");
        }
        assert_eq!(words("diff feat", tmp.path()), vec!["feature-amp"]);
    }
}
