//! Painter genérico para cualquier `RenderableScene` de viewer-core.
//!
//! No sabe de qué backend viene la escena — dibuja los 5 primitivos neutros
//! (Line/Rect/Circle/Polygon/Text). Las features ricas de cada formato
//! (fantasmas, anotaciones, wires/junctions de Xschem) se quedan en su
//! painter específico (`sch_painter.rs`).
//!
//! Sistemas de coordenadas:
//! - **mundo**: el de la escena; Y-up o Y-down según `scene.y_axis()`.
//! - **vista**: Y-down, relativo a la esquina del panel. Es donde vive el
//!   `Viewport` (pan/zoom), de modo que `fit_to(w, h)` centra en el panel.
//! - **pantalla**: vista + `rect.min` del panel dentro de la ventana.
//!
//! `ScreenXform` concentra las tres conversiones para que dibujo, culling,
//! auto-fit y zoom usen exactamente la misma transformación.

use std::collections::HashSet;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, StrokeKind};
use viewer_core::{
    bbox::BoundingBox,
    element::{DrawElement, HAlign, Layer, VAlign},
    paint::Rgba,
    scene::RenderableScene,
    viewport::{screen_to_world, world_to_screen, Viewport, YAxis},
};

use crate::polygon_fill::paint_filled_polygon;

/// Fondo del lienzo. Independiente de la paleta de capas: la capa 0 es una
/// capa real en GDS y no debe confundirse con el fondo.
const BACKGROUND: Color32 = Color32::from_rgb(20, 20, 24);

