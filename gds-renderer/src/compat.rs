use std::collections::HashSet;

use crate::palette::{color_for_tag, default_layer_style};
use crate::renderer::render_scene;
use crate::scene::{DrawCommand, HighlightSet, OwnedPolygon, RenderScene};
use crate::style::{LayerCatalog, RenderConfig};
use crate::viewport::Viewport;
use gdstk_rs::GdsTag;

pub fn render_cell(
    cell: &gdstk_rs::Cell<'_>,
    config: &RenderConfig,
) -> crate::output::RenderOutput {
    let scene = scene_from_cell(cell, config);
    render_scene(&scene)
}

pub fn render_cell_with_highlights(
    cell: &gdstk_rs::Cell<'_>,
    highlights: &[OwnedPolygon],
    config: &RenderConfig,
) -> crate::output::RenderOutput {
    let mut scene = scene_from_cell(cell, config);
    scene.highlights = highlight_set_from_polygons(highlights);
    render_scene(&scene)
}

fn scene_from_cell(cell: &gdstk_rs::Cell<'_>, config: &RenderConfig) -> RenderScene {
    let flattened = cell.get_polygons().build();
    let mut commands = Vec::new();
    let mut tags = HashSet::new();

    for polygon in flattened.polygons() {
        let tag = GdsTag {
            layer: polygon.layer(),
            datatype: polygon.datatype(),
        };
        tags.insert(tag);
        commands.push(DrawCommand::Polygon {
            tag,
            points: polygon.points().collect(),
        });
    }

    if config.show_labels {
        for label in cell.labels() {
            let tag = GdsTag {
                layer: label.layer(),
                datatype: label.texttype(),
            };
            tags.insert(tag);
            commands.push(DrawCommand::Label {
                tag,
                text: label.text().into_owned(),
                origin: label.origin(),
            });
        }
    }

    let mut world_box = cell.bbox();
    if !world_box.min_x.is_finite()
        || !world_box.min_y.is_finite()
        || !world_box.max_x.is_finite()
        || !world_box.max_y.is_finite()
    {
        world_box = gdstk_rs::BoundingBox {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 1.0,
            max_y: 1.0,
        };
    }

    let mut styles: Vec<_> = tags
        .into_iter()
        .enumerate()
        .map(|(idx, tag)| {
            let fill = color_for_tag(tag, config.pdk);
            default_layer_style(tag, idx as u32, fill)
        })
        .collect();
    styles.sort_by_key(|style| (style.order, style.tag.layer, style.tag.datatype));

    RenderScene {
        viewport: viewport_from_config(config, world_box),
        catalog: LayerCatalog { layers: styles },
        commands,
        highlights: HighlightSet {
            added: Vec::new(),
            removed: Vec::new(),
            modified: Vec::new(),
        },
        show_labels: config.show_labels,
        background: config.background,
        include_layer_metadata: config.include_layer_metadata,
    }
}

fn highlight_set_from_polygons(polygons: &[OwnedPolygon]) -> HighlightSet {
    HighlightSet {
        added: polygons.to_vec(),
        removed: Vec::new(),
        modified: Vec::new(),
    }
}

fn viewport_from_config(config: &RenderConfig, world_box: gdstk_rs::BoundingBox) -> Viewport {
    Viewport {
        width: config.width,
        height: config.height,
        world_box,
        pan_x: 0.0,
        pan_y: 0.0,
        scale: 1.0,
    }
}
