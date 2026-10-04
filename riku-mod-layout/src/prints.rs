//! Huella por capa y XOR de una capa guiado por la huella, en paralelo.
//!
//! Aplanar una celda es, en gdstk, lo propio de la celda más lo de cada
//! referencia. Acá se hace por esos pedazos: cada uno se aplana, se usa y se
//! suelta, así nunca está la celda entera aplanada en memoria (un layout de
//! 42 MB ocupa 1,2 GB aplanado; por pedazos, unos cientos de MB) y los
//! pedazos se reparten entre los hilos del pool de `rayon`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Condvar, Mutex, OnceLock};

use crate::box_grid::BoxGrid;
use gdstk_rs::{xor_split_owned, Cell, FlattenedPolygons, OwnedPolygon, Polygon, Reference};
use rayon::prelude::*;

use crate::gds_diff::LayerKey;

/// Huella de la geometría aplanada de una cell, por capa: hash de cada
/// polígono en forma canónica, ordenado. Independiente del orden de los
/// polígonos. Dos capas con la misma huella dan un XOR vacío.
pub(crate) type LayerPrints = BTreeMap<LayerKey, Vec<u64>>;

/// Memoria que ocupa un polígono aplanado por gdstk (medido: 197 B en un
/// layout de IHP con 6,2 millones de polígonos).
const BYTES_PER_POLYGON: u64 = 200;

/// Los pedazos del aplanado de una cell: el pedazo 0 es lo propio
/// (polígonos y paths, `depth(0)`) y el `i` es la referencia `i - 1`
/// aplanada entera. Juntos dan los mismos polígonos que
/// `cell.get_polygons()`, que en el C++ hace exactamente eso.
struct Pieces<'a> {
    cell: Cell<'a>,
    refs: Vec<Reference<'a>>,
    filter: Option<LayerKey>,
}

impl<'a> Pieces<'a> {
    fn new(cell: &Cell<'a>, filter: Option<LayerKey>) -> Self {
        Self { cell: *cell, refs: cell.references().collect(), filter }
    }

    fn len(&self) -> usize {
        self.refs.len() + 1
    }

    fn flatten(&self, i: usize) -> FlattenedPolygons<'a> {
        let b = match i {
            0 => self.cell.get_polygons().depth(0),
            _ => self.refs[i - 1].get_polygons(),
        };
        match self.filter {
            Some(k) => b.with_filter(k.layer, k.datatype),
            None => b,
        }
        .build()
    }
}

/// Vértice cuantizado a 1e-6 unidades de usuario: muy por debajo de la
/// grilla de cualquier PDK, estable ante ruido de punto flotante.
fn quantize(q: gdstk_rs::Point2D) -> (i64, i64) {
    ((q.x * 1e6).round() as i64, (q.y * 1e6).round() as i64)
}

/// Vértices del polígono en forma canónica, en `v`: cuantizados, sin puntos
/// repetidos seguidos ni el de cierre, en sentido antihorario y empezando por
/// el menor. Dos polígonos con la misma forma canónica cubren la misma
/// región aunque el archivo los escriba distinto (otro vértice de inicio u
/// otro sentido de giro, típico al reexportar con otra herramienta); con la
/// regla nonzero de gdstk, invertir el sentido no cambia lo que se rellena.
fn canonical_points(p: &Polygon<'_>, v: &mut Vec<(i64, i64)>) {
    v.clear();
    v.extend(p.points().map(quantize));
    v.dedup();
    if v.len() > 1 && v.first() == v.last() {
        v.pop();
    }
    let n = v.len();
    let area2: i128 = (0..n)
        .map(|i| {
            let (a, b) = (v[i], v[(i + 1) % n]);
            a.0 as i128 * b.1 as i128 - b.0 as i128 * a.1 as i128
        })
        .sum();
    if area2 < 0 {
        v.reverse();
    }
    if let Some(k) = (0..n).min_by_key(|&i| v[i]) {
        v.rotate_left(k);
    }
}

/// Hash de la forma canónica; `buf` se reutiliza entre llamadas.
pub(crate) fn polygon_hash(p: &Polygon<'_>, buf: &mut Vec<(i64, i64)>) -> u64 {
    use std::hash::{Hash, Hasher};
    canonical_points(p, buf);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (p.layer(), p.datatype(), &buf[..]).hash(&mut h);
    h.finish()
}

fn key_of(p: &Polygon<'_>) -> LayerKey {
    LayerKey { layer: p.layer(), datatype: p.datatype() }
}

fn to_owned_polygon(p: &Polygon<'_>) -> OwnedPolygon {
    OwnedPolygon { layer: p.layer(), datatype: p.datatype(), points: p.points().collect() }
}

fn bbox(p: &Polygon<'_>) -> [f64; 4] {
    let b = p.bbox();
    [b.min_x, b.min_y, b.max_x, b.max_y]
}

/// Huella por capa de `cell`, aplanando por pedazos en paralelo.
pub(crate) fn layer_prints(cell: &Cell<'_>) -> LayerPrints {
    let pieces = Pieces::new(cell, None);
    let all: Vec<usize> = (0..pieces.len()).collect();
    prints_of(&pieces, &all)
}

