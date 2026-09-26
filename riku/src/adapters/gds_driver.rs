//! Adapter delgado al trait `RikuDriver` que delega el diff GDS en
//! `gds_renderer::diff_gds`. Este crate ya no depende directamente de
//! `gdstk_rs`; toda la lógica vive en `gds-renderer`.

use std::collections::BTreeMap;

use gds_renderer::{
    diff_gds_cached, DiffCache, DiffConfig, GdsError, GdsGeomDiff,
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
    after.insert("origin_path".to_string(), g.origin_path.join("/"));
    after.insert("flattened".to_string(), g.flattened.to_string());
    if g.instances > 0 {
        after.insert("instances".to_string(), g.instances.to_string());
    }
    if let Some((x, y)) = g.instance_at_um {
        after.insert("instance_at_um".to_string(), format!("{x:.3},{y:.3}"));
    }

    // Element extendido: si el cambio nace via reference, el origen
    // se incrusta como sufijo. Asi el reporte text agrupa por (cell,
    // origen) sin colisionar con entries directas de la cell raiz.
    let element = if g.origin_path.len() > 1 {
        let tail = g.origin_path[1..].join("/");
        format!("{}:L{}/{}:{}", g.cell, g.layer.layer, g.layer.datatype, tail)
    } else {
        format!("{}:L{}/{}", g.cell, g.layer.layer, g.layer.datatype)
    };

    DiffEntry {
        kind,
        element,
        before: None,
        after: Some(after),
        cosmetic: g.cosmetic,
        position_changed: false,
    }
}

fn translate_error(e: GdsError, path_hint: &str) -> String {
    match e {
        GdsError::NotGdsii { side } => format!(
            "{path_hint} ({side}): no es un layout GDSII ni OASIS, se omite el diff."
        ),
        GdsError::Parse { side, msg } => format!(
            "{path_hint} ({side}): no se pudo leer el layout: {msg}"
        ),
    }
}

pub struct GdsDriver {
    cached_info: std::sync::OnceLock<DriverInfo>,
    cosmetic_threshold_um2: f64,
    /// Cache en disco del reporte (solo layouts grandes; ver `DiffCache`).
    cache: DiffCache,
}

impl GdsDriver {
    pub fn new() -> Self {
        Self::with_threshold(DEFAULT_COSMETIC_THRESHOLD_UM2)
    }

    /// Constructor con umbral cosmetico custom (µm²) (flag
    /// `--cosmetic-threshold-um2`). Usa la cache segun el entorno.
    pub fn with_threshold(cosmetic_threshold_um2: f64) -> Self {
        Self::with_config(cosmetic_threshold_um2, true)
    }

    /// `use_cache = false` (flag `--no-cache`) desactiva la cache de diffs.
    pub fn with_config(cosmetic_threshold_um2: f64, use_cache: bool) -> Self {
        Self {
            cached_info: std::sync::OnceLock::new(),
            cosmetic_threshold_um2,
            cache: if use_cache { DiffCache::from_env() } else { DiffCache::disabled() },
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
            extensions: vec![".gds".to_string(), ".oas".to_string()],
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
        let r = match diff_gds_cached(content_a, content_b, &cfg, &self.cache) {
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
        // "A → B" con kind Modified: el formateador lo marca como renombre.
        for (from, to) in r.cells_renamed {
            report.changes.push(cell_entry(&format!("{from} → {to}"), ChangeKind::Modified));
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

    fn fixture_bytes(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("gds-renderer")
            .join("tests")
            .join("fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn high_threshold_marks_changes_cosmetic() {
        // Con threshold 200 µm² (mas grande que el cambio del fixture: 100 µm²),
        // el diff debe marcar todos los entries geometricos como cosmetic.
        let a = fixture_bytes("datatype_a.gds");
        let b = fixture_bytes("datatype_b.gds");
        let driver = GdsDriver::with_threshold(200.0);
        let report = driver.diff(&a, &b, "datatype.gds");
        let geom: Vec<&DiffEntry> = report
            .changes
            .iter()
            .filter(|c| c.element.contains(":L"))
            .collect();
        assert!(!geom.is_empty(), "deberia haber entries geom");
        assert!(
            geom.iter().all(|c| c.cosmetic),
            "todos los entries geom deben ser cosmetic con threshold alto"
        );
    }

    #[test]
    fn default_threshold_keeps_real_changes_non_cosmetic() {
        // Con default 0.01 µm², un cambio de 100 µm² NO debe ser cosmetico.
        let a = fixture_bytes("datatype_a.gds");
        let b = fixture_bytes("datatype_b.gds");
        let report = GdsDriver::new().diff(&a, &b, "datatype.gds");
        let geom: Vec<&DiffEntry> = report
            .changes
            .iter()
            .filter(|c| c.element.contains(":L"))
            .collect();
        assert!(!geom.is_empty());
        assert!(
            geom.iter().any(|c| !c.cosmetic),
            "al menos un entry geom no deberia ser cosmetic con threshold bajo"
        );
    }

    #[test]
    fn can_handle_gds_extension() {
        let d = GdsDriver::new();
        assert!(d.can_handle("foo.gds"));
        assert!(d.can_handle("path/to/Bar.GDS"));
        assert!(!d.can_handle("foo.sch"));
    }

    fn renderer_fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../gds-renderer/tests/fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn renamed_cell_is_one_rename_entry() {
        let r = GdsDriver::new().diff(
            &renderer_fixture("rename_a.gds"),
            &renderer_fixture("rename_b.gds"),
            "rename.gds",
        );
        let rename: Vec<_> = r.changes.iter().filter(|c| c.element.contains(" → ")).collect();
        assert_eq!(rename.len(), 1);
        assert_eq!(rename[0].element, "cell:INV → INV_X1");
        assert_eq!(rename[0].kind, ChangeKind::Modified);
        assert!(!r.changes.iter().any(|c| c.element == "cell:INV" || c.element == "cell:INV_X1"));
    }

    #[test]
    fn instances_of_the_same_subcell_are_grouped() {
        let r = GdsDriver::new().diff(
            &renderer_fixture("multi_inst_a.gds"),
            &renderer_fixture("multi_inst_b.gds"),
            "multi.gds",
        );
        let top = r.changes.iter().find(|c| c.element == "TOP:L1/0:INV").expect("TOP");
        let after = top.after.as_ref().unwrap();
        assert_eq!(after.get("instances").map(String::as_str), Some("2"));
        assert!(!after.contains_key("instance_at_um"));
    }
}
