//! Constructor `FileSummary::from_report*` y helpers de clasificación.
//!
//! Recorre los cambios de un `FileChange`, los cuenta, agrega detalles y
//! decide la categoría agregada. La separación del shape (en `types`) y de
//! las etiquetas (en `labels`) deja este archivo enfocado en la lógica de
//! agregación.

use std::collections::BTreeMap;

use riku_kernel::legacy::{self, LegacyEntry};

use crate::core::domain::models::{Change, ChangeKind, Element, FileChange};

use super::labels;
use super::types::{DetailEntry, DetailKind, DetailLevel, FileSummary, SummaryCategory};

impl FileSummary {
    /// Construye un summary desde un `FileChange` en nivel resumen.
    ///
    /// Conservado por compatibilidad con consumidores de Fase 1. Equivale a
    /// `from_report_with(report, path, DetailLevel::Resumen)`.
    pub fn from_report(report: &FileChange, path: &str) -> Self {
        Self::from_report_with(report, path, DetailLevel::Resumen)
    }

    /// Construye un summary desde un `FileChange` con el nivel solicitado.
    pub fn from_report_with(report: &FileChange, path: &str, level: DetailLevel) -> Self {
        let agg = aggregate_changes(report, level);
        let category = decide_category(agg.semantic, agg.cosmetic);
        let full_report = matches!(level, DetailLevel::Completo).then(|| report.clone());

        Self {
            path: path.to_string(),
            format: report.format.clone(),
            category,
            counts: agg.counts,
            details: agg.details,
            full_report,
            errors: Vec::new(),
        }
    }
}

/// Resultado de recorrer los cambios de un `FileChange`: contadores
/// agregados por tipo de cambio, detalles opcionales y conteos brutos de
/// semánticos/cosméticos para que el caller decida la categoría.
struct Aggregated {
    counts: BTreeMap<String, i64>,
    details: Vec<DetailEntry>,
    semantic: i64,
    cosmetic: i64,
}

fn aggregate_changes(report: &FileChange, level: DetailLevel) -> Aggregated {
    let mut counts: BTreeMap<String, i64> = BTreeMap::new();
    let mut details: Vec<DetailEntry> = Vec::new();
    let mut semantic = 0i64;
    let mut cosmetic = 0i64;

    // La forma v1 da el nombre y los parámetros tal como los mostraban
    // `status`/`log` (su JSON es riku-status/v1 y riku-log/v1).
    for (change, entry) in report.changes.iter().zip(legacy::entries(report)) {
        if change.cosmetic {
            cosmetic += 1;
            continue;
        }
        semantic += 1;

        if matches!(change.element, Element::Whole) {
            continue;
        }

        let (count_key, detail_kind) = classify(change);
        *counts.entry(count_key.to_string()).or_insert(0) += 1;

        if matches!(level, DetailLevel::Detalle | DetailLevel::Completo) {
            let element = match &change.element {
                Element::Net { name } => name.clone(),
                _ => entry.element.clone(),
            };
            details.push(DetailEntry { kind: detail_kind, element, params: extract_param_changes(&entry) });
        }
    }

    Aggregated {
        counts,
        details,
        semantic,
        cosmetic,
    }
}

/// Regla de negocio: prioridad `Semantic > Cosmetic > Unchanged`.
fn decide_category(semantic: i64, cosmetic: i64) -> SummaryCategory {
    if semantic > 0 {
        SummaryCategory::Semantic
    } else if cosmetic > 0 {
        SummaryCategory::Cosmetic
    } else {
        SummaryCategory::Unchanged
    }
}

/// Nets por un lado; componentes, celdas y geometría cuentan como
/// "componentes" (así lo reportaban `status`/`log` v1).
fn classify(change: &Change) -> (&'static str, DetailKind) {
    let is_net = matches!(change.element, Element::Net { .. });
    match (is_net, change.kind) {
        (true, ChangeKind::Added) => (labels::NETS_ADDED, DetailKind::NetAdded),
        (true, ChangeKind::Removed) => (labels::NETS_REMOVED, DetailKind::NetRemoved),
        (true, ChangeKind::Modified | ChangeKind::Renamed) => (labels::NETS_MODIFIED, DetailKind::NetModified),
        (false, ChangeKind::Added) => (labels::COMPONENTS_ADDED, DetailKind::ComponentAdded),
        (false, ChangeKind::Removed) => (labels::COMPONENTS_REMOVED, DetailKind::ComponentRemoved),
        (false, ChangeKind::Renamed) => (labels::COMPONENTS_RENAMED, DetailKind::ComponentRenamed),
        (false, ChangeKind::Modified) => (labels::COMPONENTS_MODIFIED, DetailKind::ComponentModified),
    }
}