/// Huella por capa de los pedazos `which` de `pieces`.
fn prints_of(pieces: &Pieces<'_>, which: &[usize]) -> LayerPrints {
    let mut out = which
        .par_iter()
        .fold(
            || (LayerPrints::new(), Vec::new()),
            |(mut acc, mut buf), &i| {
                for p in pieces.flatten(i).polygons() {
                    acc.entry(key_of(&p)).or_default().push(polygon_hash(&p, &mut buf));
                }
                (acc, buf)
            },
        )
        .map(|(acc, _)| acc)
        .reduce(LayerPrints::new, |mut a, b| {
            for (k, mut v) in b {
                let e = a.entry(k).or_default();
                if e.len() < v.len() {
                    std::mem::swap(e, &mut v);
                }
                e.extend(v);
            }
            a
        });
    out.par_iter_mut().for_each(|(_, v)| v.sort_unstable());
    out
}

/// Huella jerárquica de cada cell de una librería: un árbol de Merkle sobre
/// el grafo de la jerarquía, como los árboles de Git. La de una cell combina
/// la huella de su geometría propia (polígonos y paths, sin bajar a las
/// referencias) con, por cada referencia, la huella jerárquica de la cell
/// referenciada y su transformación (origen, rotación, escala, reflejo y
/// repetición). Dos cells con la misma huella jerárquica tienen el mismo
/// aplanado, sin aplanar nada: solo se miran los polígonos propios (en un
/// layout de 42 MB, 655 mil en lugar de 13,8 millones).
pub(crate) struct TreePrints(HashMap<String, CellTree>);

#[derive(Clone, Copy)]
struct CellTree {
    /// Huella de la geometría propia (el pedazo 0).
    own: u64,
    /// `None` si no se puede decidir barato (una repetición explícita enorme
    /// o una referencia circular): entonces se compara aplanando.
    tree: Option<u64>,
}

impl TreePrints {
    /// `true` si `name` tiene la misma huella jerárquica en `self` y en
    /// `other` (el mismo aplanado). Sin huella de algún lado: `false`.
    pub(crate) fn same(&self, other: &TreePrints, name: &str) -> bool {
        matches!((self.0.get(name), other.0.get(name)), (Some(a), Some(b)) if a.tree.is_some() && a.tree == b.tree)
    }

    /// Clave de cada pedazo de `pieces` para emparejarlo con la otra
    /// versión: dos pedazos con la misma clave aplanan a lo mismo. El pedazo
    /// 0 es la geometría propia; el de una referencia, la huella jerárquica
    /// de su cell y su transformación. `None`: sin gemelo posible.
    fn piece_keys(&self, pieces: &Pieces<'_>) -> Vec<Option<u64>> {
        use std::hash::{Hash, Hasher};
        let own = self.0.get(pieces.cell.name()).map(|c| c.own);
        std::iter::once(own)
            .chain(pieces.refs.iter().map(|r| {
                let child = self.0.get(r.cell_name())?.tree?;
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (child, transform_hash(r)?).hash(&mut h);
                Some(h.finish())
            }))
            .collect()
    }
}

pub(crate) fn tree_prints(lib: &gdstk_rs::Library) -> TreePrints {
    use std::hash::{Hash, Hasher};
    type Local = (u64, Option<Vec<(String, u64)>>);

    let cells: Vec<Cell<'_>> = lib.cells().collect();
    let local: Vec<Local> = cells
        .par_iter()
        .map_init(Vec::new, |buf, cell| {
            let mut own: Vec<u64> = cell.get_polygons().depth(0).build().polygons().map(|p| polygon_hash(&p, buf)).collect();
            own.sort_unstable();
            let mut h = std::collections::hash_map::DefaultHasher::new();
            own.hash(&mut h);
            let refs = cell.references().map(|r| Some((r.cell_name().to_string(), transform_hash(&r)?))).collect();
            (h.finish(), refs)
        })
        .collect();

    let index: HashMap<&str, usize> = cells.iter().enumerate().map(|(i, c)| (c.name(), i)).collect();
    let mut memo: Vec<Option<Option<u64>>> = vec![None; cells.len()];
    let mut visiting = vec![false; cells.len()];

    fn resolve(
        i: usize,
        local: &[Local],
        index: &HashMap<&str, usize>,
        memo: &mut Vec<Option<Option<u64>>>,
        visiting: &mut Vec<bool>,
    ) -> Option<u64> {
        if let Some(done) = memo[i] {
            return done;
        }
        if visiting[i] {
            return None;
        }
        visiting[i] = true;
        let (own, refs) = &local[i];
        let result = refs.as_ref().and_then(|refs| {
            let mut children: Vec<(u64, u64)> = Vec::with_capacity(refs.len());
            for (name, transform) in refs {
                // Una referencia a algo que no es una cell de la librería
                // (sin resolver o a una RawCell) no aporta polígonos: cuenta
                // su nombre.
                let child = match index.get(name.as_str()) {
                    Some(&j) => resolve(j, local, index, memo, visiting)?,
                    None => {
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        name.hash(&mut h);
                        h.finish()
                    }
                };
                children.push((child, *transform));
            }
            children.sort_unstable();
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (own, children).hash(&mut h);
            Some(h.finish())
        });
        visiting[i] = false;
        memo[i] = Some(result);
        result
    }

    TreePrints(
        (0..cells.len())
            .map(|i| {
                let tree = resolve(i, &local, &index, &mut memo, &mut visiting);
                (cells[i].name().to_string(), CellTree { own: local[i].0, tree })
            })
            .collect(),
    )
}

