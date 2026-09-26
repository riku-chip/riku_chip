//! Adapter delgado al trait `RikuDriver` que delega el diff GDS en
//! `gds_renderer::diff_gds`. Este crate ya no depende directamente de
//! `gdstk_rs`; toda la lógica vive en `gds-renderer`.

use gds_renderer::{
    diff_gds_cached, DiffCache, DiffConfig, GdsError, GdsGeomDiff,
    DEFAULT_COSMETIC_THRESHOLD_UM2,
};

use crate::core::domain::driver::{DriverInfo, RikuDriver};
use crate::core::domain::models::{
    Bounds, Change, ChangeKind, DriverKind, Element, FileChange, FileFormat, Via,
};

fn cell_change(name: &str, kind: ChangeKind) -> Change {
    Change::new(kind, Element::Cell { name: name.to_string() })
}

/// Traduce un cambio de geometría de gds-renderer al vocabulario del núcleo:
/// áreas y conteos como números, sub-celda e instancia tipadas.
fn geom_change(g: &GdsGeomDiff) -> Change {
    let kind = match (g.added_polygons, g.removed_polygons) {
        (a, 0) if a > 0 => ChangeKind::Added,
        (0, r) if r > 0 => ChangeKind::Removed,
        _ => ChangeKind::Modified,
    };
    // origin_path = [cell] (geometría propia) o [cell, sub] (vía una instancia).
    let via = (g.origin_path.len() > 1).then(|| Via {
        path: g.origin_path[1..].to_vec(),
        instances: g.instances,
        at: g.instance_at_um.map(|(x, y)| [x, y]),
    });
    let element = Element::Geometry {
        cell: g.cell.clone(),
        layer: g.layer.layer,
        datatype: g.layer.datatype,
        via,
    };
    let mut c = Change::new(kind, element)
        .cosmetic(g.cosmetic)
        .with_detail("added_polygons", None, Some(g.added_polygons.into()))
        .with_detail("removed_polygons", None, Some(g.removed_polygons.into()))
        .with_detail("added_area_um2", None, Some(g.added_area_um2.into()))
        .with_detail("removed_area_um2", None, Some(g.removed_area_um2.into()));
    c.location = g.bbox_um.map(|b| Bounds { min_x: b.min_x, min_y: b.min_y, max_x: b.max_x, max_y: b.max_y });
    c
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

    fn diff(&self, content_a: &[u8], content_b: &[u8], path_hint: &str) -> FileChange {
        let mut report = FileChange::new(FileFormat::Gds);

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
            report.changes.push(cell_change(&n, ChangeKind::Removed));
        }
        for n in r.cells_added {
            report.changes.push(cell_change(&n, ChangeKind::Added));
        }
        for (from, to) in r.cells_renamed {
            let mut c = cell_change(&to, ChangeKind::Renamed);
            c.renamed_from = Some(from);
            report.changes.push(c);
        }
        for g in &r.geometry {
            report.changes.push(geom_change(g));
        }
        report.warnings.extend(r.warnings);
        report
    }

    fn format(&self) -> FileFormat {
        FileFormat::Gds
    }

    fn detect(&self, content: &[u8]) -> bool {
        gds_renderer::is_layout(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::registry::detect_format;

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
        let geom: Vec<&Change> = report
            .changes
            .iter()
            .filter(|c| matches!(c.element, Element::Geometry { .. }))
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
        let geom: Vec<&Change> = report
            .changes
            .iter()
            .filter(|c| matches!(c.element, Element::Geometry { .. }))
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
        let rename: Vec<&Change> = r.changes.iter().filter(|c| c.kind == ChangeKind::Renamed).collect();
        assert_eq!(rename.len(), 1);
        assert_eq!(rename[0].element, Element::Cell { name: "INV_X1".into() });
        assert_eq!(rename[0].renamed_from.as_deref(), Some("INV"));
        let is_cell = |c: &&Change, n: &str| c.element == Element::Cell { name: n.into() } && c.kind != ChangeKind::Renamed;
        assert!(!r.changes.iter().any(|c| is_cell(&c, "INV") || is_cell(&c, "INV_X1")));
    }

    #[test]
    fn instances_of_the_same_subcell_are_grouped() {
        let r = GdsDriver::new().diff(
            &renderer_fixture("multi_inst_a.gds"),
            &renderer_fixture("multi_inst_b.gds"),
            "multi.gds",
        );
        let top = r.changes.iter().find(|c| c.element.name() == "TOP:L1/0:INV").expect("TOP");
        let Element::Geometry { via: Some(via), .. } = &top.element else { panic!("sin via: {top:?}") };
        assert_eq!((via.instances, via.at), (2, None));
        assert_eq!(top.after("added_area_um2").and_then(|v| v.as_f64()), Some(2.0));
    }
}
