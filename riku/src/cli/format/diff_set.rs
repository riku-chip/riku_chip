//! Salida de `riku diff` sin archivo: todos los que cambiaron entre dos
//! versiones. Texto con el mismo formato por archivo que `riku show`; JSON
//! con el schema `riku-diff-set/v1` (los archivos, como en `riku-show/v1`).

use serde_json::json;

use super::color;
use super::show_json::file_json;
use super::show_text::print_files;
use crate::core::analysis::diff_set::DiffSetReport;
use crate::i18n::tr;

pub const DIFF_SET_SCHEMA: &str = "riku-diff-set/v1";

pub fn print_text(report: &DiffSetReport) -> Result<(), String> {
    let n = report.files.len();
    let head = format!("diff {} → {}", report.from.label(), report.to.label());
    let files = if n == 1 { tr!("diffset.file") } else { tr!("diffset.files", count = n) };
    println!("{}  ({files})", color::bold(&head));
    println!();
    if n == 0 {
        println!("{}", tr!("diffset.clean"));
        return Ok(());
    }
    print_files(&report.files, &tr!("diffset.untouched"))
}

pub fn print_json(report: &DiffSetReport, pretty: bool) -> Result<(), String> {
    let payload = json!({
        "schema": DIFF_SET_SCHEMA,
        "from": report.from.label(),
        "to": report.to.label(),
        "files": report.files.iter().map(file_json).collect::<Vec<_>>(),
    });
    super::print_enveloped(&payload, pretty)
}