/// Huella por capa de dos versiones de una cell, contando solo los pedazos
/// sin gemelo en la otra (los gemelos aportan lo mismo a los dos lados y se
/// cancelan). Las capas con la misma huella acá son iguales en el aplanado
/// completo; las que difieren se comparan con [`xor_layer`].
pub(crate) struct PairPrints {
    pub(crate) a: LayerPrints,
    pub(crate) b: LayerPrints,
    only_a: Vec<usize>,
    only_b: Vec<usize>,
}

/// Empareja los pedazos de `ca` y `cb` por su clave (ver
/// [`TreePrints::piece_keys`]) y calcula la huella de los que sobran. Sin
/// huellas jerárquicas, ningún pedazo se empareja: es la huella completa.
pub(crate) fn pair_prints(ca: &Cell<'_>, cb: &Cell<'_>, trees: Option<(&TreePrints, &TreePrints)>) -> PairPrints {
    let (pa, pb) = (Pieces::new(ca, None), Pieces::new(cb, None));
    let (only_a, only_b) = match trees {
        Some((ta, tb)) => unmatched(&ta.piece_keys(&pa), &tb.piece_keys(&pb)),
        None => ((0..pa.len()).collect(), (0..pb.len()).collect()),
    };
    let (a, b) = rayon::join(|| prints_of(&pa, &only_a), || prints_of(&pb, &only_b));
    PairPrints { a, b, only_a, only_b }
}

impl PairPrints {
    /// `true` si las dos versiones aplanan a la misma geometría.
    pub(crate) fn same(&self) -> bool {
        self.a == self.b
    }
}

/// Índices de los pedazos sin gemelo de cada lado, emparejando claves como
/// multiconjunto. Una clave `None` nunca se empareja.
fn unmatched(ka: &[Option<u64>], kb: &[Option<u64>]) -> (Vec<usize>, Vec<usize>) {
    let mut left: HashMap<u64, usize> = HashMap::new();
    for k in ka.iter().flatten() {
        *left.entry(*k).or_default() += 1;
    }
    let mut only_b = Vec::new();
    for (j, k) in kb.iter().enumerate() {
        match k.and_then(|k| left.get_mut(&k).filter(|n| **n > 0)) {
            Some(n) => *n -= 1,
            None => only_b.push(j),
        }
    }
    let only_a = ka
        .iter()
        .enumerate()
        .filter(|(_, k)| match k.and_then(|k| left.get_mut(&k).filter(|n| **n > 0)) {
            Some(n) => {
                *n -= 1;
                true
            }
            None => k.is_none(),
        })
        .map(|(i, _)| i)
        .collect();
    (only_a, only_b)
}

/// Repeticiones explícitas más grandes que esto no se recorren (leer cada
/// offset cuesta O(n) en gdstk-rs): la cell se compara aplanando.
const MAX_EXPLICIT_OFFSETS: u64 = 4096;

/// Hash de la transformación de una referencia, cuantizada como los
/// vértices. `None` si la repetición es explícita y enorme.
pub(crate) fn transform_hash(r: &Reference<'_>) -> Option<u64> {
    use gdstk_rs::RepetitionType as K;
    use std::hash::{Hash, Hasher};
    let q = |v: f64| (v * 1e6).round() as i64;
    let qp = |p: gdstk_rs::Point2D| (q(p.x), q(p.y));
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (qp(r.origin()), (r.rotation() * 1e9).round() as i64, (r.magnification() * 1e9).round() as i64, r.x_reflection())
        .hash(&mut h);
    let rep = r.repetition();
    let kind = rep.kind();
    (kind as u8).hash(&mut h);
    match kind {
        K::None => {}
        K::Rectangular => (rep.columns(), rep.rows(), qp(rep.spacing())).hash(&mut h),
        K::Regular => (rep.columns(), rep.rows(), qp(rep.v1()), qp(rep.v2())).hash(&mut h),
        K::ExplicitX | K::ExplicitY => {
            for c in rep.coords() {
                q(c).hash(&mut h);
            }
        }
        K::Explicit => {
            if rep.count() > MAX_EXPLICIT_OFFSETS {
                return None;
            }
            for p in rep.offsets() {
                qp(p).hash(&mut h);
            }
        }
    }
    Some(h.finish())
}

/// Diferencia de dos multiconjuntos ordenados: lo que sobra en cada uno.
fn multiset_diff(a: &[u64], b: &[u64]) -> (Vec<u64>, Vec<u64>) {
    let (mut i, mut j) = (0, 0);
    let (mut only_a, mut only_b) = (Vec::new(), Vec::new());
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                only_a.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                only_b.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    only_a.extend_from_slice(&a[i..]);
    only_b.extend_from_slice(&b[j..]);
    (only_a, only_b)
}

