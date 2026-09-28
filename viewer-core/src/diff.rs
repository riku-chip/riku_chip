//! Cambios entre dos versiones de un archivo, en términos neutros.
//!
//! Un backend que soporte diff (`ViewerBackend::load_diff`) devuelve una
//! escena que ya trae la geometría resaltada (en capas propias, con su
//! `LayerPaint`) y además la lista de cambios para que la UI los enumere y
//! permita saltar a cada uno.

use crate::bbox::BoundingBox;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

/// Un cambio listable: qué, cuánto y dónde.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeItem {
    pub kind: ChangeKind,
    /// Qué cambió (p.ej. `"met1 68/20"` o `"celda añadida: inv_2"`).
    pub label: String,
    /// Magnitud legible (p.ej. `"+2 / −1 pol · +0.350 / −0.120 µm²"`).
    pub detail: String,
    /// Zona del cambio en coordenadas de mundo de la escena, si se ve en ella.
    pub bbox: Option<BoundingBox>,
    /// Cambio por debajo del umbral de relevancia (ruido de snap, slivers).
    pub cosmetic: bool,
    /// Cambia el circuito (un abierto o un corto de un layout): la UI lo
    /// muestra primero y resaltado.
    pub error: bool,
}

/// Marca visual de un cambio sobre la escena de diff: recuadro alrededor de
/// un elemento o trazos resaltados (una net).
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub kind: ChangeKind,
    /// Cambio sin efecto funcional (mismo significado que en `ChangeItem`).
    pub cosmetic: bool,
    /// El elemento además se movió.
    pub moved: bool,
    /// Texto junto a la marca (nombre del elemento o de la net).
    pub label: String,
    pub shape: AnnotationShape,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationShape {
    /// Recuadro en coordenadas de mundo.
    Box(BoundingBox),
    /// Segmentos `(x1, y1, x2, y2)` en coordenadas de mundo.
    Segments(Vec<(f64, f64, f64, f64)>),
}
