//! Diff de alto nivel sobre layouts (GDSII, OASIS y Magic). Encapsula
//! gdstk_rs y devuelve un reporte de riku sin filtrar tipos del parser.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use gdstk_rs::{sniff_format, Cell, GdsTag, Library, OwnedPolygon};
use rayon::prelude::*;
use viewer_core::FileSource;

use crate::source::{self, Raw, ReadError};
use crate::top_cell::is_meta_cell;

use crate::hier_walk::{Origin, OriginPath, Origins};
use crate::prints::{layer_prints, pair_prints, tree_prints, xor_layer, LayerPrints, PairPrints};

/// Identificador de capa GDS (par layer/datatype). Tipo propio para no
/// filtrar `gdstk_rs::GdsTag` por la API publica de riku-mod-layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BBoxUm {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GdsGeomDiff {
    pub cell: String,
    /// Cadena de cells desde la raiz hasta la sub-cell que aporto el
    /// poligono. Longitud 1 (= [cell]) si nace en la cell raiz; 2 si
    /// viene via una reference. Profundidad mayor se bucketea por la
    /// reference inmediata (fase 1).
    pub origin_path: OriginPath,
    pub layer: LayerKey,
    /// Nombre de la capa si el archivo lo da (Magic, LAYERNAME de OASIS).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_name: Option<String>,
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

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GdsDiffReport {
    pub cells_added: Vec<String>,
    pub cells_removed: Vec<String>,
    /// Pares `(nombre en A, nombre en B)` con la misma geometria.
    pub cells_renamed: Vec<(String, String)>,
    pub geometry: Vec<GdsGeomDiff>,
    /// Puertos que cambiaron (Magic).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<crate::mag::PortChange>,
    /// Transistores que cambiaron, en las celdas con cambios de geometría
    /// (ver `docs/formatos.md`, «Transistores y redes»).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<crate::devices::DeviceChange>,
    /// Abiertos, cortos y renombres de redes, en las celdas con cambios en
    /// una capa conductora (ver `docs/formatos.md`, «Transistores y redes»).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nets: Vec<crate::nets::NetChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum GdsError {
    #[error("{side}: no es un layout GDSII, OASIS ni Magic")]
    NotGdsii { side: &'static str },
    #[error("{side}: no se pudo leer el layout: {msg}")]
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

/// `true` si el contenido es un layout legible: GDSII (record HEADER),
/// OASIS (firma `%SEMI-OASIS\r\n`) o Magic (primera línea `magic`).
pub fn is_layout(content: &[u8]) -> bool {
    sniff_format(content).is_some()
}

/// Un GDS corrupto puede tener un ciclo de celdas (A instancia a B y B a
/// A). Aplanarlo recursa sin fin en gdstk y aborta el proceso por desborde
/// de pila, así que se busca antes, una vez: DFS iterativo por el grafo de
/// referencias, O(celdas + referencias). El error dice el ciclo.
pub(crate) fn check_acyclic(lib: &Library) -> Result<(), String> {
    let cells: Vec<_> = lib.cells().collect();
    let index: HashMap<&str, usize> = cells.iter().enumerate().map(|(i, c)| (c.name(), i)).collect();
    // Hijos de cada celda (solo los que están en el archivo).
    let children: Vec<Vec<usize>> = cells
        .iter()
        .map(|c| c.references().filter_map(|r| index.get(r.cell_name()).copied()).collect())
        .collect();
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        New,
        Open,
        Done,
    }
    let mut mark = vec![Mark::New; cells.len()];
    for root in 0..cells.len() {
        if mark[root] != Mark::New {
            continue;
        }
        // Pila de (celda, próximo hijo a visitar); la pila es el camino.
        let mut stack = vec![(root, 0usize)];
        mark[root] = Mark::Open;
        while let Some(top) = stack.last_mut() {
            let (cell, next) = *top;
            top.1 += 1;
            match children[cell].get(next) {
                Some(&child) => {
                    match mark[child] {
                        Mark::New => {
                            mark[child] = Mark::Open;
                            stack.push((child, 0));
                        }
                        Mark::Open => {
                            let start = stack.iter().position(|&(c, _)| c == child).unwrap_or(0);
                            let mut path: Vec<&str> = stack[start..].iter().map(|&(c, _)| cells[c].name()).collect();
                            path.push(cells[child].name());
                            return Err(format!("ciclo de celdas (el archivo está dañado): {}", path.join(" → ")));
                        }
                        Mark::Done => {}
                    }
                }
                None => {
                    mark[cell] = Mark::Done;
                    stack.pop();
                }
            }
        }
    }
    Ok(())
}

/// Un lado de un diff: el contenido del archivo y los otros archivos de su
/// versión (para las sub-celdas de un `.mag`). `files = None` = solo el PDK.
#[derive(Clone, Copy)]
pub struct LayoutSide<'a> {
    pub bytes: &'a [u8],
    pub files: Option<&'a dyn FileSource>,
}

/// Diff de un archivo de layout entre dos versiones, con acceso a los otros
/// archivos de cada una: un `.mag` resuelve sus sub-celdas en su misma
/// versión (ver `mag`). `path` es la ruta del archivo relativa a la raíz de
/// `files`. Un lado vacío (0 bytes: el archivo no existía en esa versión)
/// cuenta como library vacía; bytes que no son un layout son error.
///
/// La cache usa como clave todos los archivos leídos (editar una sub-celda
/// cambia el resultado aunque el archivo principal sea el mismo) y el lambda
/// de Magic.
pub fn diff_layout_sides(
    a: LayoutSide<'_>,
    b: LayoutSide<'_>,
    path: &str,
    cfg: &DiffConfig,
    cache: &crate::DiffCache,
) -> Result<GdsDiffReport, GdsError> {
    fn collect<'a>(s: LayoutSide<'a>, path: &str, side: &'static str) -> Result<Option<Raw<'a>>, GdsError> {
        if s.bytes.is_empty() {
            return Ok(None);
        }
        source::collect(s.bytes, Some(path), s.files).map(Some).map_err(|e| side_error(e, side))
    }
    let (ra, rb) = rayon::join(|| collect(a, path, "A"), || collect(b, path, "B"));
    let (ra, rb) = (ra?, rb?);
    let (inputs, read_params) = source::pair_key(ra.as_ref(), rb.as_ref());
    let params = format!("{read_params}cosmetic={}", cfg.cosmetic_threshold_um2);
    cache
        .get_or_compute("report", &inputs, &params, || {
            let read = |r: &Option<Raw<'_>>, side| r.as_ref().map(|r| r.read(None).map_err(|e| side_error(e, side))).transpose();
            let (la, lb) = rayon::join(|| read(&ra, "A"), || read(&rb, "B"));
            let (mut la, mut lb) = (la?, lb?);
            source::same_unit(ra.as_ref(), &mut la, rb.as_ref(), &mut lb).map_err(|e| side_error(e, "A"))?;
            let mut report = diff_libraries(la.as_ref().map(|s| &*s.lib), lb.as_ref().map(|s| &*s.lib), cfg);
            report.ports = crate::mag::port_changes(
                la.as_ref().and_then(|s| s.info.as_ref()),
                lb.as_ref().and_then(|s| s.info.as_ref()),
            );
            for (label, side) in [("antes", &la), ("después", &lb)] {
                if let Some(s) = side {
                    report.warnings.extend(s.notices.iter().map(|n| format!("{label}: {n}")));
                }
            }
            if let (Some(a), Some(b)) = (&la, &lb) {
                device_changes(&a.lib, &b.lib, path, &mut report);
                net_changes(&a.lib, &b.lib, (a.info.as_ref(), b.info.as_ref()), path, &mut report);
            }
            Ok(report)
        })
        .map(|(r, _)| r)
}

