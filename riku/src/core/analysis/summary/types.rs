//! Tipos del modelo de `Summary`: niveles, categorías y la struct `FileSummary`.
//!
//! Los constructores no triviales (`from_report`, `from_report_with`) viven en
//! [`super::build`] para mantener este archivo enfocado en el shape.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::core::domain::models::{FileChange, FileFormat};

/// Cuánta información incluir en el `FileSummary`.
///
/// - `Resumen`: solo `counts` (lo que ya hacíamos en Fase 1).
/// - `Detalle`: además, `details` con entradas legibles (qué componente cambió,
///   qué parámetro pasó de X a Y).
/// - `Completo`: además, `full_report` con el `FileChange` íntegro.
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
    SignalAdded,
    SignalRemoved,
    SignalModified,
    /// El driver reportó un cambio que no encaja en las categorías anteriores.
    Other,
}

impl DetailKind {
    /// Marker de una sola letra usado por los formateadores de texto del CLI.
    pub fn marker(&self) -> &'static str {
        match self {
            Self::ComponentAdded | Self::NetAdded | Self::SignalAdded => "+",
            Self::ComponentRemoved | Self::NetRemoved | Self::SignalRemoved => "-",
            Self::ComponentRenamed => "r",
            Self::ComponentModified | Self::NetModified | Self::SignalModified | Self::Other => "~",
        }
    }
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
    /// Eventos agregados — claves canónicas en [`super::labels`], otras pasan tal cual.
    pub counts: BTreeMap<String, i64>,
    /// Detalle por entrada. Vacío en nivel resumen.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub details: Vec<DetailEntry>,
    /// Reporte completo del driver. Solo presente en nivel completo.
    /// En el JSON sale con la forma v1 (schemas riku-status/v1 y riku-log/v1).
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "full_report_v1", skip_deserializing, default)]
    pub full_report: Option<FileChange>,
    /// Mensajes de error si `category == Error`. Vacío en otros casos.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub errors: Vec<String>,
}

impl FileSummary {
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

fn full_report_v1<S: serde::Serializer>(report: &Option<FileChange>, s: S) -> Result<S::Ok, S::Error> {
    match report {
        Some(r) => serde::Serialize::serialize(&riku_kernel::legacy::driver_report(r), s),
        None => s.serialize_none(),
    }
}
