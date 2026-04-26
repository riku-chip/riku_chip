use crate::core::domain::models::ChangeKind;

/// Colores RGBA para cada tipo de cambio.
/// Separados aquí para que el GUI pueda reutilizarlos sin depender de SVG.
pub struct AnnotationStyle {
    pub fill: &'static str,
    pub stroke: &'static str,
}

pub fn annotation_style(kind: &ChangeKind, cosmetic: bool) -> AnnotationStyle {
    match (kind, cosmetic) {
        (ChangeKind::Added, _) => AnnotationStyle {
            fill: "rgba(0,200,0,0.25)",
            stroke: "rgba(0,200,0,0.85)",
        },
        (ChangeKind::Removed, _) => AnnotationStyle {
            fill: "rgba(200,0,0,0.25)",
            stroke: "rgba(200,0,0,0.85)",
        },
        (ChangeKind::Modified, true) => AnnotationStyle {
            fill: "rgba(120,120,120,0.20)",
            stroke: "rgba(120,120,120,0.85)",
        },
        (ChangeKind::Modified, false) => AnnotationStyle {
            fill: "rgba(255,180,0,0.25)",
            stroke: "rgba(255,180,0,0.85)",
        },
    }
}
