//! Helpers compartidos por los formateadores de texto del CLI.
//!
//! Cada salida (`diff`, `log`, `status`) tenía su propia copia del switch de
//! marker y del `format_counts`. Aquí viven las versiones únicas; los
//! formateadores solo difieren en composición e indentación.

use crate::core::analysis::summary::{DetailEntry, FileSummary, SummaryCategory, label_for};
use crate::core::domain::models::ChangeKind;
use crate::i18n::tr;

/// Marker de una sola letra para un `ChangeKind` (`r` = renombre).
pub(super) fn marker_for_change(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "+",
        ChangeKind::Removed => "-",
        ChangeKind::Modified => "~",
        ChangeKind::Renamed => "r",
    }
}

/// Formato corto de los `counts` de un `FileSummary`. Cuando
/// `allow_cosmetic_label` es true, los archivos `Cosmetic` se etiquetan
/// explícitamente; cuando es false, se asume que el caller ya los separó en
/// otra sección y solo procesa `counts`.
pub(super) fn format_counts(f: &FileSummary, allow_cosmetic_label: bool) -> String {
    if allow_cosmetic_label && matches!(f.category, SummaryCategory::Cosmetic) {
        return tr!("summary.only_cosmetic");
    }
    match f.category {
        SummaryCategory::Error => {
            let msg = f.errors.first().cloned().unwrap_or_else(|| tr!("status.no_detail"));
            return format!("{} {msg}", super::color::red(&tr!("diff.failed")));
        }
        SummaryCategory::Unchanged => return tr!("summary.unchanged"),
        _ => {}
    }
    if f.counts.is_empty() {
        return tr!("summary.no_detail");
    }
    let mut parts = Vec::with_capacity(f.counts.len());
    for (key, count) in &f.counts {
        let label = label_for(key, *count).unwrap_or_else(|| key.clone());
        parts.push(format!("{count} {label}"));
    }
    parts.join(", ")
}

/// Los avisos del módulo para un archivo, una línea cada uno.
pub(super) fn warning_lines(f: &FileSummary, indent: &str) -> Vec<String> {
    f.warnings.iter().map(|w| format!("{indent}[!] {w}")).collect()
}

/// Imprime una entrada de detalle con la indentación dada. La indentación se
/// pasa como string para que cada formateador conserve su estética sin
/// necesidad de un parámetro `level`.
pub(super) fn print_detail(d: &DetailEntry, indent: &str) {
    for line in detail_lines(d, indent) {
        println!("{line}");
    }
}

/// Las líneas de [`print_detail`], sin imprimirlas.
pub(super) fn detail_lines(d: &DetailEntry, indent: &str) -> Vec<String> {
    let mut out = vec![format!("{indent}{} {}", d.kind.marker(), d.element)];
    for (k, v) in &d.params {
        out.push(format!("{indent}    {k}: {v}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::analysis::summary::DetailKind;
    use std::collections::BTreeMap;

    #[test]
    fn marker_rename_se_detecta_por_flecha_en_nombre() {
        assert_eq!(marker_for_change(ChangeKind::Renamed), "r");
        assert_eq!(marker_for_change(ChangeKind::Modified), "~");
    }

    #[test]
    fn marker_added_y_removed_se_mapean_directo() {
        assert_eq!(marker_for_change(ChangeKind::Added), "+");
        assert_eq!(marker_for_change(ChangeKind::Removed), "-");
    }

    #[test]
    fn format_counts_cosmetico_etiquetado_solo_si_se_permite() {
        let mut f = FileSummary::unknown("a.sch");
        f.category = SummaryCategory::Cosmetic;
        assert_eq!(format_counts(&f, true), "(cosmetic changes only)");
        // Sin permiso, cae en la rama de counts vacíos.
        assert_eq!(format_counts(&f, false), "(changes without detail)");
    }

    #[test]
    fn detail_kind_marker_es_consistente_con_marker_for_change() {
        assert_eq!(DetailKind::ComponentAdded.marker(), "+");
        assert_eq!(DetailKind::ComponentRemoved.marker(), "-");
        assert_eq!(DetailKind::ComponentRenamed.marker(), "r");
        assert_eq!(DetailKind::ComponentModified.marker(), "~");
        assert_eq!(DetailKind::Other.marker(), "~");
    }

    #[test]
    fn print_detail_no_panica_con_params_vacios() {
        let d = DetailEntry {
            kind: DetailKind::ComponentAdded,
            element: "M1".into(),
            params: BTreeMap::new(),
        };
        print_detail(&d, "  ");
    }
}