/// Transistores que cambiaron en cada celda con cambios de geometría (que
/// exista en los dos lados); una celda demasiado grande queda en un aviso.
/// Solo las celdas donde cambió alguna capa de las reglas (difusión, poly,
/// implantes, pozos, o las de Magic de transistores y fuente/drenaje): si
/// cambió solo metal, ningún transistor pudo cambiar, y no se aplana nada.
fn device_changes(la: &Library, lb: &Library, path: &str, report: &mut GdsDiffReport) {
    let Some(rules) = crate::devices::rules_for_library(lb, Some(path)) else { return };
    let used = rules.used_layers();
    let magic: HashMap<(u32, u32), String> = lb.layer_names().into_iter().map(|(t, n)| ((t.layer, t.datatype), n)).collect();
    let relevant = |k: &LayerKey| {
        used.iter().any(|&(l, d)| l == k.layer && d.is_none_or(|d| d == k.datatype))
            || magic.get(&(k.layer, k.datatype)).is_some_and(|n| {
                rules.device_type(n).is_some() || rules.devices.iter().any(|(_, t)| rules.is_sd_of(t, n))
            })
    };
    let cells: BTreeSet<&str> = report.geometry.iter().filter(|g| own_change(g) && relevant(&g.layer)).map(|g| g.cell.as_str()).collect();
    let cells: Vec<&str> = cells.into_iter().collect();
    let found: Vec<(&str, Option<Vec<crate::devices::DeviceChange>>)> = cells
        .par_iter()
        .map(|&c| (c, crate::devices::cell_device_changes(la, lb, c, rules, crate::devices::MAX_POLYGONS)))
        .collect();
    let mut skipped = Vec::new();
    for (cell, changes) in found {
        match changes {
            Some(v) => report.devices.extend(v),
            None if la.find_cell(cell).is_some() && lb.find_cell(cell).is_some() => skipped.push(cell.to_string()),
            None => {}
        }
    }
    if !skipped.is_empty() {
        report.warnings.push(format!(
            "transistores no comparados en {} (más de {} millones de polígonos): se comparan en sus sub-celdas",
            skipped.join(", "),
            crate::devices::MAX_POLYGONS / 1_000_000
        ));
    }
}

/// Un cambio en la geometría propia de la celda, no heredado de una
/// sub-celda. Transistores y redes se analizan solo ahí: el cambio de una
/// sub-celda ya se analiza en ella, y repetirlo en cada ancestro (hasta la
/// raíz de un chip) cuesta segundos por commit. Lo que no se ve así: un
/// corto que solo aparece por el contexto del padre.
fn own_change(g: &GdsGeomDiff) -> bool {
    g.origin_path.len() <= 1
}

