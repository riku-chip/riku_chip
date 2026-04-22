use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, StrokeKind};
use xschem_viewer::ResolvedScene;
use riku::core::models::{ChangeKind, DiffReport};

// ─── Viewport ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SchViewport {
    pub pan_x: f64,
    pub pan_y: f64,
    pub scale: f64,
}

impl Default for SchViewport {
    fn default() -> Self {
        Self { pan_x: 0.0, pan_y: 0.0, scale: 1.0 }
    }
}

impl SchViewport {
    pub fn fit_to(&mut self, scene: &ResolvedScene, rect: Rect) {
        let bbox = &scene.bbox;
        if bbox.is_empty() { return; }
        let w = bbox.width().max(1.0);
        let h = bbox.height().max(1.0);
        let scale_x = rect.width() as f64 / w;
        let scale_y = rect.height() as f64 / h;
        self.scale = (scale_x.min(scale_y) * 0.9).max(0.001);
        self.pan_x = bbox.min_x + w / 2.0;
        self.pan_y = bbox.min_y + h / 2.0;
    }
}

// ─── Coordinate transform ─────────────────────────────────────────────────────

fn world_to_screen(vp: &SchViewport, rect: Rect, wx: f64, wy: f64) -> Pos2 {
    let cx = rect.center().x as f64;
    let cy = rect.center().y as f64;
    let sx = cx + (wx - vp.pan_x) * vp.scale;
    // Y invertido: xschem crece hacia abajo, egui también — pero el origin xschem
    // puede estar negativo, así que simplemente escalamos sin invertir.
    let sy = cy + (wy - vp.pan_y) * vp.scale;
    Pos2::new(sx as f32, sy as f32)
}

fn scale_f(vp: &SchViewport) -> f32 {
    vp.scale as f32
}

// ─── Layer colors ─────────────────────────────────────────────────────────────

fn layer_color(layer: i32) -> Color32 {
    match layer {
        1 => Color32::from_rgb(100, 180, 255),  // wires
        2 => Color32::from_rgb(200, 200, 100),  // components
        3 => Color32::from_rgb(180, 180, 180),  // text
        4 => Color32::from_rgb(100, 220, 100),  // pins
        _ => Color32::from_rgb(160, 160, 160),
    }
}

// ─── Main paint function ──────────────────────────────────────────────────────

/// Pinta el schematic con anotaciones de diff superpuestas.
/// `diff` puede ser None para mostrar solo el schematic sin anotaciones.
pub fn paint_sch(
    ui: &mut egui::Ui,
    scene: &ResolvedScene,
    vp: &SchViewport,
    diff: Option<&DiffReport>,
) {
    let available = ui.available_size_before_wrap();
    let (_, painter) = ui.allocate_painter(available, egui::Sense::hover());
    let rect = painter.clip_rect();

    painter.rect_filled(rect, 0.0, Color32::from_rgb(18, 18, 22));

    if scene.bbox.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Schematic vacío",
            egui::FontId::proportional(16.0),
            Color32::from_gray(160),
        );
        return;
    }

    // ── Primitivos del schematic ──────────────────────────────────────────────
    for elem in &scene.elements {
        paint_element(&painter, vp, rect, elem);
    }

    // ── Anotaciones de diff ───────────────────────────────────────────────────
    if let Some(report) = diff {
        paint_diff_annotations(&painter, vp, rect, scene, report);
    }
}

fn paint_element(
    painter: &egui::Painter,
    vp: &SchViewport,
    rect: Rect,
    elem: &xschem_viewer::DrawElement,
) {
    use xschem_viewer::DrawElement::*;
    match elem {
        Line { x1, y1, x2, y2, layer, .. } => {
            let a = world_to_screen(vp, rect, *x1, *y1);
            let b = world_to_screen(vp, rect, *x2, *y2);
            painter.line_segment([a, b], Stroke::new(1.0, layer_color(*layer)));
        }
        Rect { x, y, w, h, layer, filled, .. } => {
            let min = world_to_screen(vp, rect, *x, *y);
            let max = world_to_screen(vp, rect, x + w, y + h);
            let r = egui::Rect::from_min_max(min, max);
            let color = layer_color(*layer);
            if *filled {
                painter.rect_filled(r, 0.0, color.gamma_multiply(0.3));
            }
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, color), StrokeKind::Outside);
        }
        Circle { cx, cy, r, layer, .. } => {
            let center = world_to_screen(vp, rect, *cx, *cy);
            let radius = (*r * vp.scale) as f32;
            let color = layer_color(*layer);
            painter.circle_stroke(center, radius, Stroke::new(1.0, color));
        }
        Arc { cx, cy, r, start_angle, sweep_angle, layer, .. } => {
            // egui no tiene arc nativo — aproximamos con líneas
            let color = layer_color(*layer);
            let steps = (sweep_angle.abs() / 5.0).ceil() as usize + 1;
            let mut pts: Vec<Pos2> = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let angle_deg = start_angle + sweep_angle * i as f64 / steps as f64;
                let angle_rad = -angle_deg.to_radians(); // xschem Y invertido
                let wx = cx + r * angle_rad.cos();
                let wy = cy + r * angle_rad.sin();
                pts.push(world_to_screen(vp, rect, wx, wy));
            }
            if pts.len() >= 2 {
                painter.add(Shape::line(pts, Stroke::new(1.0, color)));
            }
        }
        Polygon { points, layer, filled, .. } => {
            let pts: Vec<Pos2> = points.iter()
                .map(|(wx, wy)| world_to_screen(vp, rect, *wx, *wy))
                .collect();
            let color = layer_color(*layer);
            if *filled && pts.len() >= 3 {
                painter.add(Shape::convex_polygon(
                    pts.clone(),
                    color.gamma_multiply(0.3),
                    Stroke::new(1.0, color),
                ));
            } else if pts.len() >= 2 {
                painter.add(Shape::line(pts, Stroke::new(1.0, color)));
            }
        }
        Text { x, y, content, size, layer, .. } => {
            let pos = world_to_screen(vp, rect, *x, *y);
            let font_size = (size * vp.scale).clamp(6.0, 500.0) as f32;
            // xschem text anchor is top-left by default (baseline grows downward)
            painter.text(
                pos,
                egui::Align2::LEFT_TOP,
                content,
                egui::FontId::monospace(font_size),
                layer_color(*layer),
            );
        }
        MissingSymbol { x, y, name, .. } => {
            let pos = world_to_screen(vp, rect, *x, *y);
            let half = (10.0 * scale_f(vp)).max(4.0);
            let r = egui::Rect::from_center_size(pos, egui::vec2(half * 2.0, half * 2.0));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(200, 80, 80)), StrokeKind::Outside);
            painter.text(
                pos,
                egui::Align2::CENTER_CENTER,
                format!("?{}", name.split('/').last().unwrap_or(name)),
                egui::FontId::monospace(10.0),
                Color32::from_rgb(200, 80, 80),
            );
        }
    }
}

