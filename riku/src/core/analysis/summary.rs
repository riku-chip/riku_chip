//! Vistas agregadas (`Summary`) sobre un `DriverDiffReport`.
//!
//! `riku status` y `riku log` necesitan presentar muchos archivos en una sola
//! pantalla. El `DriverDiffReport` completo es demasiado verboso para eso —
//! `FileSummary` es una agregación pensada para listas: pocas claves, fácil de
//! formatear en una línea, y categorizada (semantic / cosmetic / unchanged).
//!
//! El mapa `counts` es flexible a propósito: cada driver decide qué eventos
//! reporta (`components_added`, `nets_renamed`, `polygons_added_M1`, ...). El
//! formateador traduce las claves conocidas a etiquetas humanas; las que no
//! conoce las muestra tal cual. Eso permite añadir formatos sin tocar el core.
//!
//! Las claves canónicas aceptadas por el formateador de texto se documentan
//! en [`labels`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::core::domain::driver::{
    is_layout_element, is_net_element, net_name, DiffEntry, DriverDiffReport,
};
use crate::core::domain::models::{ChangeKind, FileFormat};

/// Cuánta información incluir en el `FileSummary`.
///
/// - `Resumen`: solo `counts` (lo que ya hacíamos en Fase 1).
/// - `Detalle`: además, `details` con entradas legibles (qué componente cambió,
///   qué parámetro pasó de X a Y).
/// - `Completo`: además, `full_report` con el `DriverDiffReport` íntegro.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailLevel {
    #[default]
    Resumen,
    Detalle,
    Completo,
}

impl DetailLevel {
    /// Resuelve el nivel a partir de los flags `--detail` y `--full` del CLI.
    /// `full` tiene precedencia sobre `detail`; ambos en falso → `Resumen`.
    pub fn from_flags(detail: bool, full: bool) -> Self {
        if full {
            Self::Completo
        } else if detail {
            Self::Detalle
        } else {
            Self::Resumen
        }
    }
}

/// Categoría agregada de un archivo en una lista (`riku status`, `riku log`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryCategory {
    /// Hay al menos un cambio no-cosmético.
    Semantic,
    /// Hubo cambios pero todos cosméticos (reposicionamiento, etc.).
    Cosmetic,
    /// Driver no detectó ningún cambio.
    Unchanged,
    /// No hay driver para este formato — Riku no opina.
    Unknown,
    /// El driver crasheó o el blob no se pudo leer.
    Error,
}

impl SummaryCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Cosmetic => "cosmetic",
            Self::Unchanged => "unchanged",
            Self::Unknown => "unknown",
            Self::Error => "error",
        }
    }
}

/// Tipo de entrada de detalle para un cambio puntual.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetailKind {
    ComponentAdded,
    ComponentRemoved,
    ComponentModified,
    ComponentRenamed,
    NetAdded,
    NetRemoved,
    NetModified,
    /// El driver reportó un cambio que no encaja en las categorías anteriores.
    Other,
}

/// Una entrada de detalle: qué cambió y opcionalmente cómo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetailEntry {
    pub kind: DetailKind,
    /// Nombre del elemento ("M3", "vbias", "vin → vin_diff", ...).
    pub element: String,
    /// Parámetros que cambiaron, ej. {"W": "4u → 8u"}. Solo en cambios
    /// `Modified` con before/after disponibles.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub params: BTreeMap<String, String>,
}

/// Vista resumida de un archivo, lista para ser mostrada en una línea.
///
/// `counts` siempre se llena (incluso en nivel resumen).
/// `details` se llena en niveles `Detalle` y `Completo`.
/// `full_report` se llena solo en nivel `Completo`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileSummary {
    pub path: String,
    pub format: FileFormat,
    pub category: SummaryCategory,
    /// Eventos agregados — claves canónicas en [`labels`], otras pasan tal cual.
    pub counts: BTreeMap<String, i64>,
    /// Detalle por entrada. Vacío en nivel resumen.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub details: Vec<DetailEntry>,
    /// Reporte completo del driver. Solo presente en nivel completo.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub full_report: Option<DriverDiffReport>,
    /// Mensajes de error si `category == Error`. Vacío en otros casos.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub errors: Vec<String>,
}