/// Abiertos y cortos en cada celda con cambios en una capa que conduce
/// (según las reglas del PDK); una celda demasiado grande queda en un aviso.
fn net_changes(
    la: &Library,
    lb: &Library,
    info: (Option<&gdstk_rs::magic::MagInfo>, Option<&gdstk_rs::magic::MagInfo>),
    path: &str,
    report: &mut GdsDiffReport,
) {
    let Some(rules) = crate::devices::rules_for_library(lb, Some(path)) else { return };
    let mut types = rules.conductors();
    types.extend(rules.resistors.iter().map(|r| r.magic.clone()));
    let used = rules.type_layers(&types);
    let magic: HashMap<(u32, u32), String> = lb.layer_names().into_iter().map(|(t, n)| ((t.layer, t.datatype), n)).collect();
    let relevant = |k: &LayerKey| {
        used.iter().any(|&(l, d)| l == k.layer && d.is_none_or(|d| d == k.datatype))
            || magic.get(&(k.layer, k.datatype)).is_some_and(|n| types.iter().any(|t| t == rules.canonical(n)))
    };
    let mut cells: BTreeMap<&str, Vec<[f64; 4]>> = BTreeMap::new();
    for g in report.geometry.iter().filter(|g| own_change(g) && relevant(&g.layer)) {
        let boxes = cells.entry(g.cell.as_str()).or_default();
        if let Some(b) = g.bbox_um {
            boxes.push([b.min_x, b.min_y, b.max_x, b.max_y]);
        }
    }
    let found: Vec<(&str, Option<Vec<crate::nets::NetChange>>)> = cells
        .par_iter()
        .map(|(&c, boxes)| (c, crate::nets::cell_net_changes(la, lb, c, rules, crate::devices::MAX_POLYGONS, info, boxes)))
        .collect();
    let mut skipped = Vec::new();
    for (cell, changes) in found {
        match changes {
            Some(v) => report.nets.extend(v),
            None if la.find_cell(cell).is_some() && lb.find_cell(cell).is_some() => skipped.push(cell.to_string()),
            None => {}
        }
    }
    // Los ancestros de lo que cambió: solo alrededor del cambio (ver
    // `nets::context`).
    let unit_um = lb.unit() / 1e-6;
    let mut via: HashMap<String, BTreeSet<String>> = HashMap::new();
    for g in report.geometry.iter().filter(|g| !own_change(g) && relevant(&g.layer)) {
        if let Some(child) = g.origin_path.get(1) {
            via.entry(g.cell.clone()).or_default().insert(child.clone());
        }
    }
    // Las ventanas de una celda que cambió y que otras usan: sus polígonos
    // del XOR (lo que cambió de verdad, no su caja), con 50 nm de margen para
    // que un toque en el borde caiga adentro.
    let children: BTreeSet<&String> = via.values().flatten().collect();
    let margin = 0.05 / unit_um;
    let cfg = DiffConfig::default();
    let xor: Vec<(String, Vec<[f64; 4]>, BTreeSet<LayerKey>)> = cells
        .keys()
        .filter(|c| children.iter().any(|k| k.as_str() == **c))
        .collect::<Vec<_>>()
        .par_iter()
        .map(|&&c| {
            let d = diff_cell(Some(la), Some(lb), c, &cfg);
            let changed: Vec<&LayerPolygons> = d.polygons.iter().filter(|lp| relevant(&lp.layer)).collect();
            let boxes = changed
                .iter()
                .flat_map(|lp| lp.added.iter().chain(&lp.removed))
                .map(|p| {
                    let b = crate::devices::extract::bbox(p);
                    [b[0] - margin, b[1] - margin, b[2] + margin, b[3] + margin]
                })
                .collect();
            (c.to_string(), boxes, changed.iter().map(|lp| lp.layer.clone()).collect())
        })
        .collect();
    let own: HashMap<String, Vec<[f64; 4]>> = xor.iter().map(|(c, b, _)| (c.clone(), b.clone())).collect();
    // Los tipos a mirar en las ventanas: los de las capas que cambiaron y los
    // que conducen con ellos (sus contactos y vías). Un cambio en metal2 solo
    // une o separa redes por metal2 y sus vías.
    let changed: BTreeSet<LayerKey> = xor.iter().flat_map(|(_, _, l)| l.iter().cloned()).collect();
    let mut window_types: Vec<String> = types
        .iter()
        .filter(|t| {
            let base = rules.base_layers(t);
            changed.iter().any(|k| {
                base.iter().any(|&(l, d)| l == k.layer && d.is_none_or(|d| d == k.datatype))
                    || magic.get(&(k.layer, k.datatype)).is_some_and(|n| rules.canonical(n) == t.as_str())
            })
        })
        .cloned()
        .collect();
    for (a, b) in &rules.connect {
        if a.iter().chain(b).any(|t| window_types.contains(t)) {
            for t in a.iter().chain(b) {
                if t != "space" && !window_types.contains(t) {
                    window_types.push(t.clone());
                }
            }
        }
    }
    let windows = crate::nets::context::windows(lb, &own, &via);
    let inherited: Vec<&String> = via.keys().filter(|c| !cells.contains_key(c.as_str())).collect();
    let found: Vec<(&String, Vec<crate::nets::NetChange>, bool)> = inherited
        .par_iter()
        .filter_map(|&c| {
            let w = windows.get(c).filter(|w| !w.is_empty())?;
            let (ca, cb) = (la.find_cell(c)?, lb.find_cell(c)?);
            let max = crate::devices::MAX_POLYGONS;
            if crate::devices::flat_polygon_estimate(la, &ca).max(crate::devices::flat_polygon_estimate(lb, &cb)) > max {
                return None;
            }
            let window = Some((w.as_slice(), window_types.as_slice()));
            let (a, b) = rayon::join(
                || crate::nets::cell_nets_in(la, &ca, rules, info.0, window),
                || crate::nets::cell_nets_in(lb, &cb, rules, info.1, window),
            );
            let w_um: Vec<[f64; 4]> = w.iter().map(|b| b.map(|v| v * unit_um)).collect();
            if !crate::nets::pieces_changed(&a, &b) {
                return None;
            }
            use crate::nets::NetChangeKind::{Open, Short};
            let near = crate::nets::net_changes(c, &a, &b, unit_um, &w_um);
            // Confirmar con la celda entera (el recorte puede partir una red).
            match crate::nets::cell_net_changes(la, lb, c, rules, max, info, &w_um) {
                Some(full) => Some((c, full.into_iter().filter(|n| matches!(n.kind, Open | Short)).collect(), true)),
                None => Some((c, near, false)),
            }
        })
        .collect();
    // El mismo corto se ve en cada ancestro de donde aparece: se informa solo
    // en la celda más baja.
    let mut with: BTreeSet<String> = report.nets.iter().map(|n| n.cell.clone()).collect();
    with.extend(found.iter().filter(|(_, v, _)| !v.is_empty()).map(|(c, _, _)| (*c).clone()));
    fn below(c: &str, via: &HashMap<String, BTreeSet<String>>, with: &BTreeSet<String>, depth: usize) -> bool {
        depth < 64 && via.get(c).is_some_and(|kids| kids.iter().any(|k| with.contains(k) || below(k, via, with, depth + 1)))
    }
    for (c, v, confirmed) in found {
        if below(c, &via, &with, 0) {
            continue;
        }
        if !confirmed && !v.is_empty() {
            report.warnings.push(format!("{}: redes comparadas solo cerca del cambio", v[0].cell));
        }
        report.nets.extend(v);
    }
    if !skipped.is_empty() {
        report.warnings.push(format!(
            "redes no comparadas en {} (más de {} millones de polígonos): se comparan en sus sub-celdas",
            skipped.join(", "),
            crate::devices::MAX_POLYGONS / 1_000_000
        ));
    }
}

