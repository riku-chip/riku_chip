use gdstk_rs::{BoundingBox, GdsTag, Point2D};

use crate::style::{Color, LayerCatalog};
use crate::viewport::Viewport;

#[derive(Clone, Debug, PartialEq)]
pub struct OwnedPolygon {
    pub layer: u32,
    pub datatype: u32,
    pub points: Vec<Point2D>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    Polygon {
        tag: GdsTag,
        points: Vec<Point2D>,
    },
    Path {
        tag: GdsTag,
        points: Vec<Point2D>,
        closed: bool,
    },
    Rect {
        tag: GdsTag,
        bbox: BoundingBox,
    },
    Label {
        tag: GdsTag,
        text: String,
        origin: Point2D,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderPlane {
    Base,
    Labels,
}

impl DrawCommand {
    pub fn tag(&self) -> GdsTag {
        match self {
            Self::Polygon { tag, .. }
            | Self::Path { tag, .. }
            | Self::Rect { tag, .. }
            | Self::Label { tag, .. } => *tag,
        }
    }

    pub fn plane(&self) -> RenderPlane {
        match self {
            Self::Label { .. } => RenderPlane::Labels,
            Self::Polygon { .. } | Self::Path { .. } | Self::Rect { .. } => RenderPlane::Base,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HighlightSet {
    pub added: Vec<OwnedPolygon>,
    pub removed: Vec<OwnedPolygon>,
    pub modified: Vec<OwnedPolygon>,
}

impl HighlightSet {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderScene {
    pub viewport: Viewport,
    pub catalog: LayerCatalog,
    pub commands: Vec<DrawCommand>,
    pub highlights: HighlightSet,
    pub show_labels: bool,
    pub background: Option<Color>,
    pub include_layer_metadata: bool,
}

impl RenderScene {
    pub fn empty(width: u32, height: u32) -> Self {
        Self {
            viewport: Viewport {
                width,
                height,
                world_box: BoundingBox {
                    min_x: 0.0,
                    min_y: 0.0,
                    max_x: 1.0,
                    max_y: 1.0,
                },
                pan_x: 0.0,
                pan_y: 0.0,
                scale: 1.0,
            },
            catalog: LayerCatalog { layers: Vec::new() },
            commands: Vec::new(),
            highlights: HighlightSet {
                added: Vec::new(),
                removed: Vec::new(),
                modified: Vec::new(),
            },
            show_labels: true,
            background: None,
            include_layer_metadata: true,
        }
    }
}