/// Polígonos de los pedazos `which` cuyo hash está en `own` (con sus
/// repeticiones), en el orden de los pedazos: los propios de ese lado.
/// Devuelve también su posición `(pedazo, índice)` para no tomarlos después
/// como comunes.
fn pick_own(pieces: &Pieces<'_>, which: &[usize], own: &[u64]) -> (Vec<OwnedPolygon>, HashSet<(usize, u64)>) {
    let mut left: HashMap<u64, usize> = HashMap::with_capacity(own.len());
    for h in own {
        *left.entry(*h).or_default() += 1;
    }
    // En paralelo: candidatos (hash en `own`); en orden: se toman tantos
    // como repeticiones tenga cada hash.
    let candidates: Vec<(usize, Vec<(u64, u64, OwnedPolygon)>)> = which
        .par_iter()
        .map_init(Vec::new, |buf, &i| {
            let flat = pieces.flatten(i);
            let list = flat
                .polygons()
                .enumerate()
                .filter_map(|(k, p)| {
                    let h = polygon_hash(&p, buf);
                    left.contains_key(&h).then(|| (k as u64, h, to_owned_polygon(&p)))
                })
                .collect();
            (i, list)
        })
        .collect();
    let mut polys = Vec::new();
    let mut at = HashSet::new();
    for (i, list) in candidates {
        for (k, h, p) in list {
            if let Some(n) = left.get_mut(&h).filter(|n| **n > 0) {
                *n -= 1;
                polys.push(p);
                at.insert((i, k));
            }
        }
    }
    (polys, at)
}

/// XOR de una capa (`added = B \ A`, `removed = A \ B`) usando las huellas
/// de los dos lados para no pasarle a Clipper la capa entera.
///
/// Los hashes que sobran en cada huella son los polígonos propios de cada
/// lado (`A'`, `B'`); el resto son comunes (`C`). Vale
/// `XOR(A, B) = XOR(A', B') \ C`, y de `C` solo importan los polígonos que
/// tocan la zona de `A' ∪ B'` (`Cl`): el resultado es
/// `XOR(A' ∪ Cl, B' ∪ Cl)`. La geometría es la del XOR completo; Clipper solo
/// ve lo que cambió y su entorno.
///
/// Los propios salen solo de los pedazos sin gemelo (ver [`pair_prints`]):
/// los gemelos son comunes enteros. Para `Cl` se recorren los pedazos sin
/// gemelo de A y, de los gemelos, solo las referencias cuyo bbox toca la
/// zona del cambio. Nunca se guarda la capa entera.
///
/// El tercer valor es `true` si Clipper falló en algún cuadrante: el
/// resultado puede estar incompleto.
pub(crate) fn xor_layer(
    ca: &Cell<'_>,
    cb: &Cell<'_>,
    key: LayerKey,
    pair: &PairPrints,
) -> (Vec<OwnedPolygon>, Vec<OwnedPolygon>, bool) {
    let empty = Vec::new();
    let (pa, pb) = (pair.a.get(&key).unwrap_or(&empty), pair.b.get(&key).unwrap_or(&empty));
    let (own_a, own_b) = multiset_diff(pa, pb);
    if own_a.is_empty() && own_b.is_empty() {
        return (Vec::new(), Vec::new(), false);
    }
    // Si la capa se regeneró entera, lo propio es casi todo: que no se junten
    // varias de esas a la vez.
    let _permit = budget().acquire((own_a.len() + own_b.len()) as u64 * BYTES_PER_POLYGON);

    let profile = std::env::var_os("RIKU_PROFILE").is_some();
    let t = std::time::Instant::now();

    let (sa, sb) = (Pieces::new(ca, Some(key)), Pieces::new(cb, Some(key)));
    let ((a_own, a_at), (b_own, _)) = rayon::join(|| pick_own(&sa, &pair.only_a, &own_a), || pick_own(&sb, &pair.only_b, &own_b));

    // Comunes de A que tocan lo propio de algún lado.
    let targets: Vec<[f64; 4]> = a_own.iter().chain(&b_own).map(owned_bbox).collect();
    let grid = BoxGrid::new(&targets);
    let unmatched: HashSet<usize> = pair.only_a.iter().copied().collect();
    let scan: Vec<usize> = (0..sa.len())
        .filter(|&i| {
            // El pedazo 0 (lo propio) se recorre siempre; una referencia
            // gemela, solo si toca la zona del cambio.
            i == 0 || unmatched.contains(&i) || {
                let b = sa.refs[i - 1].bbox();
                grid.touches([b.min_x, b.min_y, b.max_x, b.max_y])
            }
        })
        .collect();
    let t_own = t.elapsed();
    let local: Vec<OwnedPolygon> = scan
        .par_iter()
        .flat_map_iter(|&i| {
            let flat = sa.flatten(i);
            flat.polygons()
                .enumerate()
                .filter(|(k, p)| !a_at.contains(&(i, *k as u64)) && grid.touches(bbox(p)))
                .map(|(_, p)| to_owned_polygon(&p))
                .collect::<Vec<_>>()
        })
        .collect();

    let t_local = t.elapsed();
    let (n_a, n_b, n_local, n_scan) = (a_own.len(), b_own.len(), local.len(), scan.len());
    let side = |own: Vec<OwnedPolygon>| -> Vec<OwnedPolygon> { own.into_iter().chain(local.iter().cloned()).collect() };
    let (added, removed, leaves, failed) = tiled_xor(&side(a_own), &side(b_own), key, LEAF_POLYGONS);
    if profile {
        eprintln!(
            "[xor] {}/{}: propios {n_a} + {n_b} en {:.2?} · comunes cerca {n_local} (de {n_scan} pedazos) en {:.2?} · clipper {:.2?} en {leaves} cuadrantes",
            key.layer,
            key.datatype,
            t_own,
            t_local - t_own,
            t.elapsed() - t_local
        );
    }
    (added, removed, failed)
}

/// Polígonos por cuadrante del XOR: con menos, una sola llamada a Clipper.
const LEAF_POLYGONS: usize = 1000;
/// Niveles máximos del quadtree (4^8 = 65 536 cuadrantes).
const MAX_DEPTH: u32 = 8;

