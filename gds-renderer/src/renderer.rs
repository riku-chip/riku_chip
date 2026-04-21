use std::fmt::Write;

use gdstk_rs::{GdsTag, Point2D};

use crate::composition::resolve_scene;
use crate::output::RenderOutput;
use crate::palette::highlight_style;
use crate::scene::{DrawCommand, HighlightSet, RenderPlane, RenderScene};
use crate::style::Color;
use crate::viewport::svg_viewbox;

pub fn render_scene(scene: &RenderScene) -> RenderOutput {
    let resolved = resolve_scene(scene);
    render_resolved_scene(&resolved)
}

pub fn render_scene_with_highlights(scene: &RenderScene) -> RenderOutput {
    let resolved = resolve_scene(scene);
    render_resolved_scene(&resolved)
}

fn render_resolved_scene(scene: &crate::composition::ResolvedScene) -> RenderOutput {
    let viewbox = svg_viewbox(scene.bbox, scene.viewport.width, scene.viewport.height);
    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{viewbox}" role="img" aria-label="layout-scene">"#,
        w = scene.viewport.width,
        h = scene.viewport.height,
        viewbox = viewbox
    );

    if let Some(bg) = scene.background {
        let _ = write!(
            svg,
            r#"<rect x="0" y="0" width="100%" height="100%" fill="{}"/>"#,
            bg.to_svg_rgba()
        );
    }

    for style in &scene.styles {
        if !style.visible {
            continue;
        }

        let attrs = format!(
            r#" id="layer-{}-{}" data-layer="{}" data-datatype="{}""#,
            style.tag.layer, style.tag.datatype, style.tag.layer, style.tag.datatype
        );
        let _ = write!(svg, r#"<g{}>"#, attrs);

        render_tag_plane(
            &mut svg,
            &scene.commands,
            style.tag,
            RenderPlane::Base,
            style.fill,
            style.stroke,
            style.opacity,
            scene.show_labels,
        );
        render_tag_plane(
            &mut svg,
            &scene.commands,
            style.tag,
            RenderPlane::Labels,
            style.fill,
            style.stroke,
            style.opacity,
            scene.show_labels,
        );

        svg.push_str("</g>");
    }

    render_highlights(&mut svg, &scene.highlights);
    svg.push_str("</svg>");

    RenderOutput {
        svg,
        bbox: scene.bbox,
        layers: scene.layer_infos.clone(),
        visible_layers: scene.visible_layers.clone(),
    }
}

fn render_command(
    svg: &mut String,
    command: &DrawCommand,
    fill: Color,
    stroke: Color,
    opacity: f32,
    show_labels: bool,
) {
    match command {
        DrawCommand::Polygon { points, .. } => {
            let _ = write!(
                svg,
                r#"<polygon points="{}" fill="{}" stroke="{}" fill-opacity="{:.3}" stroke-opacity="{:.3}" stroke-width="0.6"/>"#,
                points_to_svg(points),
                fill.to_svg_rgba(),
                stroke.to_svg_rgba(),
                opacity,
                opacity
            );
        }
        DrawCommand::Path {
            points, closed, ..
        } => {
            if *closed {
                let _ = write!(
                    svg,
                    r#"<polygon points="{}" fill="{}" stroke="{}" fill-opacity="{:.3}" stroke-opacity="{:.3}" stroke-width="0.6"/>"#,
                    points_to_svg(points),
                    fill.to_svg_rgba(),
                    stroke.to_svg_rgba(),
                    opacity,
                    opacity
                );
            } else {
                let _ = write!(
                    svg,
                    r#"<polyline points="{}" fill="none" stroke="{}" stroke-opacity="{:.3}" stroke-width="0.6"/>"#,
                    points_to_svg(points),
                    stroke.to_svg_rgba(),
                    opacity
                );
            }
        }
        DrawCommand::Rect { bbox, .. } => {
            let width = (bbox.max_x - bbox.min_x).abs();
            let height = (bbox.max_y - bbox.min_y).abs();
            let _ = write!(
                svg,
                r#"<rect x="{:.4}" y="{:.4}" width="{:.4}" height="{:.4}" fill="{}" stroke="{}" fill-opacity="{:.3}" stroke-opacity="{:.3}" stroke-width="0.6"/>"#,
                bbox.min_x,
                bbox.min_y,
                width,
                height,
                fill.to_svg_rgba(),
                stroke.to_svg_rgba(),
                opacity,
                opacity
            );
        }
        DrawCommand::Label { text, origin, .. } => {
            if !show_labels {
                return;
            }
            let _ = write!(
                svg,
                r#"<text x="{:.4}" y="{:.4}" fill="{}" font-family="monospace" font-size="3.0" text-anchor="middle">{}</text>"#,
                origin.x,
                origin.y,
                stroke.to_svg_rgba(),
                escape_text(text)
            );
        }
    }
}

