//! Constructor `FileSummary::from_report*` y helpers de clasificación.
//!
//! Recorre los cambios de un `FileChange`, los cuenta, agrega detalles y
//! decide la categoría agregada. La separación del shape (en `types`) y de
//! las etiquetas (en `labels`) deja este archivo enfocado en la lógica de
//! agregación.

use std::collections::BTreeMap;

use crate::core::domain::models::{Change, ChangeKind, Element, FileChange, FileFormat};

use super::labels;
use super::types::{DetailEntry, DetailKind, DetailLevel, FileSummary, SummaryCategory};
use crate::i18n::tr;

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
        // Otro formato con la extensión de un módulo (ver `diff_pair`).
        if report.error.is_none() && report.format == FileFormat::Unknown {
            return Self { warnings: report.warnings.clone(), ..Self::unknown(path) };
        }
        if let Some(err) = &report.error {
            return Self { format: report.format.clone(), warnings: report.warnings.clone(), ..Self::error(path, err.clone()) };
        }
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
            warnings: report.warnings.clone(),
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

    for change in &report.changes {
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
            let renamed_from = change.renamed_from.clone().filter(|_| change.kind == ChangeKind::Renamed);
            details.push(DetailEntry {
                kind: detail_kind,
                element: change.element.clone(),
                renamed_from,
                params: param_changes(change),
            });
        }
    }

    Aggregated { counts, details, semantic, cosmetic }
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

/// Nets y señales por su lado; componentes, celdas y geometría cuentan
/// como "componentes" (así lo reportaban `status`/`log` v1).
fn classify(change: &Change) -> (&'static str, DetailKind) {
    if matches!(change.element, Element::Signal { .. }) {
        return match change.kind {
            ChangeKind::Added => (labels::SIGNALS_ADDED, DetailKind::SignalAdded),
            ChangeKind::Removed => (labels::SIGNALS_REMOVED, DetailKind::SignalRemoved),
            ChangeKind::Modified | ChangeKind::Renamed => (labels::SIGNALS_MODIFIED, DetailKind::SignalModified),
        };
    }
    // Una red de un layout: un abierto o un corto cuenta aparte.
    if matches!(change.element, Element::LayoutNet { .. }) {
        let kind = change.after("kind").map(|v| v.to_string()).unwrap_or_default();
        return match kind.as_str() {
            "short" => (labels::SHORTS, DetailKind::NetModified),
            "open" => (labels::OPENS, DetailKind::NetModified),
            _ => (labels::NETS_MODIFIED, DetailKind::NetModified),
        };
    }
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

/// Parámetros que cambiaron (`{"W": "4u → 8u"}`), sin la ubicación del
/// elemento. Solo en un componente o puerto modificado o renombrado: en uno
/// añadido o eliminado todos sus valores serían "nuevos" o "eliminados", y
/// una señal lleva su Δ numérico en `riku diff`.
fn param_changes(change: &Change) -> BTreeMap<String, String> {
    let has_params = matches!(change.element, Element::Component { .. } | Element::Port { .. })
        && matches!(change.kind, ChangeKind::Modified | ChangeKind::Renamed);
    if !has_params {
        return BTreeMap::new();
    }
    change
        .params()
        .filter_map(|d| {
            let text = match (&d.before, &d.after) {
                (Some(b), Some(a)) if b != a => format!("{b} → {a}"),
                (None, Some(a)) => format!("{} → {a}", tr!("diff.new")),
                (Some(b), None) => format!("{b} → {}", tr!("diff.deleted")),
                _ => return None,
            };
            Some((d.key.clone(), text))
        })
        .collect()
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
        FileChange { format: FileFormat::Xschem, changes: entries, ..Default::default() }
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
    fn un_modulo_que_falla_es_error_no_unchanged() {
        let r = FileChange::failed(FileFormat::Gds, "(B) no es GDSII");
        let s = FileSummary::from_report(&r, "a.gds");
        assert_eq!(s.category, SummaryCategory::Error);
        assert_eq!(s.format, FileFormat::Gds);
        assert_eq!(s.errors, vec!["(B) no es GDSII".to_string()]);
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
        assert_eq!(s.details[0].label(), "M1");
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
            .with_placement("x", t("100"), t("200")); // ubicación: no es un parámetro
        let r = report(vec![e]);

        let s = FileSummary::from_report_with(&r, "a.sch", DetailLevel::Detalle);
        let d = &s.details[0];
        assert_eq!(d.params.get("W").map(String::as_str), Some("4u → 8u"));
        assert!(!d.params.contains_key("x"));
        assert!(!d.params.contains_key("L"));
    }

    #[test]
    fn renombre_y_net_se_nombran_como_en_diff() {
        let r = report(vec![entry(ChangeKind::Modified, "vin → vin_diff", false), entry(ChangeKind::Added, "net:vdd", false)]);
        let s = FileSummary::from_report_with(&r, "a.sch", DetailLevel::Detalle);
        assert_eq!(s.details[0].label(), "vin → vin_diff");
        assert_eq!(s.details[0].kind, DetailKind::ComponentRenamed);
        assert_eq!(s.details[1].label(), "vdd");
    }
}
