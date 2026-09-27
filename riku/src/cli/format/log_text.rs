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

use super::common::{format_counts, print_detail, warning_lines};
pub(crate) use crate::text::format_timestamp;
use crate::core::analysis::log::{LogCommit, LogReport};
use crate::core::analysis::summary::{DetailLevel, FileSummary};

pub fn print(report: &LogReport, level: DetailLevel) {
    for w in &report.warnings {
        eprintln!("[!] {w}");
    }

    if report.commits.is_empty() {
        println!("{}", crate::i18n::tr!("log.no_commits"));
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

    println!(
        "* {}{}{}  {}",
        c.info.short_id,
        refs,
        merge_tag,
        first_line(&c.info.message)
    );
    println!(
        "          {} · {}",
        c.info.author,
        format_timestamp(c.info.timestamp)
    );

    if c.is_merge {
        println!("          {}", crate::i18n::tr!("log.merge_no_diff"));
        return;
    }
    if c.parents.is_empty() {
        println!("          {}", crate::i18n::tr!("log.root"));
        return;
    }
    if c.files.is_empty() {
        println!("          {}", crate::i18n::tr!("log.nothing_known"));
        return;
    }

    for f in &c.files {
        print_file_line(f, level);
    }
}

pub(super) fn first_line(msg: &str) -> String {
    msg.lines().next().unwrap_or("").to_string()
}

pub(super) fn format_refs(refs: &[String]) -> String {
    // HEAD primero si está; el resto en orden alfabético estable.
    let mut sorted = refs.to_vec();
    sorted.sort_by(|a, b| match (a == "HEAD", b == "HEAD") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.cmp(b),
    });
    format!("({})", sorted.join(", "))
}



fn print_file_line(f: &FileSummary, level: DetailLevel) {
    println!("          {}  {}", f.path, format_counts(f, true));
    for w in warning_lines(f, "              ") {
        println!("{w}");
    }
    if matches!(level, DetailLevel::Detalle | DetailLevel::Completo) {
        for d in &f.details {
            print_detail(d, "              ");
        }
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
        assert_eq!(format_timestamp(0), "unknown");
        assert_eq!(format_timestamp(-1), "unknown");
    }
}
