//! Formateador JSON para `riku diff`.
//!
//! - `-f json` (v2, schema `riku-diff/v2`): los cambios tipados del núcleo
//!   (`kind`, `element` con `type`, `details` con números reales).
//! - `-f json-v1`: la forma anterior (componentes con strings, `nets_added`,
//!   `is_move_all`), sin cambios, para scripts que todavía la leen.

use serde_json::json;

use crate::core::domain::models::FileChange;

pub const DIFF_SCHEMA: &str = "riku-diff/v2";

pub fn print(report: &FileChange, warnings: &[String], file_path: &str, from: &str, to: &str, pretty: bool) -> Result<(), String> {
    let payload = json!({
        "schema": DIFF_SCHEMA,
        "file": file_path,
        "from": from,
        "to": to,
        "format": report.format,
        "error": report.error,
        "warnings": warnings,
        "changes": report.changes,
    });
    super::print_enveloped(&payload, pretty)
}

pub fn print_v1(report: &FileChange, warnings: &[String], file_path: &str, pretty: bool) -> Result<(), String> {
    let mut payload = riku_kernel::legacy::diff_report_json(report);
    payload["file"] = json!(file_path);
    // v1 no tenía campo de error: va primero entre los avisos.
    payload["warnings"] = json!(report.error.iter().chain(warnings).collect::<Vec<_>>());
    super::print_enveloped(&payload, pretty)
}
