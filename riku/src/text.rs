//! Formatos de texto que comparten la CLI, el visor y las imágenes:
//! números con prefijo de ingeniería y fechas.

/// Número con prefijo de ingeniería: `0.0123 V` → `12.3 mV`. Los dB van tal cual.
pub fn eng(v: f64, unit: &str) -> String {
    if unit == "dB" || unit == "°" || v == 0.0 || !v.is_finite() {
        return format!("{v:.3} {unit}").trim_end().to_string();
    }
    // Sin unidad (una ganancia V/V, una razón): el número tal cual se lee
    // mejor que con prefijo ("0.06698" y no "66.98 m").
    if unit.is_empty() && (1e-3..1e6).contains(&v.abs()) {
        let digits = (3 - v.abs().log10().floor() as i32).clamp(0, 6) as usize;
        return format!("{v:.digits$}");
    }
    const PREFIXES: [(f64, &str); 9] =
        [(1e9, "G"), (1e6, "M"), (1e3, "k"), (1.0, ""), (1e-3, "m"), (1e-6, "µ"), (1e-9, "n"), (1e-12, "p"), (1e-15, "f")];
    let (scale, p) = PREFIXES.iter().find(|(s, _)| v.abs() >= *s).copied().unwrap_or((1e-15, "f"));
    format!("{:.3} {p}{unit}", v / scale).trim_end().to_string()
}

/// Timestamp UNIX → string legible. No depende de chrono para no añadir dep:
/// formato `YYYY-MM-DD HH:MM` en UTC.
pub fn format_timestamp(ts: i64) -> String {
    if ts <= 0 {
        return crate::i18n::tr!("log.unknown_date");
    }
    // Conversión manual sin chrono. UNIX → UTC.
    let secs = ts as u64;
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let (y, mo, d) = days_since_epoch_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02} {hour:02}:{min:02}")
}

/// Días desde 1970-01-01 (UTC) → (year, month, day). Algoritmo de Howard Hinnant.
fn days_since_epoch_to_ymd(days: u64) -> (i32, u32, u32) {
    // Trasladar al inicio del ciclo de 400 años en 0000-03-01.
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // día dentro del era [0..146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as u32, d as u32)
}
