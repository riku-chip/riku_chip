//! Diff de alto nivel sobre GDSII. Encapsula gdstk_rs y devuelve un reporte
//! de dominio Miku sin filtrar tipos del parser.

use std::collections::{BTreeMap, BTreeSet};

use gdstk_rs::{xor_split_flat, Cell, GdsTag, Library, OwnedPolygon};

use crate::hier_walk::{origin_of_polygon, OriginPath};

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
    /// Cadena de cells desde la raiz hasta la sub-cell que aporto el
    /// poligono. Longitud 1 (= [cell]) si nace en la cell raiz; 2 si
    /// viene via una reference. Profundidad mayor se bucketea por la
    /// reference inmediata (fase 1).
    pub origin_path: OriginPath,
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
    /// `true` si el poligono vino de un subtree atravesado (origin_path
    /// con mas de un segmento). El bbox esta en coords absolutas del top.
    pub flattened: bool,
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
    // fold desde +0.0: `Sum` de f64 parte de -0.0, y una lista vacia se
    // imprimia como "-0.000" ("+-0.000" en la CLI).
    polys.iter().map(|p| polygon_area_um2(p, unit_factor)).fold(0.0, |acc, a| acc + a)
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
///
/// Un lado vacio (0 bytes: el archivo no existia en ese commit) cuenta como
/// library vacia: todas las cells del otro lado son anadidas o removidas.
/// Bytes no vacios que no son GDSII siguen siendo error.
pub fn diff_gds_with_config(
    a: &[u8],
    b: &[u8],
    cfg: &DiffConfig,
) -> Result<GdsDiffReport, GdsError> {
    let parse_opt = |bytes: &[u8], side| -> Result<Option<Library>, GdsError> {
        if bytes.is_empty() { Ok(None) } else { parse_side(bytes, side).map(Some) }
    };
    let lib_a = parse_opt(a, "A")?;
    let lib_b = parse_opt(b, "B")?;

    // unit es metros/unit. Para µm: factor = unit / 1e-6.
    // Si A y B difieren en unit, usamos el de B (lado "after").
    let unit_factor = lib_b.as_ref().or(lib_a.as_ref()).map_or(1.0, |l| l.unit() / 1e-6);

    let mut report = GdsDiffReport::default();

    let names = |l: &Option<Library>| -> BTreeSet<String> {
        l.as_ref().map(|l| l.cells().map(|c| c.name().to_string()).collect()).unwrap_or_default()
    };
    let (names_a, names_b) = (names(&lib_a), names(&lib_b));

    report.cells_removed = names_a.difference(&names_b).cloned().collect();
    report.cells_added = names_b.difference(&names_a).cloned().collect();

    let (Some(lib_a), Some(lib_b)) = (lib_a, lib_b) else {
        return Ok(report);
    };

    let mut layers: BTreeSet<LayerKey> = BTreeSet::new();
    for t in lib_a.layers().into_iter().chain(lib_b.layers()) {
        layers.insert(t.into());
    }

    for name in names_a.intersection(&names_b) {
        let (Some(ca), Some(cb)) = (lib_a.find_cell(name), lib_b.find_cell(name)) else {
            continue;
        };
        let cell = diff_one_cell(name, Some(&ca), Some(&cb), &layers, unit_factor, cfg);
        report.geometry.extend(cell.geometry);
    }

    Ok(report)
}

/// Poligonos del XOR de una capa, en coordenadas de la cell comparada
/// (unidades de usuario de la library, como el resto de la escena).
#[derive(Clone, Debug, PartialEq)]
pub struct LayerPolygons {
    pub layer: LayerKey,
    /// Presentes en B y no en A.
    pub added: Vec<OwnedPolygon>,
    /// Presentes en A y no en B.
    pub removed: Vec<OwnedPolygon>,
}

/// Diff completo de una sola cell: metricas (como en [`GdsDiffReport`]) y
/// los poligonos del XOR para pintarlos.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellDiff {
    pub geometry: Vec<GdsGeomDiff>,
    pub polygons: Vec<LayerPolygons>,
}

/// Diff de la cell `name` entre dos libraries. Un lado `None` (archivo que
/// no existia) o una cell ausente de un lado cuentan como vacios: todo lo
/// del otro lado es anadido o removido.
pub fn diff_cell(
    lib_a: Option<&Library>,
    lib_b: Option<&Library>,
    name: &str,
    cfg: &DiffConfig,
) -> CellDiff {
    let unit_factor = lib_b.or(lib_a).map_or(1.0, |l| l.unit() / 1e-6);
    let layers: BTreeSet<LayerKey> = lib_a
        .into_iter()
        .chain(lib_b)
        .flat_map(|l| l.layers())
        .map(LayerKey::from)
        .collect();
    let ca = lib_a.and_then(|l| l.find_cell(name));
    let cb = lib_b.and_then(|l| l.find_cell(name));
    diff_one_cell(name, ca.as_ref(), cb.as_ref(), &layers, unit_factor, cfg)
}