fn side_error(e: ReadError, side: &'static str) -> GdsError {
    match e {
        ReadError::NotLayout => GdsError::NotGdsii { side },
        ReadError::Parse(msg) => GdsError::Parse { side, msg },
    }
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

/// Diff de dos layouts sueltos (un `.mag` solo ve el PDK).
#[cfg(test)]
pub(crate) fn diff_gds(a: &[u8], b: &[u8]) -> Result<GdsDiffReport, GdsError> {
    diff_gds_with_config(a, b, &DiffConfig::default())
}

#[cfg(test)]
pub(crate) fn diff_gds_with_config(a: &[u8], b: &[u8], cfg: &DiffConfig) -> Result<GdsDiffReport, GdsError> {
    let side = |bytes| LayoutSide { bytes, files: None };
    diff_layout_sides(side(a), side(b), "layout.mag", cfg, &crate::DiffCache::disabled())
}

/// El aviso del lector de gdstk, si hubo (el archivo se leyó igual). Una
/// referencia a una celda que no está (típico de un stream-out parcial o de
/// celdas de otra biblioteca) dice cuáles: esas instancias no se comparan.
pub(crate) fn read_notes(lib: &Library) -> Vec<String> {
    let Some(code) = lib.read_warning() else { return Vec::new() };
    if code != gdstk_rs::ErrorCode::MissingReference {
        return vec![format!("el lector de GDS avisó: {}", code.as_str())];
    }
    let cells: HashSet<&str> = lib.cells().map(|c| c.name()).collect();
    let missing: BTreeSet<&str> = lib
        .cells()
        .flat_map(|c| c.references().map(|r| r.cell_name()).collect::<Vec<_>>())
        .filter(|n| !cells.contains(n))
        .collect();
    let list: Vec<&str> = missing.into_iter().collect();
    vec![format!("instancias de celdas que no están en el archivo (no se comparan): {}", list.join(", "))]
}

/// Diff de dos librerías ya leídas (`None` = el archivo no existía de ese
/// lado), de cualquier formato.
/// Los cambios llevan el nombre de su capa si el archivo lo da.
pub(crate) fn diff_libraries(lib_a: Option<&Library>, lib_b: Option<&Library>, cfg: &DiffConfig) -> GdsDiffReport {
    let mut report = diff_libraries_unnamed(lib_a, lib_b, cfg);
    let names: HashMap<LayerKey, String> = lib_a
        .into_iter()
        .chain(lib_b)
        .flat_map(|l| l.layer_names())
        .map(|(t, n)| (LayerKey::from(t), n))
        .collect();
    if !names.is_empty() {
        for g in &mut report.geometry {
            g.layer_name = names.get(&g.layer).cloned();
        }
    }
    report
}

fn diff_libraries_unnamed(lib_a: Option<&Library>, lib_b: Option<&Library>, cfg: &DiffConfig) -> GdsDiffReport {
    let cells = pair_cells(lib_a, lib_b);
    let mut report = GdsDiffReport {
        cells_added: cells.added,
        cells_removed: cells.removed,
        cells_renamed: cells.renamed,
        ..Default::default()
    };
    let (Some(lib_a), Some(lib_b)) = (lib_a, lib_b) else {
        return report;
    };
    let per_cell = map_changed_cells(lib_a, lib_b, &cells.common, cfg, |name, d| {
        let notes = d.failure_notes(name);
        (d.geometry, notes)
    });
    for (geometry, notes) in per_cell {
        report.geometry.extend(group_instances(geometry, cfg));
        report.warnings.extend(notes);
    }
    report
}

/// Las celdas de dos libraries: las que solo están en una, los renombres
/// (misma geometría con otro nombre, ver [`detect_renames`]) y las comunes.
/// Todo en orden de nombre.
struct CellPairing {
    added: Vec<String>,
    removed: Vec<String>,
    renamed: Vec<(String, String)>,
    common: Vec<String>,
}

fn pair_cells(lib_a: Option<&Library>, lib_b: Option<&Library>) -> CellPairing {
    let names = |l: Option<&Library>| -> BTreeSet<String> {
        l.map(|l| l.cells().map(|c| c.name().to_string()).filter(|n| !is_meta_cell(n)).collect()).unwrap_or_default()
    };
    let (na, nb) = (names(lib_a), names(lib_b));
    let only_a: BTreeSet<String> = na.difference(&nb).cloned().collect();
    let only_b: BTreeSet<String> = nb.difference(&na).cloned().collect();
    let renamed = match (lib_a, lib_b) {
        (Some(la), Some(lb)) => detect_renames(la, lb, &only_a, &only_b),
        _ => Vec::new(),
    };
    CellPairing {
        removed: only_a.into_iter().filter(|n| !renamed.iter().any(|(from, _)| from == n)).collect(),
        added: only_b.into_iter().filter(|n| !renamed.iter().any(|(_, to)| to == n)).collect(),
        common: na.intersection(&nb).cloned().collect(),
        renamed,
    }
}

/// `f` sobre el diff de cada celda común cuya geometría aplanada cambió, en
/// paralelo y en orden de nombre. Las de igual huella jerárquica tienen el
/// mismo aplanado y ni se aplanan; las de igual huella aplanada no pasan
/// por el XOR.
fn map_changed_cells<T: Send>(
    lib_a: &Library,
    lib_b: &Library,
    common: &[String],
    cfg: &DiffConfig,
    f: impl Fn(&str, CellDiff) -> T + Sync,
) -> Vec<T> {
    // unit es metros/unit. Para µm: factor = unit / 1e-6. Los dos lados
    // llegan en la misma unidad (`source::same_unit`).
    let unit_factor = lib_b.unit() / 1e-6;
    let layers: BTreeSet<LayerKey> = lib_a.layers().into_iter().chain(lib_b.layers()).map(LayerKey::from).collect();
    let (tree_a, tree_b) = rayon::join(|| tree_prints(lib_a), || tree_prints(lib_b));
    common
        .par_iter()
        .filter_map(|name| {
            if tree_a.same(&tree_b, name) {
                return None;
            }
            let (ca, cb) = (lib_a.find_cell(name)?, lib_b.find_cell(name)?);
            let prints = pair_prints(&ca, &cb, Some((&tree_a, &tree_b)));
            if prints.same() {
                return None;
            }
            Some(f(name, diff_one_cell(name, Some(&ca), Some(&cb), &layers, unit_factor, cfg, Some(&prints))))
        })
        .collect()
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
    /// Capas donde falló la operación booleana (Clipper): su diff puede
    /// estar incompleto.
    pub failed_layers: Vec<LayerKey>,
}

impl CellDiff {
    /// Un aviso por capa con el XOR fallido.
    pub fn failure_notes(&self, cell: &str) -> Vec<String> {
        self.failed_layers
            .iter()
            .map(|k| format!("{cell}, capa {}/{}: falló la operación booleana (Clipper); el cambio de esa capa puede estar incompleto", k.layer, k.datatype))
            .collect()
    }
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
pub(crate) fn diff_cell_as(
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
    // Con las huellas jerarquicas, las instancias gemelas no se aplanan.
    let pair = ca.as_ref().zip(cb.as_ref()).map(|(a, b)| {
        let trees = lib_a.zip(lib_b).map(|(la, lb)| rayon::join(|| tree_prints(la), || tree_prints(lb)));
        pair_prints(a, b, trees.as_ref().map(|(ta, tb)| (ta, tb)))
    });
    diff_one_cell(name_b, ca.as_ref(), cb.as_ref(), &layers, unit_factor, cfg, pair.as_ref())
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
    prints: Option<&PairPrints>,
) -> CellDiff {
    // Huella por capa de cada lado: las capas identicas no se aplanan de
    // nuevo ni pasan por el XOR (en layouts grandes es casi todo el tiempo).
    // Quien ya la calculo (para descartar cells iguales) la pasa.
    let own;
    let prints = match (prints, ca.zip(cb)) {
        (Some(p), _) => Some(p),
        (None, Some((a, b))) => {
            own = pair_prints(a, b, None);
            Some(&own)
        }
        (None, None) => None,
    };
    // Las capas que difieren, en paralelo; `collect` conserva el orden de
    // `layers`, asi la salida no depende de que hilo termina primero.
    let keys: Vec<&LayerKey> = layers
        .iter()
        .filter(|key| prints.is_none_or(|p| p.a.get(*key) != p.b.get(*key)))
        .collect();
    // Las instancias de cada lado, para atribuir los poligonos del XOR; se
    // arman una vez por cell (la primera capa que las necesita).
    let origins = (std::sync::OnceLock::new(), std::sync::OnceLock::new());
    let per_layer: Vec<(Option<(Vec<GdsGeomDiff>, LayerPolygons)>, bool)> = keys
        .par_iter()
        .map(|key| diff_layer(name, ca, cb, **key, unit_factor, cfg, prints, &origins))
        .collect();
    let mut out = CellDiff::default();
    for ((diff, failed), key) in per_layer.into_iter().zip(&keys) {
        if failed {
            out.failed_layers.push(**key);
        }
        if let Some((geometry, polygons)) = diff {
            out.geometry.extend(geometry);
            out.polygons.push(polygons);
        }
    }
    out
}

fn diff_layer(
    name: &str,
    ca: Option<&Cell<'_>>,
    cb: Option<&Cell<'_>>,
    key: LayerKey,
    unit_factor: f64,
    cfg: &DiffConfig,
    prints: Option<&PairPrints>,
    origins: &(std::sync::OnceLock<Origins>, std::sync::OnceLock<Origins>),
) -> (Option<(Vec<GdsGeomDiff>, LayerPolygons)>, bool) {
    // Aplanado con depth=-1 + filtro por (layer, dt): atravesamos SREF/AREF y
    // no perdemos cambios en sub-cells. `failed`: Clipper falló.
    let (added, removed, failed) = match (ca, cb) {
        (Some(ca), Some(cb)) => {
            let t = std::time::Instant::now();
            let own;
            let pair = match prints {
                Some(p) => p,
                None => {
                    own = pair_prints(ca, cb, None);
                    &own
                }
            };
            let out = xor_layer(ca, cb, key, pair);
            let count = |p: &LayerPrints| p.get(&key).map_or(0, Vec::len);
            let (pa, pb) = (count(&pair.a), count(&pair.b));
            if std::env::var_os("RIKU_PROFILE").is_some() {
                eprintln!(
                    "[diff] {name} {}/{}: {} + {} polígonos, xor {:.2?} → +{} −{}",
                    key.layer,
                    key.datatype,
                    pa,
                    pb,
                    t.elapsed(),
                    out.0.len(),
                    out.1.len()
                );
            }
            out
        }
        (None, Some(cb)) => (flat_layer(cb, &key), Vec::new(), false),
        (Some(ca), None) => (Vec::new(), flat_layer(ca, &key), false),
        (None, None) => (Vec::new(), Vec::new(), false),
    };
    if added.is_empty() && removed.is_empty() {
        return (None, failed);
    }

    // Bucketear cada poligono por instancia de origen. Para `added`
    // consultamos las references del lado "after"; para `removed`, las
    // del "before". La clave es la posicion de la instancia, estable
    // entre ambos lados.
    let root = || Origin { path: vec![name.to_string()], instance_at: None };
    let mut buckets: BTreeMap<_, BucketAcc> = BTreeMap::new();
    for p in &added {
        let origin = cb.map_or_else(root, |c| origins.1.get_or_init(|| Origins::new(c)).of(p));
        let acc = buckets.entry(origin.key()).or_default();
        acc.origin.get_or_insert(origin);
        acc.added.push(p.clone());
    }
    for p in &removed {
        let origin = ca.map_or_else(root, |c| origins.0.get_or_init(|| Origins::new(c)).of(p));
        let acc = buckets.entry(origin.key()).or_default();
        acc.origin.get_or_insert(origin);
        acc.removed.push(p.clone());
    }

    let mut geometry = Vec::new();
    for (_, acc) in buckets {
        let added_area = sum_area_um2(&acc.added, unit_factor);
        let removed_area = sum_area_um2(&acc.removed, unit_factor);
        let origin = acc.origin.unwrap_or_else(root);
        let flattened = origin.path.len() > 1;
        let instance_at_um = origin.instance_at.map(|(x, y)| (x * unit_factor, y * unit_factor));
        geometry.push(GdsGeomDiff {
            cell: name.to_string(),
            origin_path: origin.path,
            instance_at_um,
            instances: usize::from(flattened),
            layer: key,
            layer_name: None,
            added_polygons: acc.added.len(),
            removed_polygons: acc.removed.len(),
            added_area_um2: added_area,
            removed_area_um2: removed_area,
            bbox_um: union_bbox_um(&acc.added, &acc.removed, unit_factor),
            cosmetic: (added_area + removed_area) < cfg.cosmetic_threshold_um2,
            flattened,
        });
    }
    (Some((geometry, LayerPolygons { layer: key, added, removed })), failed)
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
///
/// Los grupos se buscan en un mapa (celda, capa, sub-cell) → posición: antes
/// se recorría la lista de grupos por cada item, O(n²).
fn group_instances(items: Vec<GdsGeomDiff>, cfg: &DiffConfig) -> Vec<GdsGeomDiff> {
    let mut out: Vec<GdsGeomDiff> = Vec::with_capacity(items.len());
    let mut groups: HashMap<(String, LayerKey, Vec<String>), usize> = HashMap::new();
    for g in items {
        // Solo los de una instancia se agrupan, y con el primero que la tuvo.
        let key = (g.instances > 0).then(|| (g.cell.clone(), g.layer, g.origin_path.clone()));
        let same = key.as_ref().and_then(|k| groups.get(k)).map(|&i| &mut out[i]);
        match same {
            Some(o) => {
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
            None => {
                if let Some(k) = key {
                    groups.insert(k, out.len());
                }
                out.push(g);
            }
        }
    }
    out
}

/// Como cambio una cell entre dos libraries.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
        let prints: Vec<(LayerPrints, &String)> = names
            .par_iter()
            .filter_map(|n| Some((layer_prints(&lib.find_cell(n)?), n)))
            .filter(|(fp, _)| !fp.is_empty())
            .collect();
        let mut m: BTreeMap<LayerPrints, Vec<String>> = BTreeMap::new();
        for (fp, n) in prints {
            m.entry(fp).or_default().push(n.clone());
        }
        m
    };
    let (fa, fb) = rayon::join(|| by_fp(la, removed), || by_fp(lb, added));
    let layers: BTreeSet<LayerKey> = la.layers().into_iter().chain(lb.layers()).map(LayerKey::from).collect();
    let cfg = DiffConfig { cosmetic_threshold_um2: 0.0 };
    let mut out = Vec::new();
    for (fp, to) in &fb {
        let Some(from) = fa.get(fp) else { continue };
        let ([from], [to]) = (from.as_slice(), to.as_slice()) else { continue };
        let (Some(ca), Some(cb)) = (la.find_cell(from), lb.find_cell(to)) else { continue };
        if diff_one_cell(to, Some(&ca), Some(&cb), &layers, 1.0, &cfg, None).geometry.is_empty() {
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
pub(crate) fn changed_cells(lib_a: Option<&Library>, lib_b: Option<&Library>) -> BTreeMap<String, CellChange> {
    let cells = pair_cells(lib_a, lib_b);
    let mut out: BTreeMap<String, CellChange> = BTreeMap::new();
    out.extend(cells.added.into_iter().map(|n| (n, CellChange::Added)));
    out.extend(cells.removed.into_iter().map(|n| (n, CellChange::Removed)));
    out.extend(cells.renamed.into_iter().map(|(from, to)| (to, CellChange::Renamed { from })));
    if let (Some(la), Some(lb)) = (lib_a, lib_b) {
        let modified = map_changed_cells(la, lb, &cells.common, &DiffConfig::default(), |name, d| {
            (!d.geometry.is_empty()).then(|| name.to_string())
        });
        out.extend(modified.into_iter().flatten().map(|n| (n, CellChange::Modified)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lo mismo con y sin la celda de contexto que agrega KLayout al guardar.
    #[test]
    fn klayout_context_cell_is_not_a_change() {
        use gdstk_rs::{GdsTag, LibraryBuilder};
        let gds = |with_context: bool| {
            let mut b = LibraryBuilder::new("L", 1e-6, 1e-9);
            let top = b.add_cell("TOP");
            b.add_box(top, GdsTag { layer: 1, datatype: 0 }, 0.0, 0.0, 1.0, 1.0);
            if with_context {
                b.add_cell("$$$CONTEXT_INFO$$$");
            }
            let path = std::env::temp_dir().join(format!("riku-ctx-{with_context}-{}.gds", std::process::id()));
            b.build().write_gds(path.to_str().unwrap()).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            let _ = std::fs::remove_file(&path);
            bytes
        };
        let (plain, with_ctx) = (gds(false), gds(true));

        let r = diff_gds(&plain, &with_ctx).expect("diff");
        assert!(r.cells_added.is_empty() && r.cells_removed.is_empty() && r.geometry.is_empty(), "{r:?}");
        let (a, b) = (Library::from_bytes(&plain).unwrap(), Library::from_bytes(&with_ctx).unwrap());
        assert!(changed_cells(Some(&a), Some(&b)).is_empty());
        assert_eq!(crate::select_top_cell(&b).map(|c| c.name().to_string()).as_deref(), Some("TOP"));
    }

    fn proof_lib_bytes() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("external")
            .join("gdstk")
            .join("tests")
            .join("proof_lib.gds");
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn a_reference_to_a_cell_outside_the_file_is_a_warning() {
        // TOP con un rectángulo y una SREF a IO_PAD, que no está en el
        // archivo. Antes gdstk lo daba como error y no se podía comparar.
        let gds = fixture_bytes("missing_ref.gds");
        let r = diff_gds(&[], &gds).expect("un aviso no impide comparar");
        assert_eq!(r.cells_added, vec!["TOP".to_string()]);
        assert!(diff_gds(&gds, &gds).unwrap().geometry.is_empty(), "contra sí mismo no cambia nada");
        assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
        assert!(r.warnings[0].starts_with("después:") && r.warnings[0].contains("IO_PAD"), "{:?}", r.warnings);
    }

    #[test]
    fn a_cycle_of_cells_is_an_error_not_a_stack_overflow() {
        // TOP → A → B → A. Antes: aplanar TOP recursaba sin fin.
        let gds = fixture_bytes("cycle.gds");
        let err = diff_gds(&gds, &gds).unwrap_err().to_string();
        assert!(err.contains("A → B → A"), "{err}");
        assert!(check_acyclic(&Library::from_bytes(&fixture_bytes("hier_inv_a.gds")).unwrap()).is_ok());
    }

    /// TOP con `n` instancias sueltas de INV; en B, INV suma un rectángulo.
    fn many_instances(n: usize, changed: bool) -> Library {
        use gdstk_rs::{GdsTag, LibraryBuilder, Placement, Point2D};
        let mut b = LibraryBuilder::new("L", 1e-6, 1e-9);
        let top = b.add_cell("TOP");
        let inv = b.add_cell("INV");
        let m1 = GdsTag { layer: 1, datatype: 0 };
        b.add_box(inv, m1, 0.0, 0.0, 1.0, 1.0);
        if changed {
            b.add_box(inv, m1, 2.0, 0.0, 2.5, 1.0);
        }
        let side = (n as f64).sqrt().ceil() as usize;
        for k in 0..n {
            let origin = Point2D { x: (k % side) as f64 * 4.0, y: (k / side) as f64 * 2.0 };
            b.add_reference(top, inv, &Placement { origin, ..Placement::default() });
        }
        b.build()
    }

    #[test]
    fn a_cell_used_many_times_is_attributed_quickly() {
        // Cada polígono del XOR se atribuye a su instancia: antes se miraban
        // todas (n × n chequeos), ahora solo las de su zona.
        let n = 40_000;
        let (a, b) = (many_instances(n, false), many_instances(n, true));
        let t = std::time::Instant::now();
        let r = diff_libraries(Some(&a), Some(&b), &DiffConfig::default());
        eprintln!("[P3] {n} instancias: {:?}", t.elapsed());
        let top: Vec<_> = r.geometry.iter().filter(|g| g.cell == "TOP").collect();
        assert_eq!(top.len(), 1, "agrupado por instancias: {top:?}");
        assert_eq!((top[0].instances, top[0].added_polygons), (n, n));
        assert!((top[0].added_area_um2 - 0.5 * n as f64).abs() < 1e-6);
    }

    fn fixture_bytes(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn the_same_layout_in_other_units_has_no_changes() {
        let um = fixture_bytes("hier_inv_a.gds");
        let path = std::env::temp_dir().join(format!("riku-units-{}.gds", std::process::id()));
        let lib = Library::from_bytes_any_in_unit(&um, 1e-9).unwrap();
        lib.write_gds(path.to_str().unwrap()).unwrap();
        let nm = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!((Library::from_bytes(&nm).unwrap().unit() - 1e-9).abs() < 1e-18);
        for (a, b) in [(&um, &nm), (&nm, &um)] {
            let r = diff_gds(a, b).expect("diff");
            assert!(r.geometry.is_empty() && r.cells_added.is_empty() && r.cells_removed.is_empty(), "{r:?}");
            assert!(r.warnings.iter().any(|w| w.contains("unidad")), "{:?}", r.warnings);
        }
        // Con un cambio, las áreas salen en µm² igual que entre dos µm.
        let changed = diff_gds(&nm, &fixture_bytes("hier_inv_b.gds")).expect("diff");
        let same = diff_gds(&um, &fixture_bytes("hier_inv_b.gds")).expect("diff");
        assert_eq!(changed.geometry, same.geometry);
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
    fn oasis_and_gds_give_the_same_report() {
        let gds = diff_gds(&fixture_bytes("hier_inv_a.gds"), &fixture_bytes("hier_inv_b.gds")).expect("gds");
        let oas = diff_gds(&fixture_bytes("hier_inv_a.oas"), &fixture_bytes("hier_inv_b.oas")).expect("oas");
        assert!(!oas.geometry.is_empty());
        assert_eq!(gds, oas);
        // Tambien mezclados: la version A en GDSII y la B en OASIS.
        let mixed = diff_gds(&fixture_bytes("hier_inv_a.gds"), &fixture_bytes("hier_inv_b.oas")).expect("mixto");
        assert_eq!(gds, mixed);
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

    fn nand2_diff(a: &str, b: &str) -> GdsDiffReport {
        diff_layout_sides(
            LayoutSide { bytes: &fixture_bytes(a), files: None },
            LayoutSide { bytes: &fixture_bytes(b), files: None },
            "nand2.gds",
            &DiffConfig::default(),
            &crate::DiffCache::disabled(),
        )
        .expect("diff")
    }

    #[test]
    fn report_lists_shorts_and_opens() {
        use crate::nets::NetChangeKind;
        // Un li entre la entrada B y la salida Y (gen_nand2_nets.py).
        let r = nand2_diff("nand2_a.gds", "nand2_short.gds");
        assert_eq!(r.nets.len(), 1, "{:?}", r.nets);
        let n = &r.nets[0];
        assert_eq!((n.kind, n.before.clone(), n.after.clone()), (NetChangeKind::Short, vec!["B".to_string(), "Y".into()], vec!["B = Y".to_string()]));
        // Dónde: el li agregado, lo que no se superpone con el que había.
        let want = [0.43, 1.10, 0.60, 1.28];
        assert!(n.bbox_um.iter().zip(want).all(|(x, y)| (x - y).abs() < 1e-6), "{:?}", n.bbox_um);
        // Sin las vías del riel de tierra: VGND se parte en dos.
        let r = nand2_diff("nand2_a.gds", "nand2_open.gds");
        assert_eq!(r.nets.len(), 1, "{:?}", r.nets);
        let n = &r.nets[0];
        assert_eq!((n.kind, n.before.clone(), n.after.len()), (NetChangeKind::Open, vec!["VGND".to_string()], 2));
        assert!(nand2_diff("nand2_a.gds", "nand2_b.gds").nets.is_empty(), "un transistor más angosto no cambia las redes");
    }

    #[test]
    fn a_short_that_appears_only_in_the_parent_is_found() {
        use crate::nets::NetChangeKind;
        // El metal1 de PAD se ensancha y toca el de TOP (gen_context.py):
        // el cambio es de PAD, el corto aparece recién en TOP.
        let r = nand2_diff("context_a.gds", "context_b.gds");
        assert_eq!(r.nets.len(), 1, "{:?}", r.nets);
        let n = &r.nets[0];
        assert_eq!((n.cell.as_str(), n.kind, n.before.clone()), ("TOP", NetChangeKind::Short, vec!["A".to_string(), "B".into()]));
        assert!(r.warnings.iter().all(|w| !w.contains("solo cerca")), "confirmado con la celda entera: {:?}", r.warnings);
    }

    #[test]
    fn report_lists_transistors_whose_size_changed() {
        // nand2_1 de SKY130 con la difusión N más baja (gen_nand2_devices.py):
        // los dos nfet pasan de W 0,65 a 0,46 µm; los pfet no cambian.
        let r = diff_layout_sides(
            LayoutSide { bytes: &fixture_bytes("nand2_a.gds"), files: None },
            LayoutSide { bytes: &fixture_bytes("nand2_b.gds"), files: None },
            "nand2.gds",
            &DiffConfig::default(),
            &crate::DiffCache::disabled(),
        )
        .expect("diff");
        assert_eq!(r.devices.len(), 2, "{:?}", r.devices);
        for d in &r.devices {
            let (a, b) = (d.before.as_ref().expect("antes"), d.after.as_ref().expect("después"));
            assert_eq!((a.model.as_str(), b.model.as_str()), ("sky130_fd_pr__nfet_01v8", "sky130_fd_pr__nfet_01v8"));
            assert!((a.w_um - 0.65).abs() < 1e-6 && (b.w_um - 0.46).abs() < 1e-6, "{d:?}");
            assert!((b.l_um - 0.15).abs() < 1e-6);
        }
        let same = diff_layout_sides(
            LayoutSide { bytes: &fixture_bytes("nand2_a.gds"), files: None },
            LayoutSide { bytes: &fixture_bytes("nand2_a.gds"), files: None },
            "nand2.gds",
            &DiffConfig::default(),
            &crate::DiffCache::disabled(),
        )
        .expect("diff");
        assert!(same.devices.is_empty());
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

    /// GDSII mínimo con una celda `TOP` y rectángulos/polígonos en la capa 1,
    /// escritos con los vértices en el orden dado (unidades: 1 nm, 1000 = 1 µm).
    fn gds_with(polys: &[&[(i32, i32)]]) -> Vec<u8> {
        fn rec(out: &mut Vec<u8>, kind: u16, data: &[u8]) {
            out.extend_from_slice(&((4 + data.len()) as u16).to_be_bytes());
            out.extend_from_slice(&kind.to_be_bytes());
            out.extend_from_slice(data);
        }
        let i2 = |v: &[i16]| v.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>();
        let mut o = Vec::new();
        rec(&mut o, 0x0002, &i2(&[600]));
        rec(&mut o, 0x0102, &i2(&[0; 12]));
        rec(&mut o, 0x0206, b"LIB\0");
        // UNITS: 1e-3 unidades de usuario (µm) y 1e-9 m por unidad de base.
        let units = [0x3E41_8937_4BC6_A7EFu64, 0x3944_B82F_A09B_5A51u64];
        rec(&mut o, 0x0305, &units.iter().flat_map(|u| u.to_be_bytes()).collect::<Vec<u8>>());
        rec(&mut o, 0x0502, &i2(&[0; 12]));
        rec(&mut o, 0x0606, b"TOP\0");
        for pts in polys {
            rec(&mut o, 0x0800, &[]);
            rec(&mut o, 0x0D02, &i2(&[1]));
            rec(&mut o, 0x0E02, &i2(&[0]));
            let xy: Vec<u8> =
                pts.iter().chain(pts.first()).flat_map(|(x, y)| [x.to_be_bytes(), y.to_be_bytes()].concat()).collect();
            rec(&mut o, 0x1003, &xy);
            rec(&mut o, 0x1100, &[]);
        }
        rec(&mut o, 0x0700, &[]);
        rec(&mut o, 0x0400, &[]);
        o
    }

    /// Cuadrado de lado 1000 con esquina en (x, 0), antihorario desde (x, 0).
    fn sq(x: i32) -> [(i32, i32); 4] {
        [(x, 0), (x + 1000, 0), (x + 1000, 1000), (x, 1000)]
    }

    #[test]
    fn reexported_polygons_are_not_a_change() {
        // B escribe los mismos cuadrados empezando en otro vértice y en sentido
        // horario: la misma geometría, sin cambios (y sin pasar por el XOR).
        let a: Vec<[(i32, i32); 4]> = (0..20).map(|i| sq(i * 2000)).collect();
        let b: Vec<[(i32, i32); 4]> = a.iter().map(|s| [s[2], s[1], s[0], s[3]]).collect();
        let (ra, rb): (Vec<&[(i32, i32)]>, Vec<&[(i32, i32)]>) =
            (a.iter().map(|s| &s[..]).collect(), b.iter().map(|s| &s[..]).collect());
        let r = diff_gds(&gds_with(&ra), &gds_with(&rb)).expect("diff");
        assert!(r.geometry.is_empty(), "{:?}", r.geometry);
        assert!(changed_cells(
            Some(&Library::from_bytes_any(&gds_with(&ra)).unwrap()),
            Some(&Library::from_bytes_any(&gds_with(&rb)).unwrap())
        )
        .is_empty());
    }

    #[test]
    fn local_xor_subtracts_the_common_polygons_it_touches() {
        // 20 cuadrados comunes (reescritos en B) y uno que se mueve 0,5 µm a
        // la derecha, pegado a un común que lo cubre en parte: el XOR solo ve
        // lo que cambió y su entorno, y el común resta lo que tapa.
        //   A: móvil en [100000, 101000], común en [100500, 101500]
        //   B: móvil en [100500, 101500]  → unión A = [100000, 101500], unión B = [100500, 101500]
        //   ⇒ eliminado 0,5 × 1 µm², añadido 0.
        let common: Vec<[(i32, i32); 4]> = (0..20).map(|i| sq(i * 2000)).chain([sq(100_500)]).collect();
        let moved_a = sq(100_000);
        let moved_b = sq(100_500);
        let mut a: Vec<&[(i32, i32)]> = common.iter().map(|s| &s[..]).collect();
        a.push(&moved_a);
        let reversed: Vec<[(i32, i32); 4]> = common.iter().map(|s| [s[3], s[2], s[1], s[0]]).collect();
        let mut b: Vec<&[(i32, i32)]> = reversed.iter().map(|s| &s[..]).collect();
        b.push(&moved_b);

        let r = diff_gds(&gds_with(&a), &gds_with(&b)).expect("diff");
        let added: f64 = r.geometry.iter().map(|g| g.added_area_um2).sum();
        let removed: f64 = r.geometry.iter().map(|g| g.removed_area_um2).sum();
        assert!(added.abs() < 1e-9, "añadido {added}");
        assert!((removed - 0.5).abs() < 1e-9, "eliminado {removed}");

        // Y da lo mismo que el XOR de la capa entera.
        let (la, lb) = (Library::from_bytes_any(&gds_with(&a)).unwrap(), Library::from_bytes_any(&gds_with(&b)).unwrap());
        let (ca, cb) = (la.find_cell("TOP").unwrap(), lb.find_cell("TOP").unwrap());
        let (fa, fb) = (ca.get_polygons().with_filter(1, 0).build(), cb.get_polygons().with_filter(1, 0).build());
        let full = gdstk_rs::xor_split_flat(&fa, &fb);
        let key = LayerKey { layer: 1, datatype: 0 };
        let (add, rem, _) = xor_layer(&ca, &cb, key, &pair_prints(&ca, &cb, None));
        assert!((sum_area_um2(&add, 1.0) - sum_area_um2(&full.added, 1.0)).abs() < 1e-9);
        assert!((sum_area_um2(&rem, 1.0) - sum_area_um2(&full.removed, 1.0)).abs() < 1e-9);
    }

    /// `gds_with` más un PATH de ancho 200 en la capa 5 (que no tiene
    /// polígonos), antes del ENDSTR de `TOP`.
    fn with_path(mut gds: Vec<u8>, pts: &[(i32, i32)]) -> Vec<u8> {
        let mut p = Vec::new();
        let mut rec = |kind: u16, data: Vec<u8>| {
            p.extend_from_slice(&((4 + data.len()) as u16).to_be_bytes());
            p.extend_from_slice(&kind.to_be_bytes());
            p.extend(data);
        };
        rec(0x0900, vec![]);
        rec(0x0D02, 5i16.to_be_bytes().to_vec());
        rec(0x0E02, 0i16.to_be_bytes().to_vec());
        rec(0x0F03, 200i32.to_be_bytes().to_vec());
        rec(0x1003, pts.iter().flat_map(|(x, y)| [x.to_be_bytes(), y.to_be_bytes()].concat()).collect());
        rec(0x1100, vec![]);
        // Los últimos 8 bytes son ENDSTR y ENDLIB.
        let at = gds.len() - 8;
        gds.splice(at..at, p);
        gds
    }

    #[test]
    fn change_in_a_layer_drawn_only_with_paths_is_reported() {
        let square = sq(0);
        let base = gds_with(&[&square[..]]);
        let a = with_path(base.clone(), &[(0, 5000), (5000, 5000)]);
        let b = with_path(base, &[(0, 5000), (6000, 5000)]);
        let r = diff_gds(&a, &b).expect("diff");
        let layers: Vec<LayerKey> = r.geometry.iter().map(|g| g.layer).collect();
        assert_eq!(layers, vec![LayerKey { layer: 5, datatype: 0 }], "{:?}", r.geometry);
        let added: f64 = r.geometry.iter().map(|g| g.added_area_um2).sum();
        assert!((added - 1.0 * 0.2).abs() < 1e-9, "añadido {added}");
    }
}