/// XOR (`added = B \ A`, `removed = A \ B`) partiendo el plano en un
/// quadtree por bbox hasta que cada hoja tenga a lo sumo `leaf` polígonos
/// (de los dos lados). Cada hoja hace su XOR en paralelo y recorta el
/// resultado a su rectángulo: `XOR(A, B) ∩ T = XOR(A ∩ T, B ∩ T)`, así la
/// unión de las hojas es el XOR completo. Clipper barre por franjas y su
/// costo crece mucho más rápido que la cantidad de bordes que comparten una
/// franja: con miles de rectángulos alineados (capa 19/0 de un chip de IHP,
/// 124 mil) una llamada tarda 358 s y 256 cuadrantes, 4,9 s.
///
/// Un polígono de diferencia que cruza un borde de cuadrante sale partido;
/// las áreas y los bbox no cambian. Devuelve también la cantidad de hojas
/// y si Clipper falló en alguna.
fn tiled_xor(
    a: &[OwnedPolygon],
    b: &[OwnedPolygon],
    key: LayerKey,
    leaf: usize,
) -> (Vec<OwnedPolygon>, Vec<OwnedPolygon>, usize, bool) {
    if a.len() + b.len() <= 2 * leaf {
        let split = xor_split_owned(a, b, key.into());
        return (split.added, split.removed, 1, split.error.is_some());
    }
    let (ba, bb): (Vec<[f64; 4]>, Vec<[f64; 4]>) = (a.iter().map(owned_bbox).collect(), b.iter().map(owned_bbox).collect());
    let bounds = ba.iter().chain(&bb).fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |r, x| {
        [r[0].min(x[0]), r[1].min(x[1]), r[2].max(x[2]), r[3].max(x[3])]
    });
    let mut leaves: Vec<([f64; 4], Vec<u32>, Vec<u32>)> = Vec::new();
    let sides = Sides { a: &ba, b: &bb, leaf };
    sides.split(bounds, (0..a.len() as u32).collect(), (0..b.len() as u32).collect(), 0, &mut leaves);
    let n = leaves.len();
    let parts: Vec<(Vec<OwnedPolygon>, Vec<OwnedPolygon>, bool)> = leaves
        .par_iter()
        .map(|(rect, ia, ib)| {
            let pick = |polys: &[OwnedPolygon], idx: &[u32]| -> Vec<OwnedPolygon> {
                idx.iter().map(|&i| polys[i as usize].clone()).collect()
            };
            let split = xor_split_owned(&pick(a, ia), &pick(b, ib), key.into());
            let cut = |v: Vec<OwnedPolygon>| -> Vec<OwnedPolygon> { v.iter().filter_map(|p| clip_to_rect(p, rect)).collect() };
            let failed = split.error.is_some();
            (cut(split.added), cut(split.removed), failed)
        })
        .collect();
    let (mut added, mut removed, mut failed) = (Vec::new(), Vec::new(), false);
    for (x, y, f) in parts {
        added.extend(x);
        removed.extend(y);
        failed |= f;
    }
    (added, removed, n, failed)
}

/// Los bbox de los polígonos de cada lado, para armar el quadtree.
struct Sides<'a> {
    a: &'a [[f64; 4]],
    b: &'a [[f64; 4]],
    leaf: usize,
}

impl Sides<'_> {
    /// Parte `rect` en 4 mientras tenga más de `leaf` polígonos (índices de
    /// A y de B cuyo bbox lo toca) y no se haya llegado a [`MAX_DEPTH`]. Las
    /// hojas sin polígonos no se guardan.
    fn split(&self, rect: [f64; 4], ia: Vec<u32>, ib: Vec<u32>, depth: u32, out: &mut Vec<([f64; 4], Vec<u32>, Vec<u32>)>) {
        if ia.is_empty() && ib.is_empty() {
            return;
        }
        if ia.len() + ib.len() <= self.leaf || depth >= MAX_DEPTH {
            out.push((rect, ia, ib));
            return;
        }
        let (mx, my) = ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        let quads =
            [[rect[0], rect[1], mx, my], [mx, rect[1], rect[2], my], [rect[0], my, mx, rect[3]], [mx, my, rect[2], rect[3]]];
        for q in quads {
            let touches = |b: &[f64; 4]| b[0] <= q[2] && b[2] >= q[0] && b[1] <= q[3] && b[3] >= q[1];
            let sub_a: Vec<u32> = ia.iter().copied().filter(|&i| touches(&self.a[i as usize])).collect();
            let sub_b: Vec<u32> = ib.iter().copied().filter(|&i| touches(&self.b[i as usize])).collect();
            self.split(q, sub_a, sub_b, depth + 1, out);
        }
    }
}