/// Poligonos de `cell` en una capa, aplanando toda la jerarquia.
fn flat_layer(cell: &Cell<'_>, key: &LayerKey) -> Vec<OwnedPolygon> {
    cell.get_polygons()
        .with_filter(key.layer, key.datatype)
        .build()
        .polygons()
        .map(|p| OwnedPolygon { layer: p.layer(), datatype: p.datatype(), points: p.points().collect() })
        .collect()
}

fn diff_one_cell(
    name: &str,
    ca: Option<&Cell<'_>>,
    cb: Option<&Cell<'_>>,
    layers: &BTreeSet<LayerKey>,
    unit_factor: f64,
    cfg: &DiffConfig,
) -> CellDiff {
    let mut out = CellDiff::default();
    for key in layers {
        // Flatten con depth=-1 + filtro por (layer, dt): atravesamos
        // SREF/AREF y no perdemos cambios en sub-cells.
        let (added, removed) = match (ca, cb) {
            (Some(ca), Some(cb)) => {
                let fp_a = ca.get_polygons().with_filter(key.layer, key.datatype).build();
                let fp_b = cb.get_polygons().with_filter(key.layer, key.datatype).build();
                let split = xor_split_flat(&fp_a, &fp_b);
                (split.added, split.removed)
            }
            (None, Some(cb)) => (flat_layer(cb, key), Vec::new()),
            (Some(ca), None) => (Vec::new(), flat_layer(ca, key)),
            (None, None) => (Vec::new(), Vec::new()),
        };
        if added.is_empty() && removed.is_empty() {
            continue;
        }

        // Bucketear cada poligono por origin_path. Para `added` consultamos
        // las references del lado "after"; para `removed`, las del "before".
        let mut buckets: BTreeMap<OriginPath, BucketAcc> = BTreeMap::new();
        for p in &added {
            let origin = cb.map_or_else(|| vec![name.to_string()], |c| origin_of_polygon(c, p));
            buckets.entry(origin).or_default().added.push(p.clone());
        }
        for p in &removed {
            let origin = ca.map_or_else(|| vec![name.to_string()], |c| origin_of_polygon(c, p));
            buckets.entry(origin).or_default().removed.push(p.clone());
        }

        for (origin, acc) in buckets {
            let added_area = sum_area_um2(&acc.added, unit_factor);
            let removed_area = sum_area_um2(&acc.removed, unit_factor);
            let flattened = origin.len() > 1;
            out.geometry.push(GdsGeomDiff {
                cell: name.to_string(),
                origin_path: origin,
                layer: *key,
                added_polygons: acc.added.len(),
                removed_polygons: acc.removed.len(),
                added_area_um2: added_area,
                removed_area_um2: removed_area,
                bbox_um: union_bbox_um(&acc.added, &acc.removed, unit_factor),
                cosmetic: (added_area + removed_area) < cfg.cosmetic_threshold_um2,
                flattened,
            });
        }
        out.polygons.push(LayerPolygons { layer: *key, added, removed });
    }
    out
}

#[derive(Default)]
struct BucketAcc {
    added: Vec<OwnedPolygon>,
    removed: Vec<OwnedPolygon>,
}

/// Como cambio una cell entre dos libraries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellChange {
    Added,
    Removed,
    Modified,
}

/// Cells con cambios geometricos entre A y B (incluye cambios heredados de
/// sub-cells: si cambia `inv_2`, cambia toda cell que la instancie).
///
/// Dos pasadas: una huella barata de la geometria aplanada descarta las
/// cells identicas y el XOR confirma las candidatas (una huella distinta
/// puede ser solo reordenamiento de poligonos, sin cambio real). Un lado
/// `None` = archivo inexistente: todas sus cells cuentan como anadidas o
/// removidas.
pub fn changed_cells(lib_a: Option<&Library>, lib_b: Option<&Library>) -> BTreeMap<String, CellChange> {
    let names = |l: Option<&Library>| -> BTreeSet<String> {
        l.map(|l| l.cells().map(|c| c.name().to_string()).collect()).unwrap_or_default()
    };
    let (na, nb) = (names(lib_a), names(lib_b));
    let mut out: BTreeMap<String, CellChange> = BTreeMap::new();
    out.extend(nb.difference(&na).map(|n| (n.clone(), CellChange::Added)));
    out.extend(na.difference(&nb).map(|n| (n.clone(), CellChange::Removed)));

    let (Some(la), Some(lb)) = (lib_a, lib_b) else { return out };
    let cfg = DiffConfig::default();
    for name in na.intersection(&nb) {
        let (Some(ca), Some(cb)) = (la.find_cell(name), lb.find_cell(name)) else { continue };
        if geometry_fingerprint(&ca) == geometry_fingerprint(&cb) {
            continue;
        }
        if !diff_cell(Some(la), Some(lb), name, &cfg).geometry.is_empty() {
            out.insert(name.clone(), CellChange::Modified);
        }
    }
    out
}

