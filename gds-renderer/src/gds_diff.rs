//! Diff de alto nivel sobre GDSII. Encapsula gdstk_rs y devuelve un reporte
//! de dominio Miku sin filtrar tipos del parser.

use std::collections::BTreeSet;

use gdstk_rs::{GdsTag, Library};

/// Identificador de capa GDS (par layer/datatype). Tipo propio para no
/// filtrar `gdstk_rs::GdsTag` por la API pública de gds-renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerKey {
    pub layer: u32,
    pub datatype: u32,
}

impl From<GdsTag> for LayerKey {
    fn from(t: GdsTag) -> Self {
        Self {
            layer: t.layer,
            datatype: t.datatype,
        }
    }
}

impl From<LayerKey> for GdsTag {
    fn from(k: LayerKey) -> Self {
        GdsTag {
            layer: k.layer,
            datatype: k.datatype,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GdsGeomDiff {
    pub cell: String,
    pub layer: LayerKey,
    pub added_polygons: usize,
    pub removed_polygons: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GdsDiffReport {
    pub cells_added: Vec<String>,
    pub cells_removed: Vec<String>,
    pub geometry: Vec<GdsGeomDiff>,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum GdsError {
    #[error("{side}: no es formato GDSII")]
    NotGdsii { side: &'static str },
    #[error("{side}: no se pudo parsear GDSII: {msg}")]
    Parse { side: &'static str, msg: String },
}

/// Verifica magic bytes GDSII (HEADER record: len=6, type=0x0002, big-endian).
fn is_gdsii(content: &[u8]) -> bool {
    content.len() >= 4
        && content[0] == 0x00
        && content[1] == 0x06
        && content[2] == 0x00
        && content[3] == 0x02
}

fn parse_side(content: &[u8], side: &'static str) -> Result<Library, GdsError> {
    if !is_gdsii(content) {
        return Err(GdsError::NotGdsii { side });
    }
    Library::from_bytes(content).map_err(|e| GdsError::Parse {
        side,
        msg: e.to_string(),
    })
}

/// Diff de dos GDSII: cells añadidas/removidas + XOR geométrico por (cell, layer, datatype).
/// Si A o B no son GDSII válidos, devuelve `Err` (el caller decide si lo trata como warning).
pub fn diff_gds(a: &[u8], b: &[u8]) -> Result<GdsDiffReport, GdsError> {
    let lib_a = parse_side(a, "A")?;
    let lib_b = parse_side(b, "B")?;

    let mut report = GdsDiffReport::default();

    let names_a: BTreeSet<String> = lib_a.cells().map(|c| c.name().to_string()).collect();
    let names_b: BTreeSet<String> = lib_b.cells().map(|c| c.name().to_string()).collect();

    report.cells_removed = names_a.difference(&names_b).cloned().collect();
    report.cells_added = names_b.difference(&names_a).cloned().collect();

    let mut layers: BTreeSet<LayerKey> = BTreeSet::new();
    for t in lib_a.layers().into_iter().chain(lib_b.layers()) {
        layers.insert(t.into());
    }

    for name in names_a.intersection(&names_b) {
        let Some(ca) = lib_a.find_cell(name) else {
            continue;
        };
        let Some(cb) = lib_b.find_cell(name) else {
            continue;
        };
        for key in &layers {
            let split = ca.xor_polygons_split(&cb, GdsTag::from(*key));
            let added = split.added.len();
            let removed = split.removed.len();
            if added == 0 && removed == 0 {
                continue;
            }
            report.geometry.push(GdsGeomDiff {
                cell: name.clone(),
                layer: *key,
                added_polygons: added,
                removed_polygons: removed,
            });
        }
    }

    Ok(report)
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
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn fixture_bytes(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn rejects_non_gdsii() {
        let res = diff_gds(b"NOT_GDS", &proof_lib_bytes());
        assert!(matches!(res, Err(GdsError::NotGdsii { side: "A" })));
    }

    #[test]
    fn identical_libs_empty_report() {
        let bytes = proof_lib_bytes();
        let r = diff_gds(&bytes, &bytes).expect("diff");
        assert!(r.cells_added.is_empty());
        assert!(r.cells_removed.is_empty());
        assert!(r.geometry.is_empty());
    }

    #[test]
    fn datatype_difference_is_reported() {
        let a = fixture_bytes("datatype_a.gds");
        let b = fixture_bytes("datatype_b.gds");
        let r = diff_gds(&a, &b).expect("diff");

        assert!(r.cells_added.is_empty());
        assert!(r.cells_removed.is_empty());

        let dt0 = r
            .geometry
            .iter()
            .find(|g| g.layer == LayerKey { layer: 1, datatype: 0 })
            .expect("entry para datatype=0");
        let dt1 = r
            .geometry
            .iter()
            .find(|g| g.layer == LayerKey { layer: 1, datatype: 1 })
            .expect("entry para datatype=1");
        assert_eq!(dt0.removed_polygons, 1);
        assert_eq!(dt0.added_polygons, 0);
        assert_eq!(dt1.removed_polygons, 0);
        assert_eq!(dt1.added_polygons, 1);
    }
}
