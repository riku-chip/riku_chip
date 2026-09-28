//! Leyenda del lienzo: las capas de lo que se ve, con su color, en la
//! esquina inferior izquierda. Pasar el puntero por una la resalta (como en
//! el panel Capas) y un clic la deja resaltada.

use std::collections::HashSet;

use eframe::egui::{self, Align2, RichText};
use viewer_core::element::Layer;
use viewer_core::paint::LayerPaint;

use crate::gui::content::SceneState;
use crate::gui::scene_painter::{to_color32, PaintStats};
use crate::gui::theme::CanvasTheme;
use crate::gui::tr;

/// Filas como máximo; el resto va en "y N más".
const MAX_ROWS: usize = 10;
/// Separación de los bordes del lienzo.
const MARGIN: f32 = 10.0;

/// Las capas de la leyenda, en el orden del panel Capas (el de apilado), y
/// cuántas quedaron afuera.
fn rows<'a>(layers: &[(Layer, &'a LayerPaint)], in_view: &HashSet<Layer>) -> (Vec<&'a LayerPaint>, usize) {
    let all: Vec<&LayerPaint> = layers.iter().filter(|(k, _)| in_view.contains(k)).map(|(_, p)| *p).collect();
    let more = all.len().saturating_sub(MAX_ROWS);
    (all.into_iter().take(MAX_ROWS).collect(), more)
}

/// Dibuja la leyenda sobre `canvas` (si hay capas a la vista) y atiende el
/// puntero y los clics.
pub(crate) fn show(ui: &egui::Ui, canvas: egui::Rect, bs: &mut SceneState, stats: &PaintStats) {
    bs.layer_hover_legend = None;
    let scene = bs.scene.clone();
    let layers = scene.layer_list();
    let (shown, more) = rows(&layers, &stats.layers_in_view);
    if shown.is_empty() {
        return;
    }
    let theme = CanvasTheme::from_visuals(ui.visuals());
    let title = if stats.layers_approx { tr!("legend.title_all") } else { tr!("legend.title") };
    let frame = egui::Frame::new()
        .fill(theme.background.gamma_multiply(0.88))
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(8, 6));
    egui::Area::new(egui::Id::new("riku_legend"))
        .order(egui::Order::Middle)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(canvas.left_bottom() + egui::vec2(MARGIN, -MARGIN))
        .constrain_to(canvas)
        .show(ui.ctx(), |ui| {
            frame.show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(title).small().weak()).on_hover_text(tr!("legend.hint"));
                for paint in shown {
                    let row = ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        let (fill, stroke) = theme.layer_colors(to_color32(paint.fill), to_color32(paint.stroke));
                        ui.painter().rect(rect, 2.0, fill, egui::Stroke::new(1.5_f32, stroke), egui::StrokeKind::Inside);
                        let pinned = bs.layer_focus.as_deref() == Some(paint.name.as_str());
                        if ui.selectable_label(pinned, RichText::new(&paint.name).small()).clicked() {
                            bs.toggle_layer_focus(&paint.name);
                        }
                    });
                    if row.response.contains_pointer() {
                        bs.layer_hover_legend = Some(paint.name.clone());
                    }
                }
                if more > 0 {
                    ui.label(RichText::new(tr!("legend.more", count = more)).small().weak());
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use viewer_core::paint::Rgba;

    fn paint(name: &str) -> LayerPaint {
        let c = Rgba { r: 1, g: 2, b: 3, a: 255 };
        LayerPaint { name: name.into(), fill: c, stroke: c }
    }

    #[test]
    fn keeps_the_stacking_order_and_caps_the_rows() {
        let paints: Vec<LayerPaint> = (0..14).map(|i| paint(&format!("L{i}"))).collect();
        let layers: Vec<(Layer, &LayerPaint)> = paints.iter().enumerate().map(|(i, p)| (i as Layer, p)).collect();
        // A la vista, en otro orden: la leyenda sigue el de apilado.
        let few: HashSet<Layer> = [5, 1, 3].into_iter().collect();
        let (shown, more) = rows(&layers, &few);
        assert_eq!(shown.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["L1", "L3", "L5"]);
        assert_eq!(more, 0);
        let all: HashSet<Layer> = (0..14).collect();
        let (shown, more) = rows(&layers, &all);
        assert_eq!((shown.len(), more), (MAX_ROWS, 4));
        assert!(rows(&layers, &HashSet::new()).0.is_empty(), "sin capas a la vista, sin leyenda");
    }
}
