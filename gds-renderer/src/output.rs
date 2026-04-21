use gdstk_rs::{BoundingBox, GdsTag};

use crate::style::LayerInfo;

#[derive(Clone, Debug, PartialEq)]
pub struct RenderOutput {
    pub svg: String,
    pub bbox: BoundingBox,
    pub layers: Vec<LayerInfo>,
    pub visible_layers: Vec<GdsTag>,
}