fn render_tag_plane(
    svg: &mut String,
    commands: &[DrawCommand],
    tag: GdsTag,
    plane: RenderPlane,
    fill: Color,
    stroke: Color,
    opacity: f32,
    show_labels: bool,
) {
    for command in commands
        .iter()
        .filter(|command| command.tag() == tag && command.plane() == plane)
    {
        render_command(svg, command, fill, stroke, opacity, show_labels);
    }
}

fn render_highlights(svg: &mut String, highlights: &HighlightSet) {
    if highlights.is_empty() {
        return;
    }

    render_highlight_group(
        svg,
        "layer-highlights-added",
        &highlights.added,
        highlight_style("added"),
    );
    render_highlight_group(
        svg,
        "layer-highlights-removed",
        &highlights.removed,
        highlight_style("removed"),
    );
    render_highlight_group(
        svg,
        "layer-highlights-modified",
        &highlights.modified,
        highlight_style("modified"),
    );
}

fn render_highlight_group(
    svg: &mut String,
    group_id: &str,
    polygons: &[crate::scene::OwnedPolygon],
    (fill, stroke): (Color, Color),
) {
    if polygons.is_empty() {
        return;
    }

    let _ = write!(svg, r#"<g id="{}">"#, group_id);
    for poly in polygons {
        let _ = write!(
            svg,
            r#"<polygon points="{}" fill="{}" stroke="{}" stroke-width="0.8"/>"#,
            points_to_svg(&poly.points),
            fill.to_svg_rgba(),
            stroke.to_svg_rgba()
        );
    }
    svg.push_str("</g>");
}

fn points_to_svg(points: &[Point2D]) -> String {
    points
        .iter()
        .map(|p| format!("{:.4},{:.4}", p.x, p.y))
        .collect::<Vec<_>>()
        .join(" ")
}

fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{DrawCommand, HighlightSet, RenderScene};
    use crate::style::{Color, HatchPattern, LayerCatalog, LayerStyle};
    use crate::viewport::Viewport;
    use gdstk_rs::{BoundingBox, GdsTag, Library, Point2D};

    #[test]
    fn renders_scene_layers_and_output_metadata() {
        let scene = RenderScene {
            viewport: Viewport {
                width: 320,
                height: 240,
                world_box: BoundingBox {
                    min_x: 0.0,
                    min_y: 0.0,
                    max_x: 10.0,
                    max_y: 10.0,
                },
                pan_x: 0.0,
                pan_y: 0.0,
                scale: 1.0,
            },
            catalog: LayerCatalog {
                layers: vec![LayerStyle {
                    tag: GdsTag {
                        layer: 1,
                        datatype: 0,
                    },
                    name: "m1".to_string(),
                    fill: Color::rgba(0, 200, 0, 255),
                    stroke: Color::rgba(0, 100, 0, 255),
                    opacity: 1.0,
                    visible: true,
                    order: 1,
                    hatch: Some(HatchPattern::Solid),
                }],
            },
            commands: vec![DrawCommand::Polygon {
                tag: GdsTag {
                    layer: 1,
                    datatype: 0,
                },
                points: vec![
                    Point2D { x: 0.0, y: 0.0 },
                    Point2D { x: 1.0, y: 0.0 },
                    Point2D { x: 1.0, y: 1.0 },
                    Point2D { x: 0.0, y: 1.0 },
                ],
            }],
            highlights: HighlightSet {
                added: vec![],
                removed: vec![],
                modified: vec![],
            },
            show_labels: true,
            background: Some(Color::rgba(18, 18, 24, 255)),
            include_layer_metadata: true,
        };

        let out = render_scene(&scene);
        assert!(out.svg.starts_with("<svg"));
        assert!(out.svg.contains("layer-1-0"));
        assert_eq!(out.layers.len(), 1);
        assert_eq!(out.visible_layers.len(), 1);
    }

    #[test]
    fn wraps_real_gds_fixture() {
        let lib = Library::open("../gdstk/tests/proof_lib.gds");
        let cell = lib.top_level().cells().next().expect("top level cell");
        let out = crate::compat::render_cell(&cell, &crate::style::RenderConfig::default());

        assert!(out.svg.starts_with("<svg"));
        assert!(out.svg.contains("</svg>"));
        assert!(!out.layers.is_empty());
    }
}
