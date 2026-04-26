//! Formateador JSON para `riku log`.
//!
//! Usa `EnvelopedLogReport` para garantizar el campo `schema` (riku-log/v1).

use crate::core::analysis::log::{EnvelopedLogReport, LogReport};

pub fn print(report: &LogReport, pretty: bool) -> Result<(), String> {
    super::print_enveloped(&EnvelopedLogReport::from(report), pretty)
}
