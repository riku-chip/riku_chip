use std::collections::{BTreeMap, BTreeSet};

use gdstk_rs::{GdsTag, Library, XorSplit};

use crate::core::domain::driver::{DiffEntry, DriverDiffReport, DriverInfo, RikuDriver};
use crate::core::domain::models::{ChangeKind, DriverKind, FileFormat};
use crate::core::format::detect_format;

/// Valida un blob como GDSII y lo parsea con `Library::from_bytes`. Mirror del
/// `validate_xschem` (`xschem_driver.rs`): chequea formato y propaga el error
/// como warning textual para que el report quede simétrico A/B.
fn validate_and_parse(content: &[u8], side: &str, path_hint: &str) -> Result<Library, String> {
    if detect_format(content) != FileFormat::Gds {
        return Err(format!(
            "{path_hint} ({side}): no es formato GDSII, se omite el diff."
        ));
    }
    Library::from_bytes(content)
        .map_err(|e| format!("{path_hint} ({side}): no se pudo parsear GDSII: {e}"))
}

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

fn geom_entry(cell: &str, tag: GdsTag, split: &XorSplit) -> Option<DiffEntry> {
    let added_n = split.added.len();
    let removed_n = split.removed.len();
    if added_n == 0 && removed_n == 0 {
        return None;
    }
    let kind = match (added_n, removed_n) {
        (a, 0) if a > 0 => ChangeKind::Added,
        (0, r) if r > 0 => ChangeKind::Removed,
        _ => ChangeKind::Modified,
    };
    let mut after = BTreeMap::new();
    after.insert("added_polygons".to_string(), added_n.to_string());
    after.insert("removed_polygons".to_string(), removed_n.to_string());
    Some(DiffEntry {
        kind,
        element: format!("{cell}:L{}/{}", tag.layer, tag.datatype),
        before: None,
        after: Some(after),
        cosmetic: false,
        position_changed: false,
    })
}

pub struct GdsDriver {
    cached_info: std::sync::OnceLock<DriverInfo>,
}

impl GdsDriver {
    pub fn new() -> Self {
        Self {
            cached_info: std::sync::OnceLock::new(),
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
            version: "gdstk-rs (cxx bridge)".to_string(),
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

        let lib_a = match validate_and_parse(content_a, "A", path_hint) {
            Ok(l) => l,
            Err(w) => {
                report.warnings.push(w);
                return report;
            }
        };
        let lib_b = match validate_and_parse(content_b, "B", path_hint) {
            Ok(l) => l,
            Err(w) => {
                report.warnings.push(w);
                return report;
            }
        };

        let names_a: BTreeSet<String> = lib_a.cells().map(|c| c.name().to_string()).collect();
        let names_b: BTreeSet<String> = lib_b.cells().map(|c| c.name().to_string()).collect();

        for name in names_a.difference(&names_b) {
            report.changes.push(cell_entry(name, ChangeKind::Removed));
        }
        for name in names_b.difference(&names_a) {
            report.changes.push(cell_entry(name, ChangeKind::Added));
        }

        // Unión de tags (layer, datatype) ordenada para reproducibilidad.
        let mut layer_set: BTreeSet<(u32, u32)> = BTreeSet::new();
        for t in lib_a.layers().into_iter().chain(lib_b.layers()) {
            layer_set.insert((t.layer, t.datatype));
        }

        for name in names_a.intersection(&names_b) {
            let Some(ca) = lib_a.find_cell(name) else { continue };
            let Some(cb) = lib_b.find_cell(name) else { continue };
            for (layer, datatype) in &layer_set {
                let split = ca.xor_polygons_split(&cb, *layer);
                let tag = GdsTag {
                    layer: *layer,
                    datatype: *datatype,
                };
                if let Some(entry) = geom_entry(name, tag, &split) {
                    report.changes.push(entry);
                }
            }
        }

        report
    }

    fn normalize(&self, content: &[u8], _path_hint: &str) -> Vec<u8> {
        content.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
