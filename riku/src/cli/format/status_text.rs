//! Formateador de texto para `riku status`.
//!
//! Soporta los tres niveles de detalle definidos en la spec:
//! - `Resumen`: una línea por archivo con contadores agregados.
//! - `Detalle`: añade entradas por componente/net cambiada.
//! - `Completo`: imprime el `DriverDiffReport` íntegro tras el resumen.

use crate::core::driver::DriverDiffReport;
use crate::core::models::ChangeKind;
use crate::core::analysis::status::StatusReport;
use crate::core::analysis::summary::{label_for, DetailEntry, DetailLevel, FileSummary, SummaryCategory};

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
        let mut header = format!("En rama {} (HEAD {})", b.name, b.head_short);
        if let Some(up) = &b.upstream {
            let rel = describe_upstream(b.ahead, b.behind);
            header.push_str(&format!(" — vs {up}: {rel}"));
        }
        println!("{header}");
    } else {
        println!("Repositorio sin HEAD (commit inicial pendiente).");
    }
}

fn describe_upstream(ahead: usize, behind: usize) -> String {
    let mut parts = Vec::new();
    if ahead > 0 {
        parts.push(format!("{ahead} adelante"));
    }
    if behind > 0 {
        parts.push(format!("{behind} atrás"));
    }
    if parts.is_empty() {
        "al día".to_string()
    } else {
        parts.join(", ")
    }
}

fn print_categorized(report: &StatusReport, level: DetailLevel, include_unknown: bool) {
    if report.files.is_empty() {
        println!();
        println!("Sin cambios.");
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
        println!("Modificados con cambios semánticos:");
        for f in &semantic {
            print_file_entry(f, level);
        }
    }
    if !cosmetic.is_empty() {
        println!();
        println!("Modificados sin cambios semánticos:");
        for f in &cosmetic {
            println!("  {}    (solo cambios cosméticos)", f.path);
        }
    }
    if !unchanged.is_empty() {
        println!();
        println!("Modificados sin diferencias detectadas por driver:");
        for f in &unchanged {
            println!("  {}", f.path);
        }
    }
    if !errored.is_empty() {
        println!();
        println!("Errores al analizar:");
        for f in &errored {
            let msg = f.errors.first().map(String::as_str).unwrap_or("(sin detalle)");
            println!("  {}    {msg}", f.path);
        }
    }
    if !unknown.is_empty() {
        if include_unknown {
            println!();
            println!("No reconocidos por Riku:");
            for f in &unknown {
                println!("  {}", f.path);
            }
        } else {
            println!();
            println!(
                "No reconocidos por Riku ({}): use --include-unknown para listarlos.",
                unknown.len()
            );
        }
    }
}

fn print_file_entry(f: &FileSummary, level: DetailLevel) {
    println!("  {}    {}", f.path, format_counts(f));
    if matches!(level, DetailLevel::Detalle | DetailLevel::Completo) {
        for d in &f.details {
            print_detail_entry(d);
        }
    }
    if let (DetailLevel::Completo, Some(rep)) = (level, &f.full_report) {
        print_full_report(rep);
    }
}

fn format_counts(f: &FileSummary) -> String {
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

fn print_detail_entry(d: &DetailEntry) {
    use crate::core::analysis::summary::DetailKind::*;
    let marker = match d.kind {
        ComponentAdded | NetAdded => "+",
        ComponentRemoved | NetRemoved => "-",
        ComponentRenamed => "r",
        ComponentModified | NetModified | Other => "~",
    };
    println!("      {marker} {}", d.element);
    for (k, v) in &d.params {
        println!("          {k}: {v}");
    }
}

fn print_full_report(rep: &DriverDiffReport) {
    println!("      ── reporte completo ──");
    if rep.changes.is_empty() {
        println!("      (sin entradas)");
        return;
    }
    for c in &rep.changes {
        let marker = match c.kind {
            ChangeKind::Added => "+",
            ChangeKind::Removed => "-",
            ChangeKind::Modified => "~",
        };
        let cosmetic = if c.cosmetic { " [cosmetic]" } else { "" };
        println!("      {marker} {}{cosmetic}", c.element);
    }
    if !rep.warnings.is_empty() {
        println!("      avisos del driver:");
        for w in &rep.warnings {
            println!("        - {w}");
        }
    }
}
