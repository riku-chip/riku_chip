//! Formateador de texto para `riku status`.
//!
//! Soporta los tres niveles de detalle definidos en la spec:
//! - `Resumen`: una línea por archivo con contadores agregados.
//! - `Detalle`: añade entradas por componente/net cambiada.
//! - `Completo`: imprime el `FileChange` íntegro tras el resumen.

use super::common::{format_counts, print_detail};
use crate::i18n::tr;
use crate::core::analysis::status::StatusReport;
use crate::core::analysis::summary::{DetailLevel, FileSummary, SummaryCategory};
use crate::core::domain::models::FileChange;

/// Imprime el reporte completo en stdout, los warnings en stderr.
pub fn print(report: &StatusReport, level: DetailLevel, include_unknown: bool) {
    print_header(report);
    for w in &report.warnings {
        eprintln!("[!] {w}");
    }
    print_categorized(report, level, include_unknown);
}

fn print_header(report: &StatusReport) {
    if let Some(b) = &report.branch {
        let mut header = tr!("status.branch", branch = b.name, head = b.head_short);
        if let Some(up) = &b.upstream {
            let rel = describe_upstream(b.ahead, b.behind);
            header.push_str(&format!(" — vs {up}: {rel}"));
        }
        println!("{header}");
    } else {
        println!("{}", tr!("status.no_head"));
    }
}

fn describe_upstream(ahead: usize, behind: usize) -> String {
    let mut parts = Vec::new();
    if ahead > 0 {
        parts.push(tr!("status.ahead", count = ahead));
    }
    if behind > 0 {
        parts.push(tr!("status.behind", count = behind));
    }
    if parts.is_empty() {
        tr!("status.up_to_date")
    } else {
        parts.join(", ")
    }
}

fn print_categorized(report: &StatusReport, level: DetailLevel, include_unknown: bool) {
    if report.files.is_empty() {
        println!();
        println!("{}", tr!("status.clean"));
        return;
    }

    let by = |cat: SummaryCategory| -> Vec<&FileSummary> {
        report.files.iter().filter(|f| f.category == cat).collect()
    };

    let semantic = by(SummaryCategory::Semantic);
    let cosmetic = by(SummaryCategory::Cosmetic);
    let unchanged = by(SummaryCategory::Unchanged);
    let unknown = by(SummaryCategory::Unknown);
    let errored = by(SummaryCategory::Error);

    if !semantic.is_empty() {
        println!();
        println!("{}", super::color::bold(&tr!("status.semantic")));
        for f in &semantic {
            print_file_entry(f, level);
        }
    }
    if !cosmetic.is_empty() {
        println!();
        println!("{}", tr!("status.cosmetic"));
        for f in &cosmetic {
            println!("  {}    {}", f.path, tr!("summary.only_cosmetic"));
        }
    }
    if !unchanged.is_empty() {
        println!();
        println!("{}", tr!("status.unchanged"));
        for f in &unchanged {
            println!("  {}", f.path);
        }
    }
    if !errored.is_empty() {
        println!();
        println!("{}", super::color::red(&tr!("status.errors")));
        for f in &errored {
            let msg = f.errors.first().cloned().unwrap_or_else(|| tr!("status.no_detail"));
            println!("  {}    {msg}", f.path);
        }
    }
    if !unknown.is_empty() {
        if include_unknown {
            println!();
            println!("{}", tr!("status.unknown"));
            for f in &unknown {
                println!("  {}", f.path);
            }
        } else {
            println!();
            println!("{}", tr!("status.unknown_count", count = unknown.len()));
        }
    }
}

fn print_file_entry(f: &FileSummary, level: DetailLevel) {
    println!("  {}    {}", f.path, format_counts(f, false));
    if matches!(level, DetailLevel::Detalle | DetailLevel::Completo) {
        for d in &f.details {
            print_detail(d, "      ");
        }
    }
    if let (DetailLevel::Completo, Some(rep)) = (level, &f.full_report) {
        print_full_report(rep);
    }
}

fn print_full_report(rep: &FileChange) {
    println!("      {}", tr!("status.full_report"));
    if rep.changes.is_empty() {
        println!("      {}", tr!("status.no_entries"));
        return;
    }
    // Misma notación que el JSON `full_report` (v1): `cell:INV`, `net:vdd`…
    for e in riku_kernel::legacy::entries(rep) {
        let marker = match e.kind {
            "added" => "+",
            "removed" => "-",
            _ => "~",
        };
        let cosmetic = if e.cosmetic { " [cosmetic]" } else { "" };
        println!("      {} {}{cosmetic}", super::color::marker(marker), e.element);
    }
    if !rep.warnings.is_empty() {
        println!("      {}", tr!("status.module_warnings"));
        for w in &rep.warnings {
            println!("        - {w}");
        }
    }
}
