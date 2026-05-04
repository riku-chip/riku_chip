mod compat;
mod composition;
mod gds_diff;
mod hier_walk;
mod output;
mod palette;
mod renderer;
mod scene;
mod style;
mod top_cell;
mod viewer_core_compat;
mod viewport;

pub use compat::{render_cell, render_cell_with_highlights, scene_from_cell};
pub use gds_diff::{
    diff_gds, diff_gds_with_config, BBoxUm, DiffConfig, GdsDiffReport, GdsError, GdsGeomDiff,
    LayerKey, DEFAULT_COSMETIC_THRESHOLD_UM2,
};
pub use output::RenderOutput;
pub use renderer::{render_scene, render_scene_with_highlights};
pub use scene::{DrawCommand, HighlightSet, OwnedPolygon, RenderPlane, RenderScene};
pub use style::{Color, HatchPattern, LayerCatalog, LayerInfo, LayerStyle, Pdk, RenderConfig};
pub use top_cell::select_top_cell;
pub use viewer_core_compat::GdsBackend;
pub use viewport::Viewport;