/// Recorta un polígono a un rectángulo (Sutherland–Hodgman: vale para
/// cualquier polígono contra una región convexa). `None` si no queda área.
fn clip_to_rect(p: &OwnedPolygon, r: &[f64; 4]) -> Option<OwnedPolygon> {
    use gdstk_rs::Point2D;
    let mut pts: Vec<Point2D> = p.points.clone();
    for side in 0..4 {
        let inside = |q: &Point2D| match side {
            0 => q.x >= r[0],
            1 => q.x <= r[2],
            2 => q.y >= r[1],
            _ => q.y <= r[3],
        };
        let cross = |a: &Point2D, b: &Point2D| -> Point2D {
            if side < 2 {
                let x = if side == 0 { r[0] } else { r[2] };
                Point2D { x, y: a.y + (x - a.x) / (b.x - a.x) * (b.y - a.y) }
            } else {
                let y = if side == 2 { r[1] } else { r[3] };
                Point2D { x: a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x), y }
            }
        };
        let mut out = Vec::with_capacity(pts.len() + 4);
        for i in 0..pts.len() {
            let (a, b) = (&pts[(i + pts.len() - 1) % pts.len()], &pts[i]);
            match (inside(a), inside(b)) {
                (true, true) => out.push(*b),
                (true, false) => out.push(cross(a, b)),
                (false, true) => {
                    out.push(cross(a, b));
                    out.push(*b);
                }
                (false, false) => {}
            }
        }
        pts = out;
        if pts.len() < 3 {
            return None;
        }
    }
    // Lo que cae justo sobre un borde del cuadrante queda sin área.
    let n = pts.len();
    let area2: f64 = (0..n).map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y).sum();
    (area2.abs() > 1e-12).then(|| OwnedPolygon { layer: p.layer, datatype: p.datatype, points: pts })
}

fn owned_bbox(p: &OwnedPolygon) -> [f64; 4] {
    p.points.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, q| {
        [b[0].min(q.x), b[1].min(q.y), b[2].max(q.x), b[3].max(q.y)]
    })
}

/// Cupo de memoria para las tareas que aplanan una capa entera: tantas a la
/// vez como quepan en la mitad de la memoria disponible al arrancar. Una
/// tarea más grande que todo el cupo espera a estar sola.
struct MemBudget {
    total: u64,
    used: Mutex<u64>,
    freed: Condvar,
}

struct Permit<'a> {
    budget: &'a MemBudget,
    bytes: u64,
}

thread_local! {
    /// Cupos que tiene este hilo. Un worker de rayon que espera sus
    /// subtareas roba otras: si una pide cupo y el hilo ya tiene uno, no
    /// debe dormir en el `Condvar` (nadie liberaría el suyo: se colgaba).
    static HELD: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

impl MemBudget {
    fn acquire(&self, bytes: u64) -> Permit<'_> {
        let bytes = bytes.min(self.total);
        let nested = HELD.with(|h| h.get() > 0);
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        while !nested && *used > 0 && *used + bytes > self.total {
            used = self.freed.wait(used).unwrap_or_else(|e| e.into_inner());
        }
        *used += bytes;
        HELD.with(|h| h.set(h.get() + 1));
        Permit { budget: self, bytes }
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        HELD.with(|h| h.set(h.get() - 1));
        let mut used = self.budget.used.lock().unwrap_or_else(|e| e.into_inner());
        *used -= self.bytes;
        self.budget.freed.notify_all();
    }
}

/// `MemAvailable` de `/proc/meminfo`; 4 GiB si no se puede leer.
fn mem_available() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            let line = s.lines().find(|l| l.starts_with("MemAvailable:"))?;
            line.split_whitespace().nth(1)?.parse::<u64>().ok()
        })
        .map_or(4 << 30, |kb| kb * 1024)
}

