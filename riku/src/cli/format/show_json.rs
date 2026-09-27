//! Formateador JSON para `riku show` (schema `riku-show/v1`): el commit y,
//! por archivo, los mismos cambios tipados que `riku diff -f json`.

use serde_json::json;

use crate::core::analysis::show::{ShowFile, ShowReport};

pub const SHOW_SCHEMA: &str = "riku-show/v1";

pub fn print(report: &ShowReport, pretty: bool) -> Result<(), String> {
    let info = &report.commit.info;
    let files: Vec<_> = report.files.iter().map(file_json).collect();
    let payload = json!({
        "schema": SHOW_SCHEMA,
        "commit": {
            "oid": info.oid,
            "short_id": info.short_id,
            "author": info.author,
            "timestamp": info.timestamp,
            "message": info.message,
            "parents": report.commit.parents,
        },
        "files": files,
    });
    super::print_enveloped(&payload, pretty)
}

/// Un archivo con sus cambios tipados (lo comparten `show` y `diff`).
pub fn file_json(f: &ShowFile) -> serde_json::Value {
    let (format, warnings, changes) = match &f.change {
        Some(c) => (json!(c.format), json!(c.warnings), json!(c.changes)),
        None => (json!(null), json!([]), json!([])),
    };
    json!({
        "file": f.path,
        "status": f.status,
        "old_path": f.old_path,
        "format": format,
        "warnings": warnings,
        "changes": changes,
    })
}
