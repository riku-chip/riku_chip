//! Diff de alto nivel sobre GDSII. Encapsula gdstk_rs y devuelve un reporte
//! de dominio Miku sin filtrar tipos del parser.

use std::collections::BTreeSet;

use gdstk_rs::{GdsTag, Library, OwnedPolygon};

/// Identificador de capa GDS (par layer/datatype). Tipo propio para no
/// filtrar `gdstk_rs::GdsTag` por la API publica de gds-renderer.
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

/// Bounding box en micrometros (µm). Coords en espacio fisico, listo para UI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBoxUm {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GdsGeomDiff {
    pub cell: String,
    pub layer: LayerKey,
    pub added_polygons: usize,
    pub removed_polygons: usize,
    pub added_area_um2: f64,
    pub removed_area_um2: f64,
    pub bbox_um: Option<BBoxUm>,
    /// `true` si la suma de areas (anadida + removida) cae bajo el umbral
    /// cosmetico (`DiffConfig::cosmetic_threshold_um2`). Polygons que se
    /// reposicionan algunos nm tipicamente caen aqui.
    pub cosmetic: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
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

/// Umbral por defecto para marcar un diff GDS como cosmetico.
///
/// 0.01 µm² esta claramente por debajo del piso DRC en PDKs tipicos
/// (sky130, gf180: min width/spacing ~0.15-0.30 µm). Captura ruido tipo
/// snap-a-grilla y slivers de redondeo, sin esconder cambios reales.
pub const DEFAULT_COSMETIC_THRESHOLD_UM2: f64 = 0.01;

/// Configuracion del diff GDS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffConfig {
    pub cosmetic_threshold_um2: f64,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            cosmetic_threshold_um2: DEFAULT_COSMETIC_THRESHOLD_UM2,
        }
    }
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

/// Area absoluta de un poligono via shoelace. Coords ya en espacio de usuario
/// gdstk; se aplica `unit_factor` para llevar a µm² (factor=1 cuando unit=1e-6).
fn polygon_area_um2(p: &OwnedPolygon, unit_factor: f64) -> f64 {
    let pts = &p.points;
    if pts.len() < 3 {
        return 0.0;
    }
    let mut acc = 0.0_f64;
    for i in 0..pts.len() {
        let j = (i + 1) % pts.len();
        acc += pts[i].x * pts[j].y - pts[j].x * pts[i].y;
    }
    let factor2 = unit_factor * unit_factor;
    (acc * 0.5).abs() * factor2
}

fn sum_area_um2(polys: &[OwnedPolygon], unit_factor: f64) -> f64 {
    polys.iter().map(|p| polygon_area_um2(p, unit_factor)).sum()
}

fn union_bbox_um(
    added: &[OwnedPolygon],
    removed: &[OwnedPolygon],
    unit_factor: f64,
) -> Option<BBoxUm> {
    let mut bbox: Option<BBoxUm> = None;
    for p in added.iter().chain(removed.iter()) {
        for pt in &p.points {
            let x = pt.x * unit_factor;
            let y = pt.y * unit_factor;
            bbox = Some(match bbox {
                None => BBoxUm {
                    min_x: x,
                    min_y: y,
                    max_x: x,
                    max_y: y,
                },
                Some(b) => BBoxUm {
                    min_x: b.min_x.min(x),
                    min_y: b.min_y.min(y),
                    max_x: b.max_x.max(x),
                    max_y: b.max_y.max(y),
                },
            });
        }
    }
    bbox
}

/// Diff de dos GDSII con configuracion por defecto.
pub fn diff_gds(a: &[u8], b: &[u8]) -> Result<GdsDiffReport, GdsError> {
    diff_gds_with_config(a, b, &DiffConfig::default())
}

/// Diff de dos GDSII: cells anadidas/removidas + XOR geometrico por
/// (cell, layer, datatype) con metricas en µm² + bbox + flag cosmetico.
pub fn diff_gds_with_config(
    a: &[u8],
    b: &[u8],
    cfg: &DiffConfig,
) -> Result<GdsDiffReport, GdsError> {
    let lib_a = parse_side(a, "A")?;
    let lib_b = parse_side(b, "B")?;

    // unit es metros/unit. Para µm: factor = unit / 1e-6.
    // Si A y B difieren en unit, usamos el de B (lado "after").
    let unit_factor = lib_b.unit() / 1e-6;

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
            let added_n = split.added.len();
            let removed_n = split.removed.len();
            if added_n == 0 && removed_n == 0 {
                continue;
            }
            let added_area = sum_area_um2(&split.added, unit_factor);
            let removed_area = sum_area_um2(&split.removed, unit_factor);
            let bbox = union_bbox_um(&split.added, &split.removed, unit_factor);
            let cosmetic = (added_area + removed_area) < cfg.cosmetic_threshold_um2;
            report.geometry.push(GdsGeomDiff {
                cell: name.clone(),
                layer: *key,
                added_polygons: added_n,
                removed_polygons: removed_n,
                added_area_um2: added_area,
                removed_area_um2: removed_area,
                bbox_um: bbox,
                cosmetic,
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

        // El fixture es un rectangulo 10×10 µm = 100 µm². Tolerancia 1e-6.
        assert!((dt0.removed_area_um2 - 100.0).abs() < 1e-6, "{}", dt0.removed_area_um2);
        assert_eq!(dt0.added_area_um2, 0.0);
        assert!((dt1.added_area_um2 - 100.0).abs() < 1e-6, "{}", dt1.added_area_um2);
        assert_eq!(dt1.removed_area_um2, 0.0);

        // 100 µm² >> 0.01 µm² umbral -> NO cosmetico.
        assert!(!dt0.cosmetic);
        assert!(!dt1.cosmetic);

        // Bbox debe cubrir el rectangulo (0,0)-(10,10).
        let b0 = dt0.bbox_um.expect("bbox dt=0");
        assert_eq!(b0.min_x, 0.0);
        assert_eq!(b0.min_y, 0.0);
        assert_eq!(b0.max_x, 10.0);
        assert_eq!(b0.max_y, 10.0);
    }

    #[test]
    fn cosmetic_threshold_classifies_small_changes() {
        // Mismo fixture pero con threshold 200 µm² -> el cambio de 100 µm²
        // queda bajo el umbral y debe marcarse cosmetico.
        let a = fixture_bytes("datatype_a.gds");
        let b = fixture_bytes("datatype_b.gds");
        let cfg = DiffConfig {
            cosmetic_threshold_um2: 200.0,
        };
        let r = diff_gds_with_config(&a, &b, &cfg).expect("diff");
        assert!(r.geometry.iter().all(|g| g.cosmetic));
    }
}
