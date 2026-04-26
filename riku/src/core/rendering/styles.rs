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

#[cfg(test)]
mod tests {
    use super::annotation_style;
    use crate::core::domain::models::ChangeKind;

    #[test]
    fn added_no_es_cosmetico() {
        let s = annotation_style(&ChangeKind::Added, false);
        assert!(s.fill.contains("0,200,0"));
        assert!(s.stroke.contains("0,200,0"));
    }

    #[test]
    fn modified_cosmetico_es_gris() {
        let s = annotation_style(&ChangeKind::Modified, true);
        assert!(s.fill.contains("120,120,120"));
    }

    #[test]
    fn cosmetic_y_semantico_difieren() {
        let cosmetic = annotation_style(&ChangeKind::Modified, true);
        assert_eq!(cosmetic.fill, "rgba(120,120,120,0.20)");
        assert_eq!(cosmetic.stroke, "rgba(120,120,120,0.85)");
        let semantic = annotation_style(&ChangeKind::Modified, false);
        assert_eq!(semantic.fill, "rgba(255,180,0,0.25)");
        assert_eq!(semantic.stroke, "rgba(255,180,0,0.85)");
    }
}
