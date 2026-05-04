//! Adapter delgado al trait `RikuDriver` que delega el diff GDS en
//! `gds_renderer::diff_gds`. Este crate ya no depende directamente de
//! `gdstk_rs`; toda la lógica vive en `gds-renderer`.

use std::collections::BTreeMap;

use gds_renderer::{
    diff_gds_with_config, DiffConfig, GdsError, GdsGeomDiff,
    DEFAULT_COSMETIC_THRESHOLD_UM2,
};

use crate::core::domain::driver::{DiffEntry, DriverDiffReport, DriverInfo, RikuDriver};
use crate::core::domain::models::{ChangeKind, DriverKind, FileFormat};

fn cell_entry(name: &str, kind: ChangeKind) -> DiffEntry {
    DiffEntry {
        kind,
        element: format!("cell:{name}"),
        before: None,
        after: None,
        cosmetic: false,
        position_changed: false,
    }
}

fn geom_entry(g: &GdsGeomDiff) -> DiffEntry {
    let kind = match (g.added_polygons, g.removed_polygons) {
        (a, 0) if a > 0 => ChangeKind::Added,
        (0, r) if r > 0 => ChangeKind::Removed,
        _ => ChangeKind::Modified,
    };
    let mut after = BTreeMap::new();
    after.insert("added_polygons".to_string(), g.added_polygons.to_string());
    after.insert(
        "removed_polygons".to_string(),
        g.removed_polygons.to_string(),
    );
    after.insert(
        "added_area_um2".to_string(),
        format!("{:.3}", g.added_area_um2),
    );
    after.insert(
        "removed_area_um2".to_string(),
        format!("{:.3}", g.removed_area_um2),
    );
    if let Some(b) = g.bbox_um {
        after.insert(
            "bbox_um".to_string(),
            format!(
                "{:.3},{:.3},{:.3},{:.3}",
                b.min_x, b.min_y, b.max_x, b.max_y
            ),
        );
    }
    DiffEntry {
        kind,
        element: format!("{}:L{}/{}", g.cell, g.layer.layer, g.layer.datatype),
        before: None,
        after: Some(after),
        cosmetic: g.cosmetic,
        position_changed: false,
    }
}

fn translate_error(e: GdsError, path_hint: &str) -> String {
    match e {
        GdsError::NotGdsii { side } => format!(
            "{path_hint} ({side}): no es formato GDSII, se omite el diff."
        ),
        GdsError::Parse { side, msg } => format!(
            "{path_hint} ({side}): no se pudo parsear GDSII: {msg}"
        ),
    }
}

pub struct GdsDriver {
    cached_info: std::sync::OnceLock<DriverInfo>,
    cosmetic_threshold_um2: f64,
}

impl GdsDriver {
    pub fn new() -> Self {
        Self::with_threshold(DEFAULT_COSMETIC_THRESHOLD_UM2)
    }

    /// Constructor con umbral cosmetico custom (µm²). Reservado para tests
    /// y futura wiring de flag CLI `--cosmetic-threshold-um2`.
    pub fn with_threshold(cosmetic_threshold_um2: f64) -> Self {
        Self {
            cached_info: std::sync::OnceLock::new(),
            cosmetic_threshold_um2,
        }
    }
}

impl Default for GdsDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl RikuDriver for GdsDriver {
    fn info(&self) -> DriverInfo {
        if let Some(info) = self.cached_info.get() {
            return info.clone();
        }
        let info = DriverInfo {
            name: DriverKind::Gds,
            available: true,
            version: "gds-renderer (gdstk cxx)".to_string(),
            extensions: vec![".gds".to_string()],
        };
        let _ = self.cached_info.set(info.clone());
        info
    }

    fn diff(&self, content_a: &[u8], content_b: &[u8], path_hint: &str) -> DriverDiffReport {
        let mut report = DriverDiffReport {
            file_type: FileFormat::Gds,
            ..Default::default()
        };

        let cfg = DiffConfig {
            cosmetic_threshold_um2: self.cosmetic_threshold_um2,
        };
        let r = match diff_gds_with_config(content_a, content_b, &cfg) {
            Ok(r) => r,
            Err(e) => {
                report.warnings.push(translate_error(e, path_hint));
                return report;
            }
        };

        for n in r.cells_removed {
            report.changes.push(cell_entry(&n, ChangeKind::Removed));
        }
        for n in r.cells_added {
            report.changes.push(cell_entry(&n, ChangeKind::Added));
        }
        for g in &r.geometry {
            report.changes.push(geom_entry(g));
        }
        report.warnings.extend(r.warnings);
        report
    }

    fn normalize(&self, content: &[u8], _path_hint: &str) -> Vec<u8> {
        content.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::format::detect_format;

    fn proof_lib_bytes() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("external")
            .join("gdstk")
            .join("tests")
            .join("proof_lib.gds");
        std::fs::read(&path)
            .unwrap_or_else(|e| panic!("no se pudo leer {}: {e}", path.display()))
    }

    #[test]
    fn detect_returns_gds_for_magic() {
        let bytes = [0x00u8, 0x06, 0x00, 0x02, 0x01, 0x00];
        assert_eq!(detect_format(&bytes), FileFormat::Gds);
    }

    #[test]
    fn diff_warns_on_non_gds_a() {
        let svg = br#"<svg xmlns='http://www.w3.org/2000/svg'></svg>"#;
        let gds = proof_lib_bytes();
        let report = GdsDriver::new().diff(svg, &gds, "x.gds");
        assert!(report.changes.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(
            report.warnings[0].contains("(A)") && report.warnings[0].contains("GDSII"),
            "warning debe identificar lado A y mencionar GDSII: {:?}",
            report.warnings
        );
    }

    #[test]
    fn diff_warns_on_non_gds_b() {
        let svg = br#"<svg xmlns='http://www.w3.org/2000/svg'></svg>"#;
        let gds = proof_lib_bytes();
        let report = GdsDriver::new().diff(&gds, svg, "x.gds");
        assert!(report.changes.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(
            report.warnings[0].contains("(B)") && report.warnings[0].contains("GDSII"),
            "warning debe identificar lado B y mencionar GDSII: {:?}",
            report.warnings
        );
    }

    #[test]
    fn diff_identical_returns_empty() {
        let gds = proof_lib_bytes();
        let report = GdsDriver::new().diff(&gds, &gds, "x.gds");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(
            report.is_empty(),
            "self-vs-self debe ser empty: {:?}",
            report.changes
        );
    }

    #[test]
    fn can_handle_gds_extension() {
        let d = GdsDriver::new();
        assert!(d.can_handle("foo.gds"));
        assert!(d.can_handle("path/to/Bar.GDS"));
        assert!(!d.can_handle("foo.sch"));
    }
}
