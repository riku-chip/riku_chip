//! Formateador JSON para `riku log`.
//!
//! Usa `EnvelopedLogReport` para garantizar el campo `schema` (riku-log/v1).

use crate::core::analysis::log::{EnvelopedLogReport, LogReport};

pub fn print(report: &LogReport, pretty: bool) -> Result<(), String> {
    let env = EnvelopedLogReport::from(report);
    let s = if pretty {
        serde_json::to_string_pretty(&env)
    } else {
        serde_json::to_string(&env)
    }
    .map_err(|e| format!("error serializando JSON: {e}"))?;
    println!("{s}");
    Ok(())
}
