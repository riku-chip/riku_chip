//! Formateador de texto para `riku show`: cabecera del commit (como
//! `git show`) y el diff de texto de cada archivo con módulo.

use super::diff_text;
use super::log_text::format_timestamp;
use crate::core::analysis::show::ShowReport;
use crate::core::domain::git_types::ChangeStatus;

pub fn print(report: &ShowReport) -> Result<(), String> {
    let info = &report.commit.info;
    let parent = match report.commit.parents.as_slice() {
        [] => "commit inicial: se compara contra vacío".to_string(),
        [p] => format!("padre {}", short(p)),
        [p, ..] => format!("merge: se compara contra el primer padre {}", short(p)),
    };
    println!("commit {}  ({parent})", info.short_id);
    println!("Autor : {}", info.author);
    println!("Fecha : {}", format_timestamp(info.timestamp));
    println!();
    for line in info.message.lines() {
        println!("    {line}");
    }
    println!();

    if report.files.is_empty() {
        println!("El commit no cambió archivos.");
        return Ok(());
    }

    let mut unknown = Vec::new();
    for f in &report.files {
        let Some(change) = &f.change else {
            unknown.push(f.path.as_str());
            continue;
        };
        for w in &change.warnings {
            eprintln!("[!] {w}");
        }
        if let Some(old) = &f.old_path {
            println!("Renombrado: {old} → {}", f.path);
        }
        if change.is_empty() {
            let why = match f.status {
                None => "no cambió en este commit",
                Some(ChangeStatus::Removed) => "eliminado",
                _ => "sin cambios semánticos",
            };
            println!("Archivo : {}\n  {why}", f.path);
        } else {
            diff_text::print(change, &f.path)?;
        }
        println!();
    }
    if !unknown.is_empty() {
        println!("Sin módulo de Riku ({}): {}", unknown.len(), unknown.join(", "));
    }
    Ok(())
}

fn short(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}
