use std::collections::{HashMap, HashSet};

use gdstk_rs::{BoundingBox, GdsTag};

use crate::palette::{color_for_tag, default_layer_style};
use crate::scene::{DrawCommand, HighlightSet, RenderScene};
use crate::style::{Color, LayerInfo, LayerStyle, Pdk};
use crate::viewport::expanded_bbox;

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedScene {
    pub viewport: crate::viewport::Viewport,
    pub styles: Vec<LayerStyle>,
    pub commands: Vec<DrawCommand>,
    pub highlights: HighlightSet,
    pub show_labels: bool,
    pub background: Option<Color>,
    pub include_layer_metadata: bool,
    pub bbox: BoundingBox,
    pub layer_infos: Vec<LayerInfo>,
    pub visible_layers: Vec<GdsTag>,
}

pub fn resolve_scene(scene: &RenderScene) -> ResolvedScene {
    let styles = resolve_styles(scene);
    let bbox = resolve_bbox(scene);
    let (layer_infos, visible_layers) = resolve_layer_metadata(scene, &styles);

    ResolvedScene {
        viewport: scene.viewport.clone(),
        styles,
        commands: scene.commands.clone(),
        highlights: scene.highlights.clone(),
        show_labels: scene.show_labels,
        background: scene.background,
        include_layer_metadata: scene.include_layer_metadata,
        bbox,
        layer_infos,
        visible_layers,
    }
}

fn resolve_styles(scene: &RenderScene) -> Vec<LayerStyle> {
    let mut style_map: HashMap<GdsTag, LayerStyle> = HashMap::new();
    for style in &scene.catalog.layers {
        style_map.insert(style.tag, style.clone());
    }

    let observed_tags = observed_tags(scene);
    let mut next_order = style_map.values().map(|style| style.order).max().unwrap_or(0) + 1;
    for tag in observed_tags {
        style_map.entry(tag).or_insert_with(|| {
            let fill = color_for_tag(tag, Pdk::Generic);
            let style = default_layer_style(tag, next_order, fill);
            next_order += 1;
            style
        });
    }

    let mut styles: Vec<LayerStyle> = style_map.into_values().collect();
    styles.sort_by_key(|style| (style.order, style.tag.layer, style.tag.datatype));
    styles
}

fn resolve_bbox(scene: &RenderScene) -> BoundingBox {
    let highlight_polygons: Vec<_> = scene
        .highlights
        .added
        .iter()
        .chain(scene.highlights.removed.iter())
        .chain(scene.highlights.modified.iter())
        .cloned()
        .collect();
    expanded_bbox(scene.viewport.effective_box(), &highlight_polygons)
}

fn resolve_layer_metadata(scene: &RenderScene, styles: &[LayerStyle]) -> (Vec<LayerInfo>, Vec<GdsTag>) {
    let mut layer_infos = Vec::new();
    let mut visible_layers = Vec::new();

    if scene.include_layer_metadata {
        for style in styles {
            layer_infos.push(LayerInfo {
                tag: style.tag,
                name: style.name.clone(),
                color: style.fill,
                visible: style.visible,
                order: style.order,
            });
        }
    }

    for style in styles {
        if style.visible {
            visible_layers.push(style.tag);
        }
    }

    (layer_infos, visible_layers)
}

fn observed_tags(scene: &RenderScene) -> HashSet<GdsTag> {
    let mut tags = HashSet::new();
    for command in &scene.commands {
        tags.insert(command.tag());
    }
    tags
}
