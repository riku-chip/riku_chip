//! Formateador JSON para `riku status`.
//!
//! Emite el reporte envuelto en `EnvelopedStatusReport`, garantizando que el
//! campo `schema` esté siempre presente. Esto es contrato público — ver
//! [`crate::core::analysis::status::STATUS_SCHEMA`].

use crate::core::analysis::status::{EnvelopedStatusReport, StatusReport};

/// Imprime el reporte como JSON en stdout. `pretty=true` añade indentación.
pub fn print(report: &StatusReport, pretty: bool) -> Result<(), String> {
    super::print_enveloped(&EnvelopedStatusReport::from(report), pretty)
}
