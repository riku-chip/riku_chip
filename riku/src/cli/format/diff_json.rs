//! Formateador JSON para `riku diff`.
//!
//! Salida pretty-printed con el reporte semántico crudo. No envuelve en un
//! schema versionado (a diferencia de log/status) porque el diff aún no tiene
//! contrato público estable.

use serde_json::json;

use crate::core::analysis::diff_view::DiffView;

pub fn print(view: &DiffView, file_path: &str) -> Result<(), String> {
    let payload = json!({
        "file": file_path,
        "warnings": view.warnings,
        "components": view.report.components,
        "nets_added": view.report.nets_added,
        "nets_removed": view.report.nets_removed,
        "is_move_all": view.report.is_move_all,
    });
    super::print_enveloped(&payload, true)
}
