//! Formateadores de salida para los comandos.
//!
//! La lógica de dominio nunca conoce el formato. Los comandos eligen el
//! formateador según los flags del CLI.

pub mod diff_json;
pub mod diff_text;
pub mod log_json;
pub mod log_text;
pub mod status_json;
pub mod status_text;

/// Serializa un envelope JSON e imprime en stdout. `pretty=true` indenta.
pub(super) fn print_enveloped<T: serde::Serialize>(
    value: &T,
    pretty: bool,
) -> Result<(), String> {
    let s = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .map_err(|e| format!("error serializando JSON: {e}"))?;
    println!("{s}");
    Ok(())
}
