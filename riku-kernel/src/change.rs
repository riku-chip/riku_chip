//! Modelo de cambios entre dos versiones de un archivo.

use serde::{Deserialize, Serialize};

use crate::FileFormat;

/// Todo lo que cambió en un archivo entre dos versiones.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileChange {
    pub format: FileFormat,
    pub changes: Vec<Change>,
    /// Problemas no fatales (un lado ilegible, formato inesperado…).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl FileChange {
    pub fn new(format: FileFormat) -> Self {
        Self { format, ..Default::default() }
    }

    /// `true` si no hay ningún cambio funcional (solo cosméticos o nada).
    pub fn is_empty(&self) -> bool {
        self.changes.iter().all(|c| c.cosmetic)
    }

    /// Cambios funcionales (no cosméticos).
    pub fn functional(&self) -> impl Iterator<Item = &Change> {
        self.changes.iter().filter(|c| !c.cosmetic)
    }

    /// Cantidad de cambios cosméticos.
    pub fn cosmetic_count(&self) -> usize {
        self.changes.iter().filter(|c| c.cosmetic).count()
    }
}

/// Qué le pasó al elemento.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
    /// Mismo elemento con otro nombre; el nombre anterior está en
    /// [`Change::renamed_from`].
    Renamed,
}

/// Un cambio puntual.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub kind: ChangeKind,
    pub element: Element,
    /// No altera la función (reposicionar, redondeos bajo el umbral…).
    pub cosmetic: bool,
    /// Además del cambio, el elemento se movió (posición, rotación o espejo).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub position_changed: bool,
    /// Nombre anterior si `kind == Renamed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renamed_from: Option<String>,
    /// Dónde está el cambio, en unidades del formato (µm en layouts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Bounds>,
    /// Propiedades del elemento antes y después.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<Detail>,
}

impl Change {
    pub fn new(kind: ChangeKind, element: Element) -> Self {
        Self {
            kind,
            element,
            cosmetic: false,
            position_changed: false,
            renamed_from: None,
            location: None,
            details: Vec::new(),
        }
    }

    pub fn cosmetic(mut self, cosmetic: bool) -> Self {
        self.cosmetic = cosmetic;
        self
    }

    pub fn with_detail(mut self, key: impl Into<String>, before: Option<Value>, after: Option<Value>) -> Self {
        self.details.push(Detail { key: key.into(), before, after });
        self
    }

    /// Valor de una propiedad en la versión posterior.
    pub fn after(&self, key: &str) -> Option<&Value> {
        self.details.iter().find(|d| d.key == key).and_then(|d| d.after.as_ref())
    }

    /// Valor de una propiedad en la versión anterior.
    pub fn before(&self, key: &str) -> Option<&Value> {
        self.details.iter().find(|d| d.key == key).and_then(|d| d.before.as_ref())
    }
}

/// Qué cambió, tipado. Cada formato usa las variantes que le corresponden.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Element {
    /// Instancia de un esquemático (`M3`, `R1`).
    Component { name: String },
    /// Net de un esquemático.
    Net { name: String },
    /// El archivo entero (p. ej. todo el esquemático se movió).
    Whole,
    /// Celda de un layout.
    Cell { name: String },
    /// Geometría de una capa dentro de una celda de un layout.
    Geometry {
        cell: String,
        layer: u32,
        datatype: u32,
        /// Si el cambio viene de una sub-celda instanciada.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        via: Option<Via>,
    },
    /// Señal de una simulación (`v(out)`, `i(vdd)`) dentro de un análisis
    /// (`Transient Analysis`).
    Signal { plot: String, name: String },
}

impl Element {
    /// Nombre corto para mostrar.
    pub fn name(&self) -> String {
        match self {
            Self::Component { name } | Self::Net { name } | Self::Cell { name } => name.clone(),
            Self::Whole => "(archivo)".into(),
            Self::Signal { name, .. } => name.clone(),
            Self::Geometry { cell, layer, datatype, via } => match via {
                Some(v) => format!("{cell}:L{layer}/{datatype}:{}", v.path.join("/")),
                None => format!("{cell}:L{layer}/{datatype}"),
            },
        }
    }
}

/// Sub-celda por la que llega un cambio de geometría.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Via {
    /// Celdas desde la raíz, sin incluirla (`["INV"]`).
    pub path: Vec<String>,
    /// Instancias agrupadas en este cambio (1 = una instancia concreta).
    pub instances: usize,
    /// Posición de la instancia si es una sola.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 2]>,
}

/// Rectángulo en unidades del formato.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

/// Propiedad de un elemento antes y después.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Detail {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
}

impl Detail {
    /// `true` si la propiedad cambió (incluye aparecer o desaparecer).
    pub fn changed(&self) -> bool {
        self.before != self.after
    }
}

/// Valor tipado de una propiedad (en JSON: número, texto o booleano).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(i) => Some(*i as f64),
            Self::Float(f) => Some(*f),
            _ => None,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(x) => write!(f, "{x:.3}"),
            Self::Text(s) => f.write_str(s),
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}
impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}
impl From<usize> for Value {
    fn from(i: usize) -> Self {
        Self::Int(i as i64)
    }
}
impl From<f64> for Value {
    fn from(x: f64) -> Self {
        Self::Float(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_json_is_typed() {
        let c = Change::new(
            ChangeKind::Added,
            Element::Geometry {
                cell: "TOP".into(),
                layer: 1,
                datatype: 0,
                via: Some(Via { path: vec!["INV".into()], instances: 2, at: None }),
            },
        )
        .with_detail("added_area_um2", None, Some(1.0.into()));
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["kind"], "added");
        assert_eq!(v["element"]["type"], "geometry");
        assert_eq!(v["element"]["via"]["instances"], 2);
        assert_eq!(v["details"][0]["after"], 1.0);
        assert!(v.get("renamed_from").is_none() && v.get("location").is_none());
        let back: Change = serde_json::from_value(v).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn empty_means_only_cosmetic() {
        let mut f = FileChange::new(FileFormat::Xschem);
        assert!(f.is_empty());
        f.changes.push(Change::new(ChangeKind::Modified, Element::Whole).cosmetic(true));
        assert!(f.is_empty());
        assert_eq!(f.cosmetic_count(), 1);
        f.changes.push(Change::new(ChangeKind::Added, Element::Net { name: "vdd".into() }));
        assert!(!f.is_empty());
        assert_eq!(f.functional().count(), 1);
    }
}