/// Huella de la geometria aplanada de una cell, independiente del orden de
/// los poligonos: hash de cada (layer, datatype, puntos) y lista ordenada.
fn geometry_fingerprint(cell: &Cell<'_>) -> Vec<u64> {
    use std::hash::{Hash, Hasher};
    let flat = cell.get_polygons().build();
    let mut hashes: Vec<u64> = flat
        .polygons()
        .map(|p| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (p.layer(), p.datatype()).hash(&mut h);
            for q in p.points() {
                // Cuantizado a 1e-6 unidades de usuario: muy por debajo de la
                // grilla de cualquier PDK, estable ante ruido de punto flotante.
                ((q.x * 1e6).round() as i64, (q.y * 1e6).round() as i64).hash(&mut h);
            }
            h.finish()
        })
        .collect();
    hashes.sort_unstable();
    hashes
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
    fn empty_side_means_file_added_or_removed() {
        let lib = proof_lib_bytes();
        let cells: BTreeSet<String> = Library::from_bytes(&lib)
            .unwrap()
            .cells()
            .map(|c| c.name().to_string())
            .collect();
        let added = diff_gds(&[], &lib).expect("archivo nuevo");
        assert_eq!(added.cells_added.iter().cloned().collect::<BTreeSet<_>>(), cells);
        assert!(added.cells_removed.is_empty() && added.geometry.is_empty());
        let removed = diff_gds(&lib, &[]).expect("archivo borrado");
        assert_eq!(removed.cells_removed.len(), cells.len());
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
        // +0.0, no -0.0: la CLI imprimia "+-0.000".
        assert!(dt0.added_area_um2.is_sign_positive());
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
    fn hierarchical_change_detected_in_top_via_flatten() {
        // El cambio real esta dentro de INV, pero TOP solo la
        // referencia via SREF. Sin flatten, el diff de TOP-vs-TOP daria
        // 0 cambios. Con flatten, el rect extra se ve en coords
        // absolutas (12,10)-(13,11) con origin_path = ["TOP","INV"].
        let a = fixture_bytes("hier_inv_a.gds");
        let b = fixture_bytes("hier_inv_b.gds");
        let r = diff_gds(&a, &b).expect("diff");

        assert!(r.cells_added.is_empty());
        assert!(r.cells_removed.is_empty());

        let top_changes: Vec<&GdsGeomDiff> =
            r.geometry.iter().filter(|g| g.cell == "TOP").collect();
        assert!(
            !top_changes.is_empty(),
            "TOP debe reportar cambios via flatten (regresion de XOR jerarquico)",
        );

        let entry = top_changes
            .iter()
            .find(|g| g.layer == LayerKey { layer: 1, datatype: 0 })
            .expect("entry TOP layer 1/0");
        assert_eq!(entry.added_polygons, 1);
        assert_eq!(entry.removed_polygons, 0);
        assert!(entry.flattened, "entry de TOP via SREF debe tener flattened=true");
        assert_eq!(entry.origin_path, vec!["TOP".to_string(), "INV".to_string()]);

        // bbox del rect extra (2,0)-(3,1) trasladado por SREF en (10,10).
        let bb = entry.bbox_um.expect("bbox");
        assert!((bb.min_x - 12.0).abs() < 1e-6, "{}", bb.min_x);
        assert!((bb.min_y - 10.0).abs() < 1e-6, "{}", bb.min_y);
        assert!((bb.max_x - 13.0).abs() < 1e-6, "{}", bb.max_x);
        assert!((bb.max_y - 11.0).abs() < 1e-6, "{}", bb.max_y);
    }

    #[test]
    fn hierarchical_change_also_visible_in_referenced_cell() {
        // El mismo cambio debe verse tambien en INV directamente
        // con origin_path = ["INV"] (sin SREF, geometria directa).
        let a = fixture_bytes("hier_inv_a.gds");
        let b = fixture_bytes("hier_inv_b.gds");
        let r = diff_gds(&a, &b).expect("diff");

        let entry = r
            .geometry
            .iter()
            .find(|g| g.cell == "INV" && g.layer == LayerKey { layer: 1, datatype: 0 })
            .expect("entry INV layer 1/0");
        assert_eq!(entry.added_polygons, 1);
        assert!(!entry.flattened);
        assert_eq!(entry.origin_path, vec!["INV".to_string()]);
        // En INV el rect extra esta en (2,0)-(3,1) sin transform.
        let bb = entry.bbox_um.expect("bbox");
        assert!((bb.min_x - 2.0).abs() < 1e-6, "{}", bb.min_x);
        assert!((bb.max_x - 3.0).abs() < 1e-6, "{}", bb.max_x);
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