// ─── Diff annotations ─────────────────────────────────────────────────────────

fn paint_diff_annotations(
    painter: &egui::Painter,
    vp: &SchViewport,
    rect: Rect,
    scene: &ResolvedScene,
    report: &DiffReport,
) {
    for comp in &report.components {
        let (fill, stroke) = annotation_colors(&comp.kind, comp.cosmetic);

        // Recopilar todos los elementos de esta instancia para calcular su bbox
        let bbox = elements_bbox_for(scene, &comp.name);
        if let Some(b) = bbox {
            let min = world_to_screen(vp, rect, b.0, b.1);
            let max = world_to_screen(vp, rect, b.2, b.3);
            let r = egui::Rect::from_min_max(min, max).expand(4.0);
            painter.rect_filled(r, 2.0, fill);
            painter.rect_stroke(r, 2.0, Stroke::new(1.5, stroke), StrokeKind::Outside);

            // Etiqueta con el nombre
            painter.text(
                r.left_top() + egui::vec2(2.0, -14.0),
                egui::Align2::LEFT_BOTTOM,
                &comp.name,
                egui::FontId::monospace(11.0),
                stroke,
            );
        }
    }
}

fn elements_bbox_for(scene: &ResolvedScene, name: &str) -> Option<(f64, f64, f64, f64)> {
    use xschem_viewer::DrawElement::*;

    // Para renombrados: "R1 → R2" — buscar el nombre del lado B (después del →)
    let lookup = if name.contains(" → ") {
        name.split(" → ").nth(1).unwrap_or(name)
    } else {
        name
    };

    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    let mut found = false;

    for elem in &scene.elements {
        if elem.component_id() != Some(lookup) { continue; }
        found = true;
        match elem {
            Line { x1, y1, x2, y2, .. } => {
                expand(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *x1, *y1);
                expand(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *x2, *y2);
            }
            Rect { x, y, w, h, .. } => {
                expand(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *x, *y);
                expand(&mut min_x, &mut min_y, &mut max_x, &mut max_y, x + w, y + h);
            }
            Circle { cx, cy, r, .. } => {
                expand(&mut min_x, &mut min_y, &mut max_x, &mut max_y, cx - r, cy - r);
                expand(&mut min_x, &mut min_y, &mut max_x, &mut max_y, cx + r, cy + r);
            }
            Arc { cx, cy, r, .. } => {
                expand(&mut min_x, &mut min_y, &mut max_x, &mut max_y, cx - r, cy - r);
                expand(&mut min_x, &mut min_y, &mut max_x, &mut max_y, cx + r, cy + r);
            }
            Polygon { points, .. } => {
                for (x, y) in points {
                    expand(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *x, *y);
                }
            }
            Text { x, y, .. } | MissingSymbol { x, y, .. } => {
                expand(&mut min_x, &mut min_y, &mut max_x, &mut max_y, *x, *y);
            }
        }
    }

    if found { Some((min_x, min_y, max_x, max_y)) } else { None }
}

fn expand(min_x: &mut f64, min_y: &mut f64, max_x: &mut f64, max_y: &mut f64, x: f64, y: f64) {
    *min_x = min_x.min(x);
    *min_y = min_y.min(y);
    *max_x = max_x.max(x);
    *max_y = max_y.max(y);
}

fn annotation_colors(kind: &ChangeKind, cosmetic: bool) -> (Color32, Color32) {
    match (kind, cosmetic) {
        (ChangeKind::Added, _) =>
            (Color32::from_rgba_unmultiplied(0, 200, 0, 50), Color32::from_rgb(0, 200, 0)),
        (ChangeKind::Removed, _) =>
            (Color32::from_rgba_unmultiplied(200, 0, 0, 50), Color32::from_rgb(200, 0, 0)),
        (ChangeKind::Modified, true) =>
            (Color32::from_rgba_unmultiplied(120, 120, 120, 40), Color32::from_gray(160)),
        (ChangeKind::Modified, false) =>
            (Color32::from_rgba_unmultiplied(255, 180, 0, 50), Color32::from_rgb(255, 180, 0)),
    }
}
