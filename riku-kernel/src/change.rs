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
    /// El módulo no pudo comparar (un lado roto o ilegible): `changes` no
    /// dice nada y el archivo no cuenta como "sin cambios".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl FileChange {
    pub fn new(format: FileFormat) -> Self {
        Self { format, ..Default::default() }
    }

    /// Un archivo que no se pudo comparar.
    pub fn failed(format: FileFormat, error: impl Into<String>) -> Self {
        Self { format, error: Some(error.into()), ..Default::default() }
    }

    /// `true` si se comparó y no hay ningún cambio funcional (solo
    /// cosméticos o nada). Con error es `false`: no se sabe.
    pub fn is_empty(&self) -> bool {
        self.error.is_none() && self.changes.iter().all(|c| c.cosmetic)
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
    /// Qué tan grave es: `error` en un abierto o un corto de un layout. Sin
    /// él, un cambio común.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
}

/// La gravedad de un cambio que no es solo "algo distinto".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Cambia el circuito: un abierto o un corto.
    Error,
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
            severity: None,
        }
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = Some(severity);
        self
    }

    pub fn cosmetic(mut self, cosmetic: bool) -> Self {
        self.cosmetic = cosmetic;
        self
    }

    pub fn with_detail(mut self, key: impl Into<String>, before: Option<Value>, after: Option<Value>) -> Self {
        self.details.push(Detail::new(key, before, after));
        self
    }

    /// Como [`Self::with_detail`], para la ubicación del elemento (ver
    /// [`Detail::placement`]).
    pub fn with_placement(mut self, key: impl Into<String>, before: Option<Value>, after: Option<Value>) -> Self {
        self.details.push(Detail { placement: true, ..Detail::new(key, before, after) });
        self
    }

    /// Las propiedades que no son ubicación (los parámetros del elemento).
    pub fn params(&self) -> impl Iterator<Item = &Detail> {
        self.details.iter().filter(|d| !d.placement)
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
        /// Nombre de la capa si el formato lo da (Magic: `metal1`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer_name: Option<String>,
        /// Si el cambio viene de una sub-celda instanciada.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        via: Option<Via>,
    },
    /// Puerto de una celda de un layout (Magic: `port 1 nsew signal input`).
    Port { cell: String, name: String },
    /// Transistor de una celda de un layout: su modelo y un punto dentro de
    /// su compuerta (µm). Los `details` dicen `model`, `w_um` y `l_um`.
    Device { cell: String, model: String, at: [f64; 2] },
    /// Red de una celda de un layout (su etiqueta, o cómo encontrarla si no
    /// tiene: `d de nfet_01v8 en (1.20, 0.50)`). Los `details` dicen `kind`
    /// (`open`, `short`) y las redes de antes y después.
    LayoutNet { cell: String, name: String },
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
            Self::Geometry { cell, layer, datatype, layer_name, via } => {
                let layer = match layer_name {
                    Some(n) => n.clone(),
                    None => format!("L{layer}/{datatype}"),
                };
                match via {
                    Some(v) => format!("{cell}:{layer}:{}", v.path.join("/")),
                    None => format!("{cell}:{layer}"),
                }
            }
            Self::Port { cell, name } => format!("{cell}:port:{name}"),
            Self::Device { cell, model, at } => format!("{cell}:{model} @ ({:.3}, {:.3})", at[0], at[1]),
            Self::LayoutNet { cell, name } => format!("{cell}:net:{name}"),
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
    /// Es la ubicación del elemento en el dibujo (posición, giro, espejo),
    /// no un parámetro: las listas de parámetros cambiados la omiten. La
    /// marca el módulo; el núcleo no sabe qué claves son de ubicación en
    /// cada formato.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub placement: bool,
}

impl Detail {
    pub fn new(key: impl Into<String>, before: Option<Value>, after: Option<Value>) -> Self {
        Self { key: key.into(), before, after, placement: false }
    }

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
                layer_name: None,
                via: Some(Via { path: vec!["INV".into()], instances: 2, at: None }),
            },
        )
        .with_detail("added_area_um2", None, Some(1.0.into()));
        let v = serde_json::to_value(&c).unwrap();
        // Sin nombre de capa el JSON es el de siempre.
        assert!(v["element"].get("layer_name").is_none());
        assert_eq!(c.element.name(), "TOP:L1/0:INV");
        assert_eq!(v["kind"], "added");
        assert_eq!(v["element"]["type"], "geometry");
        assert_eq!(v["element"]["via"]["instances"], 2);
        assert_eq!(v["details"][0]["after"], 1.0);
        assert!(v.get("renamed_from").is_none() && v.get("location").is_none());
        let back: Change = serde_json::from_value(v).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn named_layers_and_ports() {
        let g = Element::Geometry { cell: "inv".into(), layer: 7, datatype: 0, layer_name: Some("metal1".into()), via: None };
        assert_eq!(g.name(), "inv:metal1");
        let v = serde_json::to_value(&g).unwrap();
        assert_eq!(v["layer_name"], "metal1");
        // Un JSON viejo, sin layer_name, se sigue leyendo.
        let old: Element = serde_json::from_str(r#"{"type":"geometry","cell":"A","layer":1,"datatype":0}"#).unwrap();
        assert!(matches!(old, Element::Geometry { layer_name: None, .. }));
        let d = Element::Device { cell: "inv".into(), model: "sky130_fd_pr__nfet_01v8".into(), at: [1.2, 0.5] };
        assert_eq!(serde_json::to_value(&d).unwrap()["type"], "device");
        assert_eq!(d.name(), "inv:sky130_fd_pr__nfet_01v8 @ (1.200, 0.500)");
        let n = Element::LayoutNet { cell: "inv".into(), name: "Y".into() };
        assert_eq!((n.name(), serde_json::to_value(&n).unwrap()["type"].as_str()), ("inv:net:Y".to_string(), Some("layout_net")));
        let c = Change::new(ChangeKind::Modified, n).with_severity(Severity::Error);
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["severity"], "error");
        assert!(serde_json::to_value(Change::new(ChangeKind::Added, Element::Whole)).unwrap().get("severity").is_none(), "sin gravedad, el JSON de siempre");
        assert_eq!(serde_json::from_value::<Change>(v).unwrap(), c);
        let p = Element::Port { cell: "inv".into(), name: "A".into() };
        assert_eq!(p.name(), "inv:port:A");
        assert_eq!(serde_json::to_value(&p).unwrap()["type"], "port");
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

    #[test]
    fn failed_is_not_empty() {
        let f = FileChange::failed(FileFormat::Gds, "(A) no es GDSII");
        assert!(!f.is_empty());
        assert_eq!(f.functional().count(), 0);
        let v = serde_json::to_value(&f).unwrap();
        assert_eq!(v["error"], "(A) no es GDSII");
        // Sin error, el JSON no cambia.
        assert!(serde_json::to_value(FileChange::new(FileFormat::Gds)).unwrap().get("error").is_none());
    }
}
