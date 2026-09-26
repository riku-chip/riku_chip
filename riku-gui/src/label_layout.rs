//! Colocación de etiquetas legibles sobre el lienzo.
//!
//! Estrategia (ver `scene_painter` para el dibujo):
//! 1. **Fusión:** etiquetas con el mismo anclaje en pantalla se unen
//!    (`VPWR · VPB`): en GDS es común que pin y pozo compartan punto.
//! 2. **Desplazamiento:** la pastilla no tapa lo que etiqueta. Un punto marca
//!    el anclaje exacto y el texto va arriba; si choca, prueba abajo,
//!    derecha e izquierda.
//! 3. **Anti-solapamiento voraz:** en el orden recibido (el backend pone
//!    primero pines y al final textos decorativos); lo que no entra se omite
//!    y se cuenta, para avisar al usuario.
//!
//! Todo en coordenadas de pantalla y sin egui::Painter, para testearlo.

use eframe::egui::{Color32, Pos2, Rect, Vec2};

/// Etiqueta a colocar, ya proyectada a pantalla.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelCandidate {
    pub anchor: Pos2,
    pub text: String,
    pub color: Color32,
}

/// Etiqueta colocada: `rect` es la pastilla (texto + relleno interno).
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedLabel {
    pub anchor: Pos2,
    pub rect: Rect,
    pub text: String,
    pub color: Color32,
}

/// Relleno interno de la pastilla alrededor del texto.
pub const PILL_PADDING: Vec2 = Vec2::new(4.0, 1.5);
/// Separación entre el punto de anclaje y la pastilla.
const GAP: f32 = 5.0;
/// Distancia (px) bajo la cual dos anclajes cuentan como el mismo punto.
const SAME_POINT: f32 = 2.0;
/// Margen mínimo entre pastillas.
const MARGIN: f32 = 2.0;

/// Une etiquetas con el mismo anclaje (conserva el orden de la primera
/// aparición y no repite textos iguales).
pub fn merge_coincident(cands: Vec<LabelCandidate>) -> Vec<LabelCandidate> {
    let mut out: Vec<LabelCandidate> = Vec::with_capacity(cands.len());
    for c in cands {
        match out.iter_mut().find(|o| o.anchor.distance(c.anchor) <= SAME_POINT) {
            Some(o) => {
                if !o.text.split(" · ").any(|t| t == c.text) {
                    o.text.push_str(" · ");
                    o.text.push_str(&c.text);
                }
            }
            None => out.push(c),
        }
    }
    out
}

/// Posiciones a probar para una pastilla de tamaño `size`: arriba, abajo,
/// derecha e izquierda del anclaje.
fn slots(anchor: Pos2, size: Vec2) -> [Rect; 4] {
    let (w, h) = (size.x, size.y);
    [
        Rect::from_min_size(Pos2::new(anchor.x - w / 2.0, anchor.y - GAP - h), size),
        Rect::from_min_size(Pos2::new(anchor.x - w / 2.0, anchor.y + GAP), size),
        Rect::from_min_size(Pos2::new(anchor.x + GAP, anchor.y - h / 2.0), size),
        Rect::from_min_size(Pos2::new(anchor.x - GAP - w, anchor.y - h / 2.0), size),
    ]
}

/// Coloca las etiquetas sin solaparse. `measure` da el tamaño del texto en
/// pantalla. Retorna las colocadas y cuántas se omitieron por falta de lugar.
/// Etiquetas con el anclaje fuera de `clip` no cuentan (no están a la vista).
pub fn place(
    cands: Vec<LabelCandidate>,
    measure: impl Fn(&str) -> Vec2,
    clip: Rect,
) -> (Vec<PlacedLabel>, usize) {
    let mut placed: Vec<PlacedLabel> = Vec::new();
    let mut hidden = 0;
    for c in merge_coincident(cands) {
        if !clip.contains(c.anchor) {
            continue;
        }
        let size = measure(&c.text) + PILL_PADDING * 2.0;
        let free = |r: &Rect| placed.iter().all(|p| !p.rect.expand(MARGIN).intersects(*r));
        match slots(c.anchor, size).into_iter().find(free) {
            Some(rect) => placed.push(PlacedLabel { anchor: c.anchor, rect, text: c.text, color: c.color }),
            None => hidden += 1,
        }
    }
    (placed, hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(x: f32, y: f32, text: &str) -> LabelCandidate {
        LabelCandidate { anchor: Pos2::new(x, y), text: text.into(), color: Color32::WHITE }
    }

    /// 7 px por carácter y 12 px de alto: suficiente para razonar en tests.
    fn measure(t: &str) -> Vec2 {
        Vec2::new(7.0 * t.chars().count() as f32, 12.0)
    }

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))
    }

    #[test]
    fn coincident_labels_are_merged_without_duplicates() {
        let m = merge_coincident(vec![
            cand(10.0, 10.0, "VPWR"),
            cand(11.0, 10.5, "VPB"),
            cand(10.0, 10.0, "VPWR"),
            cand(50.0, 10.0, "A"),
        ]);
        let texts: Vec<_> = m.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, vec!["VPWR · VPB", "A"]);
    }

    #[test]
    fn label_goes_above_its_anchor() {
        let (p, hidden) = place(vec![cand(100.0, 100.0, "Y")], measure, screen());
        assert_eq!(hidden, 0);
        assert!(p[0].rect.max.y < 100.0, "la pastilla no tapa el anclaje: {:?}", p[0].rect);
        assert!((p[0].rect.center().x - 100.0).abs() < 1e-3);
    }

    #[test]
    fn colliding_label_tries_other_slots_then_is_hidden() {
        // Tres anclajes casi pegados (pero no el mismo punto): el segundo
        // prueba otra posición; con suficientes vecinos alguno se omite.
        let cands: Vec<_> = (0..6).map(|i| cand(100.0 + 3.0 * i as f32, 100.0, "NET")).collect();
        let (p, hidden) = place(cands, measure, screen());
        assert!(p.len() >= 2 && hidden >= 1, "colocadas {} ocultas {hidden}", p.len());
        for (i, a) in p.iter().enumerate() {
            for b in &p[i + 1..] {
                assert!(!a.rect.intersects(b.rect), "{:?} pisa {:?}", a.rect, b.rect);
            }
        }
    }

    #[test]
    fn earlier_labels_win_the_space() {
        // Mismo lugar disputado: gana el primero (prioridad del backend).
        let (p, _) = place(
            vec![cand(100.0, 100.0, "PIN"), cand(104.0, 100.0, "decorativo")],
            measure,
            screen(),
        );
        assert_eq!(p[0].text, "PIN");
        assert!(p[0].rect.max.y < 100.0, "el pin conserva la posición preferida");
    }

    #[test]
    fn offscreen_anchors_are_ignored_not_counted_as_hidden() {
        let (p, hidden) = place(vec![cand(-50.0, 10.0, "X")], measure, screen());
        assert!(p.is_empty());
        assert_eq!(hidden, 0);
    }
}
