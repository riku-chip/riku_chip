//! Salida de `riku diff` sin archivo: todos los que cambiaron entre dos
//! versiones. Texto con el mismo formato por archivo que `riku show`; JSON
//! con el schema `riku-diff-set/v1` (los archivos, como en `riku-show/v1`).

use serde_json::json;

use super::color;
use super::show_json::file_json;
use super::show_text::print_files;
use crate::core::analysis::diff_set::DiffSetReport;

pub const DIFF_SET_SCHEMA: &str = "riku-diff-set/v1";

pub fn print_text(report: &DiffSetReport) -> Result<(), String> {
    let n = report.files.len();
    let head = format!("diff {} → {}", report.from.label(), report.to.label());
    println!("{}  ({n} archivo{})", color::bold(&head), if n == 1 { "" } else { "s" });
    println!();
    if n == 0 {
        println!("Sin cambios.");
        return Ok(());
    }
    print_files(&report.files, "sin cambios")
}

pub fn print_json(report: &DiffSetReport) -> Result<(), String> {
    let payload = json!({
        "schema": DIFF_SET_SCHEMA,
        "from": report.from.label(),
        "to": report.to.label(),
        "files": report.files.iter().map(file_json).collect::<Vec<_>>(),
    });
    super::print_enveloped(&payload, true)
}