/// Paleta neutral mínima por layer. Se usa cuando la escena no provee su
/// propio `LayerPaint` — suficiente para inspección genérica.
fn neutral_layer_color(layer: Layer) -> Color32 {
    match layer {
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

pub fn to_color32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Colores de una capa: `(relleno, contorno)`. Usa el `LayerPaint` de la
/// escena si existe; si no, la paleta neutral opaca.
fn layer_colors(scene: &dyn RenderableScene, layer: Layer) -> (Color32, Color32) {
    match scene.layer_paint(layer) {
        Some(p) => (to_color32(p.fill), to_color32(p.stroke)),
        None => {
            let c = neutral_layer_color(layer);
            (c, c)
        }
    }
}

/// Transformación mundo ↔ pantalla de una escena pintada dentro de `rect`.
#[derive(Clone, Copy)]
pub struct ScreenXform {
    rect: Rect,
    vp: Viewport,
    y_axis: YAxis,
}

impl ScreenXform {
    pub fn new(rect: Rect, vp: &Viewport, y_axis: YAxis) -> Self {
        Self { rect, vp: *vp, y_axis }
    }

    pub fn to_screen(&self, x: f64, y: f64) -> Pos2 {
        let (vx, vy) = world_to_screen(&self.vp, x, self.y_axis.flip_y(y));
        Pos2::new(self.rect.min.x + vx as f32, self.rect.min.y + vy as f32)
    }

    /// Región del mundo visible en `rect`, usada como bbox de culling.
    pub fn visible_world_bbox(&self) -> BoundingBox {
        let (x1, y1) = screen_to_world(&self.vp, 0.0, 0.0);
        let (x2, y2) = screen_to_world(&self.vp, self.rect.width() as f64, self.rect.height() as f64);
        self.y_axis.flip_bbox(&BoundingBox::from_points((x1, y1), (x2, y2)))
    }
}

/// Ajusta `vp` para que toda la escena quepa centrada en `rect`.
pub fn fit_scene(vp: &mut Viewport, scene: &dyn RenderableScene, rect: Rect) {
    let view_bbox = scene.y_axis().flip_bbox(&scene.bbox());
    vp.fit_to(&view_bbox, rect.width() as f64, rect.height() as f64);
}

/// Zoom que mantiene fijo el punto de mundo bajo `pos` (pantalla absoluta).
pub fn zoom_at_screen(vp: &mut Viewport, factor: f64, pos: Pos2, rect: Rect) {
    vp.zoom_at(factor, (pos.x - rect.min.x) as f64, (pos.y - rect.min.y) as f64);
}

/// Pinta `scene` en todo el espacio disponible. Las capas en `hidden` se omiten.
pub fn paint_scene(
    ui: &mut egui::Ui,
    scene: &dyn RenderableScene,
    vp: &Viewport,
    hidden: &HashSet<Layer>,
) {
    let available = ui.available_size_before_wrap();
    let (response, painter) = ui.allocate_painter(available, egui::Sense::hover());
    let rect = response.rect;
    let painter = painter.with_clip_rect(rect);

    painter.rect_filled(rect, 0.0, BACKGROUND);

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

    let xf = ScreenXform::new(rect, vp, scene.y_axis());
    let mut visitor = |el: &DrawElement| -> bool {
        if !hidden.contains(&el.layer()) {
            draw_element(&painter, &xf, vp.scale, scene, el);
        }
        true
    };
    scene.visit(&xf.visible_world_bbox(), &mut visitor);
}

fn draw_element(
    painter: &egui::Painter,
    xf: &ScreenXform,
    scale: f64,
    scene: &dyn RenderableScene,
    el: &DrawElement,
) {
    let (fill, stroke_color) = layer_colors(scene, el.layer());
    let stroke = Stroke::new(1.0_f32, stroke_color);

    match el {
        DrawElement::Line { x1, y1, x2, y2, .. } => {
            painter.line_segment([xf.to_screen(*x1, *y1), xf.to_screen(*x2, *y2)], stroke);
        }
        DrawElement::Rect { x, y, w, h, filled, .. } => {
            let r = Rect::from_two_pos(xf.to_screen(*x, *y), xf.to_screen(*x + *w, *y + *h));
            if *filled {
                painter.rect(r, 0.0, fill, stroke, StrokeKind::Middle);
            } else {
                painter.rect_stroke(r, 0.0, stroke, StrokeKind::Middle);
            }
        }
        DrawElement::Circle { cx, cy, r, filled, .. } => {
            let center = xf.to_screen(*cx, *cy);
            let radius_screen = (*r * scale) as f32;
            if *filled {
                painter.circle(center, radius_screen, fill, stroke);
            } else {
                painter.circle_stroke(center, radius_screen, stroke);
            }
        }
        DrawElement::Polygon { points, filled, .. } => {
            if points.len() < 2 { return; }
            let pts: Vec<Pos2> = points.iter().map(|(x, y)| xf.to_screen(*x, *y)).collect();
            if *filled && pts.len() >= 3 {
                paint_filled_polygon(painter, points, pts, fill, stroke);
            } else {
                painter.add(Shape::line(pts, stroke));
            }
        }
        DrawElement::Text { x, y, content, size, angle_deg, h_align, v_align, .. } => {
            let pos = xf.to_screen(*x, *y);
            if !painter.clip_rect().expand(50.0).contains(pos) { return; }
            let align = to_egui_align(*h_align, *v_align);
            let font_size = (*size * scale) as f32;
            if font_size < 4.0 { return; }
            if angle_deg.abs() < 0.1 {
                painter.text(pos, align, content, FontId::proportional(font_size), stroke_color);
            } else {
                // Para texto rotado usamos galley + rotación; fallback visual
                // sin romper — una primera iteración no necesita perfect kerning.
                let galley = painter.layout_no_wrap(
                    content.clone(),
                    FontId::proportional(font_size),
                    stroke_color,
                );
                let shape = egui::epaint::TextShape::new(pos, galley, stroke_color)
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

#[cfg(test)]
mod tests {
    use super::*;
    use viewer_core::scene::Scene;

    fn panel() -> Rect {
        // Panel desplazado como el CentralPanel real (a la derecha del árbol).
        Rect::from_min_size(Pos2::new(212.0, 60.0), egui::vec2(800.0, 600.0))
    }

    fn square_scene(y_axis: YAxis) -> Scene {
        let mut s = Scene::new();
        s.y_axis = y_axis;
        s.push(DrawElement::Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0, layer: 1, filled: true });
        s
    }

    #[test]
    fn fit_centers_scene_inside_offset_panel() {
        for axis in [YAxis::Down, YAxis::Up] {
            let scene = square_scene(axis);
            let mut vp = Viewport::default();
            fit_scene(&mut vp, &scene, panel());
            let c = ScreenXform::new(panel(), &vp, axis).to_screen(5.0, 5.0);
            assert!((c - panel().center()).length() < 1e-3, "{axis:?}: {c:?}");
        }
    }

    #[test]
    fn y_up_puts_higher_world_y_higher_on_screen() {
        let scene = square_scene(YAxis::Up);
        let mut vp = Viewport::default();
        fit_scene(&mut vp, &scene, panel());
        let xf = ScreenXform::new(panel(), &vp, YAxis::Up);
        assert!(xf.to_screen(0.0, 10.0).y < xf.to_screen(0.0, 0.0).y);
    }

    #[test]
    fn visible_bbox_covers_fitted_scene() {
        for axis in [YAxis::Down, YAxis::Up] {
            let scene = square_scene(axis);
            let mut vp = Viewport::default();
            fit_scene(&mut vp, &scene, panel());
            let vis = ScreenXform::new(panel(), &vp, axis).visible_world_bbox();
            assert!(vis.contains(0.0, 0.0) && vis.contains(10.0, 10.0), "{axis:?}: {vis:?}");
        }
    }

    #[test]
    fn zoom_keeps_point_under_cursor() {
        let scene = square_scene(YAxis::Up);
        let mut vp = Viewport::default();
        fit_scene(&mut vp, &scene, panel());
        let cursor = ScreenXform::new(panel(), &vp, YAxis::Up).to_screen(2.0, 8.0);
        zoom_at_screen(&mut vp, 2.0, cursor, panel());
        let after = ScreenXform::new(panel(), &vp, YAxis::Up).to_screen(2.0, 8.0);
        assert!((after - cursor).length() < 1e-3);
    }

    #[test]
    fn scene_paint_overrides_neutral_palette() {
        let mut scene = square_scene(YAxis::Up);
        scene.layers.insert(1, viewer_core::paint::LayerPaint {
            name: "met1 68/20".into(),
            fill: Rgba::new(60, 130, 240, 90),
            stroke: Rgba::new(60, 130, 240, 255),
        });
        let (fill, stroke) = layer_colors(&scene, 1);
        assert!(fill.a() < 255 && stroke.a() == 255);
        let (nf, _) = layer_colors(&scene, 7);
        assert_eq!(nf, neutral_layer_color(7));
    }
}
