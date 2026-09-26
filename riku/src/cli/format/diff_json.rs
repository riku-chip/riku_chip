//! Formateador JSON para `riku diff`.
//!
//! - `-f json` (v2, schema `riku-diff/v2`): los cambios tipados del núcleo
//!   (`kind`, `element` con `type`, `details` con números reales).
//! - `-f json-v1`: la forma anterior (componentes con strings, `nets_added`,
//!   `is_move_all`), sin cambios, para scripts que todavía la leen.

use serde_json::json;

use crate::core::domain::models::FileChange;

pub const DIFF_SCHEMA: &str = "riku-diff/v2";

pub fn print(report: &FileChange, warnings: &[String], file_path: &str) -> Result<(), String> {
    let payload = json!({
        "schema": DIFF_SCHEMA,
        "file": file_path,
        "format": report.format,
        "warnings": warnings,
        "changes": report.changes,
    });
    super::print_enveloped(&payload, true)
}

pub fn print_v1(report: &FileChange, warnings: &[String], file_path: &str) -> Result<(), String> {
    let mut payload = riku_kernel::legacy::diff_report_json(report);
    payload["file"] = json!(file_path);
    payload["warnings"] = json!(warnings);
    super::print_enveloped(&payload, true)
}
