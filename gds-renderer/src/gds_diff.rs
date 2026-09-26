//! Diff de alto nivel sobre GDSII. Encapsula gdstk_rs y devuelve un reporte
//! de dominio Miku sin filtrar tipos del parser.

use std::collections::{BTreeMap, BTreeSet};

use gdstk_rs::{xor_split_flat, Cell, GdsTag, Library, OwnedPolygon};

use crate::hier_walk::{origin_of_polygon, Origin, OriginPath};

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
    /// Posicion (µm) de la instancia de la sub-cell que aporto el cambio.
    /// `None` si es geometria directa de la cell o si el item agrupa varias
    /// instancias (ver `instances`).
    pub instance_at_um: Option<(f64, f64)>,
    /// Instancias agrupadas en este item: 0 para geometria directa, 1 para
    /// un item por instancia (`diff_cell`), N en el reporte agrupado de
    /// `diff_gds` cuando la misma sub-cell cambia en N instancias.
    pub instances: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GdsDiffReport {
    pub cells_added: Vec<String>,
    pub cells_removed: Vec<String>,
    /// Pares `(nombre en A, nombre en B)` con la misma geometria.
    pub cells_renamed: Vec<(String, String)>,
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

    let (only_a, only_b) = (names_a.difference(&names_b).cloned().collect(), names_b.difference(&names_a).cloned().collect());
    for (from, to) in detect_renames(&lib_a, &lib_b, &only_a, &only_b) {
        report.cells_removed.retain(|n| *n != from);
        report.cells_added.retain(|n| *n != to);
        report.cells_renamed.push((from, to));
    }

    let mut layers: BTreeSet<LayerKey> = BTreeSet::new();
    for t in lib_a.layers().into_iter().chain(lib_b.layers()) {
        layers.insert(t.into());
    }

    for name in names_a.intersection(&names_b) {
        let (Some(ca), Some(cb)) = (lib_a.find_cell(name), lib_b.find_cell(name)) else {
            continue;
        };
        let cell = diff_one_cell(name, Some(&ca), Some(&cb), &layers, unit_factor, cfg);
        report.geometry.extend(group_instances(cell.geometry, cfg));
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
    diff_cell_as(lib_a, name, lib_b, name, cfg)
}

/// Como [`diff_cell`], con otro nombre de cada lado (celda renombrada:
/// `name_a` en A, `name_b` en B).
pub fn diff_cell_as(
    lib_a: Option<&Library>,
    name_a: &str,
    lib_b: Option<&Library>,
    name_b: &str,
    cfg: &DiffConfig,
) -> CellDiff {
    let unit_factor = lib_b.or(lib_a).map_or(1.0, |l| l.unit() / 1e-6);
    let layers: BTreeSet<LayerKey> = lib_a
        .into_iter()
        .chain(lib_b)
        .flat_map(|l| l.layers())
        .map(LayerKey::from)
        .collect();
    let ca = lib_a.and_then(|l| l.find_cell(name_a));
    let cb = lib_b.and_then(|l| l.find_cell(name_b));
    diff_one_cell(name_b, ca.as_ref(), cb.as_ref(), &layers, unit_factor, cfg)
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

        // Bucketear cada poligono por instancia de origen. Para `added`
        // consultamos las references del lado "after"; para `removed`, las
        // del "before". La clave es la posicion de la instancia, estable
        // entre ambos lados.
        let root = || Origin { path: vec![name.to_string()], instance_at: None };
        let mut buckets: BTreeMap<_, BucketAcc> = BTreeMap::new();
        for p in &added {
            let origin = cb.map_or_else(root, |c| origin_of_polygon(c, p));
            let acc = buckets.entry(origin.key()).or_default();
            acc.origin.get_or_insert(origin);
            acc.added.push(p.clone());
        }
        for p in &removed {
            let origin = ca.map_or_else(root, |c| origin_of_polygon(c, p));
            let acc = buckets.entry(origin.key()).or_default();
            acc.origin.get_or_insert(origin);
            acc.removed.push(p.clone());
        }

        for (_, acc) in buckets {
            let added_area = sum_area_um2(&acc.added, unit_factor);
            let removed_area = sum_area_um2(&acc.removed, unit_factor);
            let origin = acc.origin.unwrap_or_else(root);
            let flattened = origin.path.len() > 1;
            let instance_at_um = origin.instance_at.map(|(x, y)| (x * unit_factor, y * unit_factor));
            out.geometry.push(GdsGeomDiff {
                cell: name.to_string(),
                origin_path: origin.path,
                instance_at_um,
                instances: usize::from(flattened),
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
    origin: Option<Origin>,
    added: Vec<OwnedPolygon>,
    removed: Vec<OwnedPolygon>,
}

/// Agrupa los items de una misma (cell, capa, sub-cell) que solo difieren en
/// la instancia: la CLI reporta "en N instancias" en lugar de N lineas. Suma
/// conteos y areas, une bboxes y recalcula el flag cosmetico.
fn group_instances(items: Vec<GdsGeomDiff>, cfg: &DiffConfig) -> Vec<GdsGeomDiff> {
    let mut out: Vec<GdsGeomDiff> = Vec::with_capacity(items.len());
    for g in items {
        let same = out.iter_mut().find(|o| {
            o.instances > 0 && o.cell == g.cell && o.layer == g.layer && o.origin_path == g.origin_path
        });
        match same {
            Some(o) if g.instances > 0 => {
                o.added_polygons += g.added_polygons;
                o.removed_polygons += g.removed_polygons;
                o.added_area_um2 += g.added_area_um2;
                o.removed_area_um2 += g.removed_area_um2;
                o.bbox_um = match (o.bbox_um, g.bbox_um) {
                    (Some(a), Some(b)) => Some(BBoxUm {
                        min_x: a.min_x.min(b.min_x),
                        min_y: a.min_y.min(b.min_y),
                        max_x: a.max_x.max(b.max_x),
                        max_y: a.max_y.max(b.max_y),
                    }),
                    (a, b) => a.or(b),
                };
                o.instances += g.instances;
                o.instance_at_um = None;
                o.cosmetic = (o.added_area_um2 + o.removed_area_um2) < cfg.cosmetic_threshold_um2;
            }
            _ => out.push(g),
        }
    }
    out
}

/// Como cambio una cell entre dos libraries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellChange {
    Added,
    Removed,
    Modified,
    /// Misma geometria con otro nombre; `from` es el nombre en A.
    Renamed { from: String },
}

/// Empareja cells que desaparecen de A con cells que aparecen en B y tienen
/// la misma geometria aplanada: son renombres, no baja + alta. Retorna pares
/// `(nombre en A, nombre en B)`.
///
/// La huella propone y el XOR confirma. Si una huella se repite (varias
/// candidatas) no se adivina, y las cells sin geometria nunca se emparejan.
fn detect_renames(
    la: &Library,
    lb: &Library,
    removed: &BTreeSet<String>,
    added: &BTreeSet<String>,
) -> Vec<(String, String)> {
    let by_fp = |lib: &Library, names: &BTreeSet<String>| {
        let mut m: BTreeMap<Vec<u64>, Vec<String>> = BTreeMap::new();
        for n in names {
            if let Some(c) = lib.find_cell(n) {
                let fp = geometry_fingerprint(&c);
                if !fp.is_empty() {
                    m.entry(fp).or_default().push(n.clone());
                }
            }
        }
        m
    };
    let (fa, fb) = (by_fp(la, removed), by_fp(lb, added));
    let layers: BTreeSet<LayerKey> = la.layers().into_iter().chain(lb.layers()).map(LayerKey::from).collect();
    let cfg = DiffConfig { cosmetic_threshold_um2: 0.0 };
    let mut out = Vec::new();
    for (fp, to) in &fb {
        let Some(from) = fa.get(fp) else { continue };
        let ([from], [to]) = (from.as_slice(), to.as_slice()) else { continue };
        let (Some(ca), Some(cb)) = (la.find_cell(from), lb.find_cell(to)) else { continue };
        if diff_one_cell(to, Some(&ca), Some(&cb), &layers, 1.0, &cfg).geometry.is_empty() {
            out.push((from.clone(), to.clone()));
        }
    }
    out
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
    let only_a: BTreeSet<String> = na.difference(&nb).cloned().collect();
    let only_b: BTreeSet<String> = nb.difference(&na).cloned().collect();
    for (from, to) in detect_renames(la, lb, &only_a, &only_b) {
        out.remove(&from);
        out.insert(to, CellChange::Renamed { from });
    }
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

    fn multi_inst_libs() -> (Library, Library) {
        let a = Library::from_bytes(&fixture_bytes("multi_inst_a.gds")).expect("a");
        let b = Library::from_bytes(&fixture_bytes("multi_inst_b.gds")).expect("b");
        (a, b)
    }

    #[test]
    fn pure_rename_is_detected_and_parents_are_unchanged() {
        let a = Library::from_bytes(&fixture_bytes("rename_a.gds")).expect("a");
        let b = Library::from_bytes(&fixture_bytes("rename_b.gds")).expect("b");
        let changed = changed_cells(Some(&a), Some(&b));
        assert_eq!(changed.get("INV_X1"), Some(&CellChange::Renamed { from: "INV".into() }));
        assert!(!changed.contains_key("INV"), "el nombre viejo no queda como eliminada");
        assert!(!changed.contains_key("TOP"), "renombrar la sub-celda no cambia la geometria del padre");
        // Sin geometria no se adivina; con geometria distinta no es renombre.
        assert_eq!(changed.get("EMPTY_A"), Some(&CellChange::Removed));
        assert_eq!(changed.get("EMPTY_B"), Some(&CellChange::Added));
        assert_eq!(changed.get("OLD"), Some(&CellChange::Removed));
        assert_eq!(changed.get("NEW"), Some(&CellChange::Added));

        let d = diff_cell_as(Some(&a), "INV", Some(&b), "INV_X1", &DiffConfig::default());
        assert!(d.geometry.is_empty(), "{:?}", d.geometry);
    }

    #[test]
    fn report_lists_renames_apart_from_added_and_removed() {
        let r = diff_gds(&fixture_bytes("rename_a.gds"), &fixture_bytes("rename_b.gds")).expect("diff");
        assert_eq!(r.cells_renamed, vec![("INV".to_string(), "INV_X1".to_string())]);
        assert_eq!(r.cells_removed, vec!["EMPTY_A".to_string(), "OLD".to_string()]);
        assert_eq!(r.cells_added, vec!["EMPTY_B".to_string(), "NEW".to_string()]);
        assert!(r.geometry.is_empty(), "{:?}", r.geometry);
    }

    #[test]
    fn change_in_subcell_gives_one_item_per_instance() {
        let (a, b) = multi_inst_libs();
        let d = diff_cell(Some(&a), Some(&b), "TOP", &DiffConfig::default());
        let mut items: Vec<_> = d.geometry.iter().collect();
        items.sort_by(|x, y| x.instance_at_um.partial_cmp(&y.instance_at_um).unwrap());
        assert_eq!(items.len(), 2, "{items:?}");
        for (g, x) in items.iter().zip([10.0, 30.0]) {
            assert_eq!(g.origin_path, vec!["TOP".to_string(), "INV".to_string()]);
            assert_eq!(g.instance_at_um, Some((x, 10.0)));
            assert_eq!((g.added_polygons, g.instances), (1, 1));
            let bb = g.bbox_um.expect("bbox");
            assert!((bb.min_x - (x + 2.0)).abs() < 1e-6 && (bb.max_x - (x + 3.0)).abs() < 1e-6, "{bb:?}");
        }
    }

    #[test]
    fn each_array_repetition_is_its_own_instance() {
        let (a, b) = multi_inst_libs();
        let d = diff_cell(Some(&a), Some(&b), "ARR", &DiffConfig::default());
        let mut at: Vec<(f64, f64)> = d.geometry.iter().filter_map(|g| g.instance_at_um).collect();
        at.sort_by(|p, q| p.partial_cmp(q).unwrap());
        let expected: Vec<(f64, f64)> =
            [0.0, 10.0, 20.0].iter().flat_map(|&x| [(x, 0.0), (x, 5.0)]).collect();
        assert_eq!(at, expected);
        for g in &d.geometry {
            let (x, y) = g.instance_at_um.unwrap();
            let bb = g.bbox_um.unwrap();
            assert!((bb.min_x - (x + 2.0)).abs() < 1e-6 && (bb.min_y - y).abs() < 1e-6, "{bb:?}");
        }
    }

    #[test]
    fn report_groups_instances_of_the_same_subcell() {
        let r = diff_gds(&fixture_bytes("multi_inst_a.gds"), &fixture_bytes("multi_inst_b.gds")).expect("diff");
        let top: Vec<_> = r.geometry.iter().filter(|g| g.cell == "TOP").collect();
        assert_eq!(top.len(), 1, "{top:?}");
        assert_eq!((top[0].instances, top[0].added_polygons), (2, 2));
        assert_eq!(top[0].instance_at_um, None);
        assert!((top[0].added_area_um2 - 2.0).abs() < 1e-9);
        let arr = r.geometry.iter().find(|g| g.cell == "ARR").expect("ARR");
        assert_eq!(arr.instances, 6);
        let inv = r.geometry.iter().find(|g| g.cell == "INV").expect("INV");
        assert_eq!((inv.instances, inv.instance_at_um), (0, None));
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
