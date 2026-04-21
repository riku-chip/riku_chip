use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};
use gds_renderer::{DrawCommand, HighlightSet, LayerStyle, RenderScene};

fn color_to_egui(color: gds_renderer::Color) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

fn world_to_screen(rect: Rect, bbox: gdstk_rs::BoundingBox, world: (f64, f64)) -> Pos2 {
    let width = (bbox.max_x - bbox.min_x).abs().max(1.0);
    let height = (bbox.max_y - bbox.min_y).abs().max(1.0);
    let scale_x = rect.width() / width as f32;
    let scale_y = rect.height() / height as f32;
    let scale = scale_x.min(scale_y);
    let scaled_w = width as f32 * scale;
    let scaled_h = height as f32 * scale;
    let origin_x = rect.center().x - scaled_w * 0.5;
    let origin_y = rect.center().y - scaled_h * 0.5;
    let x = origin_x + ((world.0 - bbox.min_x) as f32 * scale);
    let y = origin_y + ((bbox.max_y - world.1) as f32 * scale);
    Pos2::new(x, y)
}

fn polygon_points(
    rect: Rect,
    bbox: gdstk_rs::BoundingBox,
    points: &[gdstk_rs::Point2D],
) -> Vec<Pos2> {
    points
        .iter()
        .map(|point| world_to_screen(rect, bbox, (point.x, point.y)))
        .collect()
}

pub fn paint_scene(ui: &mut egui::Ui, scene: &RenderScene) {
    let available = ui.available_size_before_wrap();
    let (response, painter) = ui.allocate_painter(available, egui::Sense::hover());
    let rect = response.rect;

    if let Some(background) = scene.background {
        painter.rect_filled(rect, 0.0, color_to_egui(background));
    } else {
        painter.rect_filled(rect, 0.0, Color32::from_rgb(20, 20, 24));
    }

    if scene.catalog.layers.is_empty() || scene.commands.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Open a GDS file to render it here.",
            egui::FontId::proportional(18.0),
            Color32::from_gray(180),
        );
        return;
    }

    let viewport = scene.viewport.effective_box();
    render_layers(&painter, rect, viewport, scene);
    render_highlights(&painter, rect, viewport, &scene.highlights);
}

fn render_layers(
    painter: &egui::Painter,
    rect: Rect,
    bbox: gdstk_rs::BoundingBox,
    scene: &RenderScene,
) {
    let mut styles: Vec<&LayerStyle> = scene.catalog.layers.iter().collect();
    styles.sort_by_key(|style| (style.order, style.tag.layer, style.tag.datatype));

    for style in styles {
        if !style.visible {
            continue;
        }

        let fill = color_to_egui(style.fill);
        let stroke = Stroke::new(1.0, color_to_egui(style.stroke));
        for command in scene
            .commands
            .iter()
            .filter(|command| command.tag() == style.tag)
        {
            match command {
                DrawCommand::Polygon { points, .. } => {
                    let points = polygon_points(rect, bbox, points);
                    if points.len() >= 3 {
                        painter.add(Shape::convex_polygon(points, fill, stroke));
                    }
                }
                DrawCommand::Path { points, closed, .. } => {
                    let points = polygon_points(rect, bbox, points);
                    if *closed && points.len() >= 3 {
                        painter.add(Shape::convex_polygon(points, fill, stroke));
                    } else if points.len() >= 2 {
                        painter.add(Shape::line(points, stroke));
                    }
                }
                DrawCommand::Rect {
                    bbox: rect_bbox, ..
                } => {
                    let min = world_to_screen(rect, rect_bbox, (rect_bbox.min_x, rect_bbox.max_y));
                    let max = world_to_screen(rect, rect_bbox, (rect_bbox.max_x, rect_bbox.min_y));
                    let rect = Rect::from_min_max(min, max);
                    painter.rect_filled(rect, 0.0, fill);
                    painter.line_segment([rect.left_top(), rect.right_top()], stroke);
                    painter.line_segment([rect.right_top(), rect.right_bottom()], stroke);
                    painter.line_segment([rect.right_bottom(), rect.left_bottom()], stroke);
                    painter.line_segment([rect.left_bottom(), rect.left_top()], stroke);
                }
                DrawCommand::Label { text, origin, .. } => {
                    if scene.show_labels {
                        let pos = world_to_screen(rect, bbox, (origin.x, origin.y));
                        painter.text(
                            pos,
                            egui::Align2::CENTER_CENTER,
                            text,
                            egui::FontId::monospace(12.0),
                            stroke.color,
                        );
                    }
                }
            }
        }
    }
}

fn render_highlights(
    painter: &egui::Painter,
    rect: Rect,
    bbox: gdstk_rs::BoundingBox,
    highlights: &HighlightSet,
) {
    let styles = [
        (
            &highlights.added,
            Color32::from_rgba_unmultiplied(0, 200, 0, 80),
            Color32::from_rgba_unmultiplied(0, 120, 0, 220),
        ),
        (
            &highlights.removed,
            Color32::from_rgba_unmultiplied(200, 0, 0, 80),
            Color32::from_rgba_unmultiplied(120, 0, 0, 220),
        ),
        (
            &highlights.modified,
            Color32::from_rgba_unmultiplied(255, 180, 0, 80),
            Color32::from_rgba_unmultiplied(180, 120, 0, 220),
        ),
    ];

    for (polygons, fill, stroke) in styles {
        for poly in polygons {
            let points = polygon_points(rect, bbox, &poly.points);
            if points.len() >= 3 {
                painter.add(Shape::convex_polygon(
                    points,
                    fill,
                    Stroke::new(1.5, stroke),
                ));
            }
        }
    }
}