impl FileSummary {
    /// Construye un summary desde un `DriverDiffReport` en nivel resumen.
    ///
    /// Conservado por compatibilidad con consumidores de Fase 1. Equivale a
    /// `from_report_with(report, path, DetailLevel::Resumen)`.
    pub fn from_report(report: &DriverDiffReport, path: &str) -> Self {
        Self::from_report_with(report, path, DetailLevel::Resumen)
    }

    /// Construye un summary desde un `DriverDiffReport` con el nivel solicitado.
    pub fn from_report_with(report: &DriverDiffReport, path: &str, level: DetailLevel) -> Self {
        let mut counts: BTreeMap<String, i64> = BTreeMap::new();
        let mut details: Vec<DetailEntry> = Vec::new();
        let mut semantic_changes = 0i64;
        let mut cosmetic_changes = 0i64;

        for change in &report.changes {
            if change.cosmetic {
                cosmetic_changes += 1;
                continue;
            }
            semantic_changes += 1;

            let is_net = is_net_element(&change.element);
            let is_layout = is_layout_element(&change.element);
            if is_layout {
                continue;
            }

            let (count_key, detail_kind) = classify(is_net, &change.kind, &change.element);
            *counts.entry(count_key.to_string()).or_insert(0) += 1;

            if matches!(level, DetailLevel::Detalle | DetailLevel::Completo) {
                details.push(DetailEntry {
                    kind: detail_kind,
                    element: net_label_or_element(&change.element),
                    params: extract_param_changes(change),
                });
            }
        }

        let category = if semantic_changes > 0 {
            SummaryCategory::Semantic
        } else if cosmetic_changes > 0 {
            SummaryCategory::Cosmetic
        } else {
            SummaryCategory::Unchanged
        };

        let full_report = if matches!(level, DetailLevel::Completo) {
            Some(report.clone())
        } else {
            None
        };

        Self {
            path: path.to_string(),
            format: report.file_type.clone(),
            category,
            counts,
            details,
            full_report,
            errors: Vec::new(),
        }
    }

    pub fn unknown(path: &str) -> Self {
        Self {
            path: path.to_string(),
            format: FileFormat::Unknown,
            category: SummaryCategory::Unknown,
            counts: BTreeMap::new(),
            details: Vec::new(),
            full_report: None,
            errors: Vec::new(),
        }
    }

    pub fn error(path: &str, message: impl Into<String>) -> Self {
        Self {
            path: path.to_string(),
            format: FileFormat::Unknown,
            category: SummaryCategory::Error,
            counts: BTreeMap::new(),
            details: Vec::new(),
            full_report: None,
            errors: vec![message.into()],
        }
    }
}

fn classify(is_net: bool, kind: &ChangeKind, element: &str) -> (&'static str, DetailKind) {
    match (is_net, kind) {
        (true, ChangeKind::Added) => (labels::NETS_ADDED, DetailKind::NetAdded),
        (true, ChangeKind::Removed) => (labels::NETS_REMOVED, DetailKind::NetRemoved),
        (true, ChangeKind::Modified) => (labels::NETS_MODIFIED, DetailKind::NetModified),
        (false, ChangeKind::Added) => (labels::COMPONENTS_ADDED, DetailKind::ComponentAdded),
        (false, ChangeKind::Removed) => (labels::COMPONENTS_REMOVED, DetailKind::ComponentRemoved),
        (false, ChangeKind::Modified) => {
            if element.contains(" → ") {
                (labels::COMPONENTS_RENAMED, DetailKind::ComponentRenamed)
            } else {
                (labels::COMPONENTS_MODIFIED, DetailKind::ComponentModified)
            }
        }
    }
}

fn net_label_or_element(element: &str) -> String {
    net_name(element).to_string()
}