/// Extrae cambios de parámetros (key: "before → after") ignorando posición y
/// rotación, que son cosméticos y ya filtrados por el driver pero pueden
/// aparecer en el mapa.
fn extract_param_changes(entry: &LegacyEntry) -> BTreeMap<String, String> {
    let (before, after) = match (&entry.before, &entry.after) {
        (Some(b), Some(a)) => (b, a),
        _ => return BTreeMap::new(),
    };
    let mut out = BTreeMap::new();
    for key in before.keys().chain(after.keys()) {
        if matches!(key.as_str(), "x" | "y" | "rotation" | "mirror") {
            continue;
        }
        let b = before.get(key);
        let a = after.get(key);
        match (b, a) {
            (Some(bv), Some(av)) if bv != av => {
                out.insert(key.clone(), format!("{bv} → {av}"));
            }
            (None, Some(av)) => {
                out.insert(key.clone(), format!("(nuevo) → {av}"));
            }
            (Some(bv), None) => {
                out.insert(key.clone(), format!("{bv} → (eliminado)"));
            }
            _ => {}
        }
    }
    out
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::models::{FileFormat, Value};

    /// Atajo de los tests: elementos con la notación v1 (`net:x`, `layout`,
    /// `a → b`) para leerlos igual que antes.
    fn entry(kind: ChangeKind, element: &str, cosmetic: bool) -> Change {
        let (kind, el, from) = if element == "layout" {
            (kind, Element::Whole, None)
        } else if let Some(n) = element.strip_prefix("net:") {
            (kind, Element::Net { name: n.into() }, None)
        } else if let Some((a, b)) = element.split_once(" → ") {
            (ChangeKind::Renamed, Element::Component { name: b.into() }, Some(a.to_string()))
        } else {
            (kind, Element::Component { name: element.into() }, None)
        };
        let mut c = Change::new(kind, el).cosmetic(cosmetic);
        c.renamed_from = from;
        c
    }

    fn report(entries: Vec<Change>) -> FileChange {
        FileChange { format: FileFormat::Xschem, changes: entries, warnings: Vec::new() }
    }

    #[test]
    fn solo_cosmeticos_es_categoria_cosmetic() {
        let r = report(vec![entry(ChangeKind::Modified, "layout", true)]);
        let s = FileSummary::from_report(&r, "a.sch");
        assert_eq!(s.category, SummaryCategory::Cosmetic);
        assert!(s.counts.is_empty());
    }

    #[test]
    fn semantico_cuenta_componentes_y_nets() {
        let r = report(vec![
            entry(ChangeKind::Added, "M1", false),
            entry(ChangeKind::Added, "M2", false),
            entry(ChangeKind::Removed, "net:vbias", false),
            entry(ChangeKind::Modified, "vin → vin_diff", false),
        ]);
        let s = FileSummary::from_report(&r, "a.sch");
        assert_eq!(s.category, SummaryCategory::Semantic);
        assert_eq!(s.counts.get(labels::COMPONENTS_ADDED), Some(&2));
        assert_eq!(s.counts.get(labels::NETS_REMOVED), Some(&1));
        assert_eq!(s.counts.get(labels::COMPONENTS_RENAMED), Some(&1));
    }

    #[test]
    fn sin_cambios_es_unchanged() {
        let r = report(vec![]);
        let s = FileSummary::from_report(&r, "a.sch");
        assert_eq!(s.category, SummaryCategory::Unchanged);
    }

    #[test]
    fn cambios_en_layout_no_cuentan_pero_marcan_cosmetic() {
        let r = report(vec![entry(ChangeKind::Modified, "layout", true)]);
        let s = FileSummary::from_report(&r, "a.sch");
        assert_eq!(s.category, SummaryCategory::Cosmetic);
        assert!(s.counts.is_empty());
    }

    #[test]
    fn nivel_resumen_no_llena_details_ni_full_report() {
        let r = report(vec![entry(ChangeKind::Added, "M1", false)]);
        let s = FileSummary::from_report_with(&r, "a.sch", DetailLevel::Resumen);
        assert!(s.details.is_empty());
        assert!(s.full_report.is_none());
    }

    #[test]
    fn nivel_detalle_llena_details_pero_no_full_report() {
        let r = report(vec![entry(ChangeKind::Added, "M1", false)]);
        let s = FileSummary::from_report_with(&r, "a.sch", DetailLevel::Detalle);
        assert_eq!(s.details.len(), 1);
        assert_eq!(s.details[0].kind, DetailKind::ComponentAdded);
        assert_eq!(s.details[0].element, "M1");
        assert!(s.full_report.is_none());
    }

    #[test]
    fn nivel_completo_llena_todo() {
        let r = report(vec![entry(ChangeKind::Added, "M1", false)]);
        let s = FileSummary::from_report_with(&r, "a.sch", DetailLevel::Completo);
        assert_eq!(s.details.len(), 1);
        assert!(s.full_report.is_some());
    }

    #[test]
    fn decide_category_prioriza_semantic_sobre_cosmetic() {
        assert_eq!(decide_category(1, 5), SummaryCategory::Semantic);
        assert_eq!(decide_category(0, 3), SummaryCategory::Cosmetic);
        assert_eq!(decide_category(0, 0), SummaryCategory::Unchanged);
    }

    #[test]
    fn detalle_extrae_cambios_de_parametros() {
        let t = |s: &str| Some(Value::Text(s.into()));
        let e = entry(ChangeKind::Modified, "M3", false)
            .with_detail("W", t("4u"), t("8u"))
            .with_detail("L", t("180n"), t("180n"))
            .with_detail("x", t("100"), t("200")); // debe ignorarse
        let r = report(vec![e]);

        let s = FileSummary::from_report_with(&r, "a.sch", DetailLevel::Detalle);
        let d = &s.details[0];
        assert_eq!(d.params.get("W").map(String::as_str), Some("4u → 8u"));
        assert!(!d.params.contains_key("x"));
        assert!(!d.params.contains_key("L"));
    }

    #[test]
    fn renombre_y_net_mantienen_los_nombres_de_v1() {
        let r = report(vec![entry(ChangeKind::Modified, "vin → vin_diff", false), entry(ChangeKind::Added, "net:vdd", false)]);
        let s = FileSummary::from_report_with(&r, "a.sch", DetailLevel::Detalle);
        assert_eq!(s.details[0].element, "vin → vin_diff");
        assert_eq!(s.details[0].kind, DetailKind::ComponentRenamed);
        assert_eq!(s.details[1].element, "vdd");
    }
}
