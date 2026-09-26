//! Formateador de texto para `riku status`.
//!
//! Soporta los tres niveles de detalle definidos en la spec:
//! - `Resumen`: una línea por archivo con contadores agregados.
//! - `Detalle`: añade entradas por componente/net cambiada.
//! - `Completo`: imprime el `FileChange` íntegro tras el resumen.

use super::common::{format_counts, print_detail};
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
            let msg = f
                .errors
                .first()
                .map(String::as_str)
                .unwrap_or("(sin detalle)");
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
    println!("      ── reporte completo ──");
    if rep.changes.is_empty() {
        println!("      (sin entradas)");
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
        println!("      {marker} {}{cosmetic}", e.element);
    }
    if !rep.warnings.is_empty() {
        println!("      avisos del driver:");
        for w in &rep.warnings {
            println!("        - {w}");
        }
    }
}
