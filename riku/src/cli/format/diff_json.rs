//! Formateador JSON para `riku diff` (schema `riku-diff/v2`): los cambios
//! tipados del núcleo (`kind`, `element` con `type`, `details` con números
//! reales).

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
