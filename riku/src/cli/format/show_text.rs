//! Formateador de texto para `riku show`: cabecera del commit (como
//! `git show`) y el diff de texto de cada archivo con módulo.

use super::diff_text;
use crate::i18n::tr;
use super::log_text::format_timestamp;
use crate::core::analysis::show::{ShowFile, ShowReport};
use crate::core::domain::git_types::ChangeStatus;

pub fn print(report: &ShowReport) -> Result<(), String> {
    let info = &report.commit.info;
    let parent = match report.commit.parents.as_slice() {
        [] => tr!("show.initial"),
        [p] => tr!("show.parent", parent = short(p)),
        [p, ..] => tr!("show.merge", parent = short(p)),
    };
    println!("{}  ({parent})", super::color::yellow(&format!("commit {}", info.short_id)));
    println!("{}", tr!("show.author", author = info.author));
    println!("{}", tr!("show.date", date = format_timestamp(info.timestamp)));
    println!();
    for line in info.message.lines() {
        println!("    {line}");
    }
    println!();

    if report.files.is_empty() {
        println!("{}", tr!("show.no_files"));
        return Ok(());
    }
    print_files(&report.files, &tr!("show.untouched"))
}

/// El diff de texto de cada archivo con módulo y, al final, los que ningún
/// módulo reconoce. `untouched` explica un archivo pedido que no cambió.
pub fn print_files(files: &[ShowFile], untouched: &str) -> Result<(), String> {
    let mut unknown = Vec::new();
    for f in files {
        let Some(change) = &f.change else {
            unknown.push(f.path.as_str());
            continue;
        };
        for w in &change.warnings {
            eprintln!("[!] {w}");
        }
        if let Some(old) = &f.old_path {
            println!("{}", tr!("show.renamed", old = old, new = f.path));
        }
        if let Some(err) = &change.error {
            diff_text::print_error(&f.path, err);
        } else if change.is_empty() {
            let why = match f.status {
                None => untouched.to_string(),
                Some(ChangeStatus::Removed) => tr!("show.removed"),
                _ => tr!("show.no_semantic"),
            };
            println!("{}\n  {why}", tr!("diff.file", file = f.path));
        } else {
            diff_text::print(change, &f.path)?;
        }
        println!();
    }
    if !unknown.is_empty() {
        println!("{}", tr!("show.no_module", count = unknown.len(), list = unknown.join(", ")));
    }
    Ok(())
}

fn short(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}
