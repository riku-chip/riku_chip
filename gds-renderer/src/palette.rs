use crate::style::{Color, LayerStyle, Pdk};
use gdstk_rs::GdsTag;

pub fn color_for_tag(tag: GdsTag, pdk: Pdk) -> Color {
    let palette = match pdk {
        Pdk::Sky130 => SKY130_PALETTE,
        Pdk::Gf180 => GF180_PALETTE,
        Pdk::Ihp => IHP_PALETTE,
        Pdk::Generic => GENERIC_PALETTE,
    };
    let index = ((tag.layer as usize) * 31 + tag.datatype as usize) % palette.len();
    palette[index]
}

pub fn default_layer_style(tag: GdsTag, order: u32, fill: Color) -> LayerStyle {
    LayerStyle {
        tag,
        name: format!("layer_{}_{}", tag.layer, tag.datatype),
        fill,
        stroke: fill,
        opacity: 1.0,
        visible: true,
        order,
        hatch: None,
    }
}

pub fn highlight_style(kind: &str) -> (Color, Color) {
    match kind {
        "added" => (Color::rgba(0, 200, 0, 255), Color::rgba(0, 120, 0, 255)),
        "removed" => (Color::rgba(200, 0, 0, 255), Color::rgba(120, 0, 0, 255)),
        "modified" => (Color::rgba(255, 180, 0, 255), Color::rgba(180, 120, 0, 255)),
        _ => (Color::rgba(255, 0, 0, 255), Color::rgba(180, 0, 0, 255)),
    }
}

const GENERIC_PALETTE: [Color; 12] = [
    Color::rgba(76, 175, 80, 255),
    Color::rgba(33, 150, 243, 255),
    Color::rgba(255, 193, 7, 255),
    Color::rgba(244, 67, 54, 255),
    Color::rgba(0, 188, 212, 255),
    Color::rgba(156, 39, 176, 255),
    Color::rgba(255, 152, 0, 255),
    Color::rgba(63, 81, 181, 255),
    Color::rgba(205, 220, 57, 255),
    Color::rgba(121, 85, 72, 255),
    Color::rgba(96, 125, 139, 255),
    Color::rgba(233, 30, 99, 255),
];

const SKY130_PALETTE: [Color; 12] = GENERIC_PALETTE;
const GF180_PALETTE: [Color; 12] = GENERIC_PALETTE;
const IHP_PALETTE: [Color; 12] = GENERIC_PALETTE;
