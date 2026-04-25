//! Formateador de texto para `riku log`.
//!
//! Composición visual:
//! ```text
//! * 4f2a1c  feature-amp ← HEAD   ajustar bias del OTA
//!           carlos · 2026-04-25 14:32
//!           amp_ota.sch  +2 transistores
//! ```
//!
//! Los detalles por archivo se incluyen en niveles `Detalle` y `Completo`,
//! reusando los mismos formateadores que `status` para consistencia.

use crate::core::analysis::log::{LogCommit, LogReport};
use crate::core::analysis::summary::{label_for, DetailEntry, DetailLevel, FileSummary};

pub fn print(report: &LogReport, level: DetailLevel) {
    for w in &report.warnings {
        eprintln!("[!] {w}");
    }

    if report.commits.is_empty() {
        println!("Sin commits encontrados.");
        return;
    }

    for (idx, c) in report.commits.iter().enumerate() {
        if idx > 0 {
            println!();
        }
        print_commit(c, level);
    }
}

fn print_commit(c: &LogCommit, level: DetailLevel) {
    let refs = if c.refs.is_empty() {
        String::new()
    } else {
        format!(" {}", format_refs(&c.refs))
    };
    let merge_tag = if c.is_merge { " [merge]" } else { "" };

    println!("* {}{}{}  {}", c.info.short_id, refs, merge_tag, first_line(&c.info.message));
    println!(
        "          {} · {}",
        c.info.author,
        format_timestamp(c.info.timestamp)
    );

    if c.is_merge {
        println!("          (merge commit; v1 no calcula diff por archivo)");
        return;
    }
    if c.parents.is_empty() {
        println!("          (commit raíz)");
        return;
    }
    if c.files.is_empty() {
        println!("          (sin cambios reconocidos por Riku)");
        return;
    }

    for f in &c.files {
        print_file_line(f, level);
    }
}

fn first_line(msg: &str) -> String {
    msg.lines().next().unwrap_or("").to_string()
}

fn format_refs(refs: &[String]) -> String {
    // HEAD primero si está; el resto en orden alfabético estable.
    let mut sorted = refs.to_vec();
    sorted.sort_by(|a, b| match (a == "HEAD", b == "HEAD") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    });
    format!("({})", sorted.join(", "))
}

/// Timestamp UNIX → string legible. No depende de chrono para no añadir dep:
/// formato `YYYY-MM-DD HH:MM` en UTC.
fn format_timestamp(ts: i64) -> String {
    if ts <= 0 {
        return "desconocido".to_string();
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

fn print_file_line(f: &FileSummary, level: DetailLevel) {
    println!("          {}  {}", f.path, format_counts(f));
    if matches!(level, DetailLevel::Detalle | DetailLevel::Completo) {
        for d in &f.details {
            print_detail(d);
        }
    }
}

fn format_counts(f: &FileSummary) -> String {
    use crate::core::analysis::summary::SummaryCategory;
    if matches!(f.category, SummaryCategory::Cosmetic) {
        return "(solo cambios cosméticos)".to_string();
    }
    if f.counts.is_empty() {
        return "(cambios sin detalle)".to_string();
    }
    let mut parts = Vec::with_capacity(f.counts.len());
    for (key, count) in &f.counts {
        let label = label_for(key, *count).unwrap_or_else(|| key.clone());
        parts.push(format!("{count} {label}"));
    }
    parts.join(", ")
}

fn print_detail(d: &DetailEntry) {
    use crate::core::analysis::summary::DetailKind::*;
    let marker = match d.kind {
        ComponentAdded | NetAdded => "+",
        ComponentRemoved | NetRemoved => "-",
        ComponentRenamed => "r",
        ComponentModified | NetModified | Other => "~",
    };
    println!("              {marker} {}", d.element);
    for (k, v) in &d.params {
        println!("                  {k}: {v}");
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_pone_head_primero() {
        let refs = vec!["main".to_string(), "HEAD".to_string(), "v1.0".to_string()];
        let s = format_refs(&refs);
        assert!(s.starts_with("(HEAD"));
    }

    #[test]
    fn timestamp_conocido_se_formatea_a_utc() {
        // 2026-04-25 14:30:00 UTC = 1777127400
        let s = format_timestamp(1777127400);
        assert_eq!(s, "2026-04-25 14:30");
    }

    #[test]
    fn timestamp_negativo_o_cero_se_marca_desconocido() {
        assert_eq!(format_timestamp(0), "desconocido");
        assert_eq!(format_timestamp(-1), "desconocido");
    }
}