fn budget() -> &'static MemBudget {
    static BUDGET: OnceLock<MemBudget> = OnceLock::new();
    BUDGET.get_or_init(|| MemBudget { total: (mem_available() / 2).max(1), used: Mutex::new(0), freed: Condvar::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdstk_rs::Library;

    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn proof_lib() -> Library {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../external/gdstk/tests/proof_lib.gds");
        Library::open(path.to_str().unwrap())
    }

    #[test]
    fn a_worker_holding_budget_never_sleeps_on_it() {
        // Un worker de rayon con cupo que espera sus subtareas (el par_iter
        // de tiled_xor) roba otra tarea grande, que pide cupo en el mismo
        // hilo. Antes dormía en el Condvar esperando que se liberara su
        // propio cupo: se colgaba. Acá, lo mismo sin depender del robo.
        let budget = MemBudget { total: 100, used: Mutex::new(0), freed: Condvar::new() };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            {
                let _outer = budget.acquire(80);
                let _stolen = budget.acquire(80);
            }
            let _ = tx.send(*budget.used.lock().unwrap());
        });
        let used = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("se colgó");
        assert_eq!(used, 0, "todo el cupo devuelto");
        // Otro hilo sin cupo sí espera: el tope sigue valiendo.
        let budget = std::sync::Arc::new(MemBudget { total: 100, used: Mutex::new(0), freed: Condvar::new() });
        let held = budget.acquire(80);
        let b = budget.clone();
        let waiter = std::thread::spawn(move || drop(b.acquire(80)));
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!waiter.is_finished(), "sin cupo, espera");
        drop(held);
        waiter.join().unwrap();
    }

    /// La huella de toda la cell aplanada de una vez, como antes de 6.4.
    fn whole_prints(cell: &Cell<'_>) -> LayerPrints {
        let mut out = LayerPrints::new();
        let mut buf = Vec::new();
        for p in cell.get_polygons().build().polygons() {
            out.entry(key_of(&p)).or_default().push(polygon_hash(&p, &mut buf));
        }
        for v in out.values_mut() {
            v.sort_unstable();
        }
        out
    }

    #[test]
    fn prints_by_pieces_equal_the_whole_flatten() {
        // proof_lib tiene paths, AREF y jerarquía anidada; los fixtures, SREF.
        let libs = [
            proof_lib(),
            Library::from_bytes_any(&fixture("hier_inv_a.gds")).unwrap(),
            Library::from_bytes_any(&fixture("multi_inst_b.gds")).unwrap(),
            Library::from_bytes_any(&fixture("hier_inv_a.oas")).unwrap(),
        ];
        let mut checked = 0;
        for lib in &libs {
            for cell in lib.cells() {
                let whole = whole_prints(&cell);
                assert_eq!(layer_prints(&cell), whole, "{}", cell.name());
                checked += usize::from(!whole.is_empty());
            }
        }
        assert!(checked > 10, "{checked} cells con geometría");
    }

    #[test]
    fn same_tree_print_means_same_flattened_geometry() {
        // En cada par de versiones de los fixtures: huella jerárquica igual
        // ⇒ huella aplanada igual (una cell que cambió al aplanar nunca se
        // salta). Además, alguna cell se decide sin aplanar y alguna cambia.
        let pairs = [
            ("hier_inv_a.gds", "hier_inv_b.gds"),
            ("multi_inst_a.gds", "multi_inst_b.gds"),
            ("rename_a.gds", "rename_b.gds"),
            ("datatype_a.gds", "datatype_b.gds"),
            ("hier_inv_a.gds", "hier_inv_b.oas"),
        ];
        let (mut same, mut changed) = (0, 0);
        for (a, b) in pairs {
            let (la, lb) = (Library::from_bytes_any(&fixture(a)).unwrap(), Library::from_bytes_any(&fixture(b)).unwrap());
            let (ta, tb) = (tree_prints(&la), tree_prints(&lb));
            for ca in la.cells() {
                let Some(cb) = lb.find_cell(ca.name()) else { continue };
                let flat_equal = layer_prints(&ca) == layer_prints(&cb);
                let tree_equal = ta.same(&tb, ca.name());
                assert!(!tree_equal || flat_equal, "{a}/{b} {}: huella jerárquica igual con aplanado distinto", ca.name());
                same += usize::from(tree_equal);
                changed += usize::from(!flat_equal);
            }
        }
        assert!(same > 0 && changed > 0, "iguales {same}, cambiadas {changed}");
    }

    #[test]
    fn tree_print_sees_a_moved_instance() {
        // Mismo contenido en todas las cells; solo cambia dónde se instancia
        // la sub-cell: la huella jerárquica de la top cambia, la de la
        // sub-cell no.
        let gds = |x: i32| -> Vec<u8> {
            fn rec(o: &mut Vec<u8>, kind: u16, data: &[u8]) {
                o.extend_from_slice(&((4 + data.len()) as u16).to_be_bytes());
                o.extend_from_slice(&kind.to_be_bytes());
                o.extend_from_slice(data);
            }
            let i2 = |v: &[i16]| v.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>();
            let i4 = |v: &[i32]| v.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>();
            let mut o = Vec::new();
            rec(&mut o, 0x0002, &i2(&[600]));
            rec(&mut o, 0x0102, &i2(&[0; 12]));
            rec(&mut o, 0x0206, b"LIB\0");
            let units = [0x3E41_8937_4BC6_A7EFu64, 0x3944_B82F_A09B_5A51u64];
            rec(&mut o, 0x0305, &units.iter().flat_map(|u| u.to_be_bytes()).collect::<Vec<u8>>());
            rec(&mut o, 0x0502, &i2(&[0; 12]));
            rec(&mut o, 0x0606, b"SUB\0");
            rec(&mut o, 0x0800, &[]);
            rec(&mut o, 0x0D02, &i2(&[1]));
            rec(&mut o, 0x0E02, &i2(&[0]));
            rec(&mut o, 0x1003, &i4(&[0, 0, 1000, 0, 1000, 1000, 0, 1000, 0, 0]));
            rec(&mut o, 0x1100, &[]);
            rec(&mut o, 0x0700, &[]);
            rec(&mut o, 0x0502, &i2(&[0; 12]));
            rec(&mut o, 0x0606, b"TOP\0");
            rec(&mut o, 0x0A00, &[]);
            rec(&mut o, 0x1206, b"SUB\0");
            rec(&mut o, 0x1003, &i4(&[x, 0]));
            rec(&mut o, 0x1100, &[]);
            rec(&mut o, 0x0700, &[]);
            rec(&mut o, 0x0400, &[]);
            o
        };
        let (la, lb) = (Library::from_bytes(&gds(0)).unwrap(), Library::from_bytes(&gds(500)).unwrap());
        let (ta, tb) = (tree_prints(&la), tree_prints(&lb));
        assert!(ta.same(&tb, "SUB"));
        assert!(!ta.same(&tb, "TOP"));
        assert!(ta.0["TOP"].tree.is_some());
    }

    #[test]
    fn twin_pieces_give_the_same_diff_as_flattening_everything() {
        // Emparejar instancias gemelas no cambia nada: mismas capas que
        // difieren y mismas áreas del XOR que sin emparejar.
        let area = |v: &[OwnedPolygon]| -> f64 {
            v.iter()
                .map(|p| {
                    let n = p.points.len();
                    (0..n)
                        .map(|i| p.points[i].x * p.points[(i + 1) % n].y - p.points[(i + 1) % n].x * p.points[i].y)
                        .sum::<f64>()
                        .abs()
                        / 2.0
                })
                .sum()
        };
        let pairs =
            [("hier_inv_a.gds", "hier_inv_b.gds"), ("multi_inst_a.gds", "multi_inst_b.gds"), ("rename_a.gds", "rename_b.gds")];
        let mut compared = 0;
        for (a, b) in pairs {
            let (la, lb) = (Library::from_bytes_any(&fixture(a)).unwrap(), Library::from_bytes_any(&fixture(b)).unwrap());
            let (ta, tb) = (tree_prints(&la), tree_prints(&lb));
            for ca in la.cells() {
                let Some(cb) = lb.find_cell(ca.name()) else { continue };
                let (twins, all) = (pair_prints(&ca, &cb, Some((&ta, &tb))), pair_prints(&ca, &cb, None));
                assert_eq!(twins.same(), all.same(), "{a} {}", ca.name());
                for key in all.a.keys().chain(all.b.keys()) {
                    if all.a.get(key) == all.b.get(key) {
                        continue;
                    }
                    let (x, y) = (xor_layer(&ca, &cb, *key, &twins), xor_layer(&ca, &cb, *key, &all));
                    assert!(
                        (area(&x.0) - area(&y.0)).abs() < 1e-9 && (area(&x.1) - area(&y.1)).abs() < 1e-9,
                        "{a} {} {key:?}",
                        ca.name()
                    );
                    compared += 1;
                }
            }
        }
        assert!(compared > 0);
    }

    #[test]
    fn unmatched_pairs_keys_as_a_multiset() {
        let (a, b) = unmatched(&[Some(1), Some(2), Some(2), None], &[Some(2), Some(3), Some(1), None]);
        assert_eq!((a, b), (vec![1, 3], vec![1, 3]));
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> OwnedPolygon {
        let pts = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        OwnedPolygon { layer: 1, datatype: 0, points: pts.iter().map(|&(x, y)| gdstk_rs::Point2D { x, y }).collect() }
    }

    fn area(v: &[OwnedPolygon]) -> f64 {
        v.iter()
            .map(|p| {
                let n = p.points.len();
                (0..n)
                    .map(|i| p.points[i].x * p.points[(i + 1) % n].y - p.points[(i + 1) % n].x * p.points[i].y)
                    .sum::<f64>()
                    .abs()
                    / 2.0
            })
            .sum()
    }

    #[test]
    fn clip_to_rect_keeps_the_inside() {
        let r = rect(0.0, 0.0, 4.0, 2.0);
        let c = clip_to_rect(&r, &[1.0, -1.0, 3.0, 1.0]).expect("área");
        assert!((area(&[c]) - 2.0).abs() < 1e-12);
        // Fuera, o solo tocando un borde: nada.
        assert!(clip_to_rect(&r, &[5.0, 0.0, 6.0, 1.0]).is_none());
        assert!(clip_to_rect(&r, &[4.0, 0.0, 6.0, 1.0]).is_none());
    }

    #[test]
    fn tiled_xor_gives_the_same_areas_as_one_call() {
        // Una grilla de 60×60 rectángulos en A; en B los de las columnas
        // pares corridos 0,3 y uno de cada 7 borrado. Con hojas de 50
        // polígonos, el quadtree baja varios niveles y parte polígonos del
        // resultado sobre los bordes.
        let mut a = Vec::new();
        let mut b = Vec::new();
        for i in 0..60 {
            for j in 0..60 {
                let (x, y) = (i as f64 * 2.0, j as f64 * 1.5);
                a.push(rect(x, y, x + 1.2, y + 1.0));
                if (i * 60 + j) % 7 != 0 {
                    let dx = if i % 2 == 0 { 0.3 } else { 0.0 };
                    b.push(rect(x + dx, y, x + dx + 1.2, y + 1.0));
                }
            }
        }
        let key = LayerKey { layer: 1, datatype: 0 };
        let whole = xor_split_owned(&a, &b, key.into());
        let (added, removed, leaves, failed) = tiled_xor(&a, &b, key, 50);
        assert!(!failed && whole.error.is_none());
        assert!(leaves > 16, "{leaves} hojas");
        assert!((area(&added) - area(&whole.added)).abs() < 1e-9, "{} vs {}", area(&added), area(&whole.added));
        assert!((area(&removed) - area(&whole.removed)).abs() < 1e-9, "{} vs {}", area(&removed), area(&whole.removed));
        // Sin nada que partir: una sola llamada.
        assert_eq!(tiled_xor(&a[..10], &b[..10], key, 50).2, 1);
    }

    #[test]
    fn multiset_diff_keeps_repetitions() {
        assert_eq!(multiset_diff(&[1, 2, 2, 5], &[2, 3, 5, 5]), (vec![1, 2], vec![3, 5]));
        assert_eq!(multiset_diff(&[], &[4]), (vec![], vec![4]));
    }

    #[test]
    fn budget_lets_a_task_bigger_than_everything_run_alone() {
        let b = MemBudget { total: 100, used: Mutex::new(0), freed: Condvar::new() };
        let big = b.acquire(1_000);
        assert_eq!(*b.used.lock().unwrap(), 100);
        drop(big);
        let (x, y) = (b.acquire(40), b.acquire(60));
        assert_eq!(*b.used.lock().unwrap(), 100);
        drop((x, y));
        assert_eq!(*b.used.lock().unwrap(), 0);
    }
}
