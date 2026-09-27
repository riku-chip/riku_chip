//! Módulo de layouts de Riku (GDSII, OASIS y Magic) sobre `gdstk-rs`: diff
//! geométrico con cache, paletas por PDK y el backend del visor.

mod diff_cache;
mod gds_diff;
mod hier_walk;
mod labels;
pub mod mag;
mod palette;
mod prints;
mod palette_generated;
mod magic_layers_generated;
mod scene;
mod style;
mod top_cell;
mod viewer_core_compat;

pub use diff_cache::DiffCache;
pub use gds_diff::{
    changed_cells, diff_cell, diff_cell_as, diff_gds, diff_gds_cached, diff_gds_with_config, diff_layout_sides,
    diff_libraries, is_layout, BBoxUm, CellChange, CellDiff, DiffConfig, GdsDiffReport, GdsError, GdsGeomDiff,
    LayerKey, LayerPolygons, LayoutSide,
    DEFAULT_COSMETIC_THRESHOLD_UM2,
};
pub use labels::{flatten_labels, FlatLabel};
pub use scene::{draw_commands, DrawCommand};
pub use style::{Color, Pdk};
pub use top_cell::select_top_cell;
pub use viewer_core_compat::{list_cells, GdsBackend};
