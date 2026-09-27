//! Módulo de layouts de Riku (GDSII, OASIS y Magic) sobre `gdstk-rs`: diff
//! geométrico con cache, estilo de capas según el PDK y el backend del visor.
//!
//! Lo público es lo que usa `riku`: [`diff_layout_sides`] (el diff), el
//! visor ([`GdsBackend`]) y los tipos del reporte. El resto es interno.

mod box_grid;
mod diff_cache;
mod diff_scene;
mod gds_diff;
mod hier_walk;
mod labels;
mod layer_style;
pub mod mag;
mod magic_layers_generated;
mod palette;
mod palette_generated;
pub mod pdk_tech;
mod prints;
mod process;
mod source;
mod style;
mod top_cell;
mod viewer_core_compat;

pub use diff_cache::DiffCache;
pub use gds_diff::{
    diff_cell, diff_layout_sides, is_layout, BBoxUm, CellChange, CellDiff, DiffConfig, GdsDiffReport, GdsError,
    GdsGeomDiff, LayerKey, LayerPolygons, LayoutSide, DEFAULT_COSMETIC_THRESHOLD_UM2,
};
pub use labels::{flatten_labels, FlatLabel};
pub use viewer_core_compat::GdsBackend;

pub(crate) use top_cell::select_top_cell;
