//! Painter genérico para cualquier `RenderableScene` de viewer-core.
//!
//! No sabe de qué backend viene la escena — dibuja los 5 primitivos neutros
//! (Line/Rect/Circle/Polygon/Text). Las features ricas de cada formato
//! (fantasmas, anotaciones, wires/junctions de Xschem) se quedan en su
//! painter específico (`sch_painter.rs`).

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, StrokeKind};
use viewer_core::{
    bbox::BoundingBox,
    element::{DrawElement, HAlign, VAlign},
    scene::RenderableScene,
    viewport::{world_to_screen, Viewport},
};

/// Paleta neutral mínima por layer. Se usa cuando el backend no provee su
/// propio mapeo — suficiente para inspección genérica.
fn layer_color(layer: u16) -> Color32 {
    match layer {
        0 => Color32::from_rgb(20, 20, 24),       // background
        1 => Color32::from_rgb(180, 180, 200),    // wire/primary
        2 => Color32::from_rgb(120, 120, 140),    // grid
        3 => Color32::from_rgb(220, 220, 160),    // text
        4 => Color32::from_rgb(120, 200, 255),    // pin
        5 => Color32::from_rgb(180, 200, 255),    // label
        6 => Color32::from_rgb(220, 180, 120),    // component
        _ => {
            // Hash simple para layers desconocidos (GDS puede tener muchos).
            let r = ((layer.wrapping_mul(131)) & 0xFF) as u8;
            let g = ((layer.wrapping_mul(197)) & 0xFF) as u8;
            let b = ((layer.wrapping_mul(263)) & 0xFF) as u8;
            Color32::from_rgb(r.saturating_add(80), g.saturating_add(80), b.saturating_add(80))
        }
    }
}

fn viewport_world_bbox(vp: &Viewport, rect: Rect) -> BoundingBox {
    // Desproyectamos las 4 esquinas del viewport de pantalla para conocer
    // qué región del mundo es visible. El painter la usa como bbox de culling.
    let inv = |sx: f32, sy: f32| -> (f64, f64) {
        ((sx as f64 - vp.pan_x) / vp.scale, (sy as f64 - vp.pan_y) / vp.scale)
    };
    let p1 = inv(rect.min.x, rect.min.y);
    let p2 = inv(rect.max.x, rect.max.y);
    BoundingBox {
        min_x: p1.0.min(p2.0),
        min_y: p1.1.min(p2.1),
        max_x: p1.0.max(p2.0),
        max_y: p1.1.max(p2.1),
    }
}

pub fn paint_scene(ui: &mut egui::Ui, scene: &dyn RenderableScene, vp: &Viewport) {
    let available = ui.available_size_before_wrap();
    let (response, painter) = ui.allocate_painter(available, egui::Sense::hover());
    let rect = response.rect;

    painter.rect_filled(rect, 0.0, layer_color(0));

    if scene.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Escena vacía.",
            FontId::proportional(16.0),
            Color32::from_gray(160),
        );
        return;
    }

    let world_bbox = viewport_world_bbox(vp, rect);

    let mut visitor = |el: &DrawElement| -> bool {
        draw_element(&painter, vp, rect, el);
        true
    };
    scene.visit(&world_bbox, &mut visitor);
}

fn draw_element(painter: &egui::Painter, vp: &Viewport, clip: Rect, el: &DrawElement) {
    let to_screen = |x: f64, y: f64| -> Pos2 {
        let (sx, sy) = world_to_screen(vp, x, y);
        Pos2::new(sx as f32, sy as f32)
    };

    match el {
        DrawElement::Line { x1, y1, x2, y2, layer } => {
            let a = to_screen(*x1, *y1);
            let b = to_screen(*x2, *y2);
            painter.line_segment([a, b], Stroke::new(1.0, layer_color(*layer)));
        }
        DrawElement::Rect { x, y, w, h, layer, filled } => {
            let min = to_screen(*x, *y);
            let max = to_screen(*x + *w, *y + *h);
            let r = Rect::from_two_pos(min, max);
            let color = layer_color(*layer);
            if *filled {
                painter.rect_filled(r, 0.0, color);
            } else {
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, color), StrokeKind::Middle);
            }
        }
        DrawElement::Circle { cx, cy, r, layer, filled } => {
            let center = to_screen(*cx, *cy);
            let radius_screen = (*r * vp.scale) as f32;
            let color = layer_color(*layer);
            if *filled {
                painter.circle_filled(center, radius_screen, color);
            } else {
                painter.circle_stroke(center, radius_screen, Stroke::new(1.0, color));
            }
        }
        DrawElement::Polygon { points, layer, filled } => {
            if points.len() < 2 { return; }
            let pts: Vec<Pos2> = points.iter().map(|(x, y)| to_screen(*x, *y)).collect();
            let color = layer_color(*layer);
            let stroke = Stroke::new(1.0, color);
            if *filled && pts.len() >= 3 {
                painter.add(Shape::convex_polygon(pts, color, stroke));
            } else {
                painter.add(Shape::line(pts, stroke));
            }
        }
        DrawElement::Text { x, y, content, size, angle_deg, h_align, v_align, layer } => {
            let pos = to_screen(*x, *y);
            if !clip.expand(50.0).contains(pos) { return; }
            let align = to_egui_align(*h_align, *v_align);
            let font_size = (*size * vp.scale) as f32;
            if font_size < 4.0 { return; }
            if angle_deg.abs() < 0.1 {
                painter.text(pos, align, content, FontId::proportional(font_size), layer_color(*layer));
            } else {
                // Para texto rotado usamos galley + rotación; fallback visual
                // sin romper — una primera iteración no necesita perfect kerning.
                let galley = painter.layout_no_wrap(
                    content.clone(),
                    FontId::proportional(font_size),
                    layer_color(*layer),
                );
                let shape = egui::epaint::TextShape::new(pos, galley, layer_color(*layer))
                    .with_angle(angle_deg.to_radians() as f32);
                painter.add(Shape::Text(shape));
            }
        }
    }
}

fn to_egui_align(h: HAlign, v: VAlign) -> Align2 {
    use egui::Align;
    let ha = match h {
        HAlign::Start => Align::LEFT,
        HAlign::Middle => Align::Center,
        HAlign::End => Align::RIGHT,
    };
    let va = match v {
        VAlign::Top => Align::TOP,
        VAlign::Middle => Align::Center,
        VAlign::Bottom => Align::BOTTOM,
    };
    Align2([ha, va])
}