/// Extrae cambios de parámetros (key: "before → after") ignorando posición y
/// rotación, que son cosméticos y ya filtrados por el driver pero pueden
/// aparecer en el mapa.
fn extract_param_changes(entry: &DiffEntry) -> BTreeMap<String, String> {
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

/// Claves canónicas para el mapa `counts`. Mantenidas como constantes para
/// evitar typos y facilitar refactor.
pub mod labels {
    pub const COMPONENTS_ADDED: &str = "components_added";
    pub const COMPONENTS_REMOVED: &str = "components_removed";
    pub const COMPONENTS_MODIFIED: &str = "components_modified";
    pub const COMPONENTS_RENAMED: &str = "components_renamed";
    pub const NETS_ADDED: &str = "nets_added";
    pub const NETS_REMOVED: &str = "nets_removed";
    pub const NETS_MODIFIED: &str = "nets_modified";
}

/// Traduce una clave canónica a etiqueta corta humana (singular/plural).
/// Devuelve `None` si la clave no es canónica — el formateador puede entonces
/// imprimir la clave tal cual.
pub fn label_for(key: &str, count: i64) -> Option<String> {
    let plural = count.abs() != 1;
    let label = match key {
        labels::COMPONENTS_ADDED if !plural => "componente añadido",
        labels::COMPONENTS_ADDED => "componentes añadidos",
        labels::COMPONENTS_REMOVED if !plural => "componente eliminado",
        labels::COMPONENTS_REMOVED => "componentes eliminados",
        labels::COMPONENTS_MODIFIED if !plural => "componente modificado",
        labels::COMPONENTS_MODIFIED => "componentes modificados",
        labels::COMPONENTS_RENAMED if !plural => "componente renombrado",
        labels::COMPONENTS_RENAMED => "componentes renombrados",
        labels::NETS_ADDED if !plural => "net añadida",
        labels::NETS_ADDED => "nets añadidas",
        labels::NETS_REMOVED if !plural => "net eliminada",
        labels::NETS_REMOVED => "nets eliminadas",
        labels::NETS_MODIFIED if !plural => "net modificada",
        labels::NETS_MODIFIED => "nets modificadas",
        _ => return None,
    };
    Some(label.to_string())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::driver::DiffEntry;

    fn entry(kind: ChangeKind, element: &str, cosmetic: bool) -> DiffEntry {
        DiffEntry {
            kind,
            element: element.to_string(),
            before: None,
            after: None,
            cosmetic,
            position_changed: false,
        }
    }

    fn report(entries: Vec<DiffEntry>) -> DriverDiffReport {
        DriverDiffReport {
            file_type: FileFormat::Xschem,
            changes: entries,
            ..Default::default()
        }
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
    fn label_for_singular_y_plural() {
        assert_eq!(
            label_for(labels::COMPONENTS_ADDED, 1).unwrap(),
            "componente añadido"
        );
        assert_eq!(
            label_for(labels::COMPONENTS_ADDED, 3).unwrap(),
            "componentes añadidos"
        );
        assert_eq!(label_for("clave_desconocida", 1), None);
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
    fn detalle_extrae_cambios_de_parametros() {
        let mut before = BTreeMap::new();
        before.insert("W".to_string(), "4u".to_string());
        before.insert("L".to_string(), "180n".to_string());
        before.insert("x".to_string(), "100".to_string()); // debe ignorarse
        let mut after = BTreeMap::new();
        after.insert("W".to_string(), "8u".to_string());
        after.insert("L".to_string(), "180n".to_string());
        after.insert("x".to_string(), "200".to_string());

        let mut e = entry(ChangeKind::Modified, "M3", false);
        e.before = Some(before);
        e.after = Some(after);
        let r = report(vec![e]);

        let s = FileSummary::from_report_with(&r, "a.sch", DetailLevel::Detalle);
        let d = &s.details[0];
        assert_eq!(d.params.get("W").map(String::as_str), Some("4u → 8u"));
        assert!(!d.params.contains_key("x"));
        assert!(!d.params.contains_key("L"));
    }
}
