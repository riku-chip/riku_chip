mod compat;
mod composition;
mod output;
mod palette;
mod renderer;
mod scene;
mod style;
mod viewport;

pub use compat::{render_cell, render_cell_with_highlights, scene_from_cell};
pub use output::RenderOutput;
pub use renderer::{render_scene, render_scene_with_highlights};
pub use scene::{DrawCommand, HighlightSet, OwnedPolygon, RenderPlane, RenderScene};
pub use style::{Color, HatchPattern, LayerCatalog, LayerInfo, LayerStyle, Pdk, RenderConfig};
pub use viewport::Viewport;
