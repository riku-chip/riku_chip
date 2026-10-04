//! Índice espacial de una escena: en cada cuadro, qué dibujar sin recorrer
//! todos los elementos.
//!
//! Se arma una vez al cargar ([`crate::scene::Scene::build_index`]) y sirve
//! para tres cosas:
//!
//! - **Culling:** los elementos se reparten en *cubetas* por tamaño (lado
//!   mayor del bbox) y cada cubeta tiene una grilla con celdas de su tamaño,
//!   así cada elemento toca a lo sumo 2×2 celdas. Una consulta visita solo
//!   las celdas de la región visible.
//! - **Nivel de detalle:** solo cuando lo visible no cabe en un presupuesto
//!   ([`BUDGET`]; si cabe, se dibuja todo como siempre). Entonces los
//!   elementos chicos en pantalla no van uno a uno: una *pirámide de
//!   cobertura* dice, por nivel y capa, qué celdas tienen algo, y el
//!   consumidor la pinta como una imagen (una celda ≈ un píxel). Hay dos
//!   pirámides: una resume lo de menos de 4 celdas y otra lo de menos de 16;
//!   las dos resumen también lo que tiene menos de una celda de ancho (cables
//!   largos y finos, marcados siguiendo sus bordes).
//! - **Relleno:** cada polígono relleno sabe si es convexo o trae sus
//!   triángulos (earcut), calculados una sola vez.
//!
//! Los textos no entran en la pirámide (su bbox es un punto): se prueban uno a
//! uno contra la región visible.

use std::collections::HashMap;

use crate::bbox::BoundingBox;
use crate::element::{DrawElement, Layer};
use crate::fill::{is_convex, triangulate};

/// Celdas por lado del nivel más fino. El nivel `ℓ` tiene `BASE_CELLS >> ℓ`.
const BASE_CELLS: usize = 2048;
/// Niveles de la pirámide: el último (`LEVELS - 1`) es una sola celda.
const LEVELS: usize = 12;
/// Dos pirámides: la `s` resume en el nivel `ℓ` los elementos de lado menor
/// a `2^SPANS[s]` celdas (4 y 16). Con una vista cargada se prueba primero
/// resumir más sin agrandar las celdas (la imagen sigue a un texel por
/// píxel); solo si no alcanza se pasa a celdas más grandes.
const SPANS: [usize; 2] = [2, 4];
/// Lado máximo de una celda de la pirámide, en píxeles, al elegir el nivel.
pub const BLOCK_PX: f64 = 1.0;
/// Elementos uno a uno que admite un cuadro antes de resumir: si lo visible
/// cabe, se dibuja todo como siempre; si no, se resume.
pub const BUDGET: usize = 60_000;

/// Cómo rellenar un elemento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    /// No es un polígono relleno, o es degenerado: solo contorno.
    None,
    /// Convexo: se rellena en abanico.
    Convex,
    /// Cóncavo: se rellena con los triángulos de [`SceneIndex::fill`].
    Triangles,
}

/// Qué pide el consumidor en un cuadro.
#[derive(Clone, Copy, Debug)]
pub struct LodQuery {
    /// Región visible, en coordenadas de mundo.
    pub bbox: BoundingBox,
    /// Unidades de mundo que ocupa un píxel.
    pub px_world: f64,
    /// `false`: todo uno a uno, sin pirámide.
    pub lod: bool,
    /// Lado máximo de una celda de la pirámide en píxeles ([`BLOCK_PX`]).
    pub block_px: f64,
    /// Textos de menos de estos píxeles de alto no se entregan (0: todos;
    /// `f64::INFINITY`: ninguno).
    pub min_text_px: f64,
    /// Elementos uno a uno que caben antes de resumir ([`BUDGET`]). `0`:
    /// resumir siempre en el nivel del píxel, sin engrosar.
    pub budget: usize,
}

/// Resultado de una consulta.
#[derive(Clone, Debug, Default)]
pub struct Visible {
    /// Índices en `Scene::elements` de lo que se dibuja uno a uno, en el
    /// orden de la escena (el de pintado).
    pub elements: Vec<u32>,
    /// Nivel de la pirámide que resume el resto ([`SceneIndex::coverage`]).
    /// `None` con el zoom cerca: todo va uno a uno.
    pub level: Option<usize>,
    /// Cuál de las dos pirámides: 0 resume hasta 4 celdas, 1 hasta 16.
    pub span: usize,
}

/// Una capa de un nivel de la pirámide, para pintarla: celda `(cx, cy)`
/// ocupada si [`CoverageLayer::is_set`].
pub struct CoverageLayer<'a> {
    pub layer: Layer,
    n: usize,
    bits: &'a [u64],
}

impl CoverageLayer<'_> {
    pub fn is_set(&self, cx: usize, cy: usize) -> bool {
        let c = cy * self.n + cx;
        self.bits[c / 64] >> (c % 64) & 1 == 1
    }

    /// Celdas ocupadas, como `(cx, cy)`, fila por fila.
    pub fn cells(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let n = self.n;
        self.bits.iter().enumerate().flat_map(move |(w, &word)| {
            let mut rest = word;
            std::iter::from_fn(move || {
                if rest == 0 {
                    return None;
                }
                let p = w * 64 + rest.trailing_zeros() as usize;
                rest &= rest - 1;
                Some((p % n, p / n))
            })
        })
    }
}

/// Un nivel de la pirámide: grilla de `n × n` celdas de lado `cell` desde
/// `origin` (esquina de coordenadas mínimas).
pub struct CoverageView<'a> {
    pub n: usize,
    pub cell: f64,
    pub origin: (f64, f64),
    levels: &'a Coverage,
}

impl CoverageView<'_> {
    /// Capas con algo en este nivel, en el orden de la escena (el de pintado).
    pub fn layers(&self) -> impl Iterator<Item = CoverageLayer<'_>> + '_ {
        self.levels.layers.iter().map(|(layer, bits)| CoverageLayer { layer: *layer, n: self.levels.n, bits })
    }

    /// Región del mundo que cubre la grilla.
    pub fn bbox(&self) -> BoundingBox {
        let side = self.cell * self.n as f64;
        BoundingBox::from_points(self.origin, (self.origin.0 + side, self.origin.1 + side))
    }
}

/// Grilla CSR: la celda `c` lista `items[offsets[c]..offsets[c + 1]]`.
struct Grid {
    n: usize,
    cell: f64,
    offsets: Vec<u32>,
    items: Vec<u32>,
}

/// Un nivel de la pirámide: por capa, bitset de `n × n` celdas.
struct Coverage {
    n: usize,
    layers: Vec<(Layer, Vec<u64>)>,
}

pub struct SceneIndex {
    x0: f64,
    y0: f64,
    side: f64,
    bboxes: Vec<[f32; 4]>,
    /// Ancho de cada elemento: `2·área/perímetro` en polígonos (el ancho de
    /// un cable, aunque haga curvas), alto en los textos, lado menor del bbox
    /// en el resto.
    width: Vec<f32>,
    /// Por cubeta (0..LEVELS: elementos con lado menor que la celda de ese
    /// nivel y no menor que la del anterior; LEVELS: los que ocupan toda la
    /// escena).
    grids: Vec<Option<Grid>>,
    /// Elementos por cubeta, para estimar cuántos se verían uno a uno.
    bucket_counts: Vec<usize>,
    /// Textos: se prueban uno a uno.
    always: Vec<u32>,
    /// Las dos pirámides (ver [`SPANS`]), por nivel.
    coverage: [Vec<Coverage>; 2],
    /// Por pirámide, nivel desde el que cada elemento se resume (`u8::MAX`:
    /// nunca, siempre uno a uno).
    summary: [Vec<u8>; 2],
    fill: Vec<Fill>,
    triangles: HashMap<u32, Box<[u32]>>,
    /// Único por índice armado: identifica la escena en cachés de la UI (la
    /// dirección de memoria no sirve, una escena nueva puede reusarla).
    id: u64,
}

impl std::fmt::Debug for SceneIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneIndex")
            .field("elements", &self.bboxes.len())
            .field("side", &self.side)
            .field("concave", &self.triangles.len())
            .finish_non_exhaustive()
    }
}

/// `f64 → f32` redondeando hacia abajo (`up = false`) o hacia arriba: un bbox
/// en `f32` nunca queda más chico que el original.
fn to_f32(v: f64, up: bool) -> f32 {
    let f = v as f32;
    let slack = f.abs() * f32::EPSILON + f32::MIN_POSITIVE;
    match (up, (f as f64) < v, (f as f64) > v) {
        (true, true, _) => f + slack,
        (false, _, true) => f - slack,
        _ => f,
    }
}

/// Marca las celdas por las que pasa el segmento `a → b` (una muestra cada
/// media celda).
fn trace(a: (f64, f64), b: (f64, f64), cell: f64, mark: &mut impl FnMut(f64, f64)) {
    let len = (b.0 - a.0).abs().max((b.1 - a.1).abs());
    let steps = (len / (cell * 0.5)).ceil().min(1e7) as usize;
    for s in 0..=steps {
        let t = if steps == 0 { 0.0 } else { s as f64 / steps as f64 };
        mark(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
    }
}

fn set_bit(bits: &mut [u64], n: usize, cx: usize, cy: usize) {
    let c = cy * n + cx;
    bits[c / 64] |= 1 << (c % 64);
}

/// ¿Rectángulo alineado a los ejes (4 vértices, o 5 con el de cierre)?
fn is_axis_rect(points: &[(f64, f64)]) -> bool {
    let pts = crate::fill::strip_closing_point(points);
    pts.len() == 4
        && (0..4).all(|k| {
            let (a, b) = (pts[k], pts[(k + 1) % 4]);
            a.0 == b.0 || a.1 == b.1
        })
}

/// Regla par-impar: ¿el punto cae dentro del polígono?
fn point_in_polygon(points: &[(f64, f64)], x: f64, y: f64) -> bool {
    let mut inside = false;
    let n = points.len();
    for k in 0..n {
        let (a, b) = (points[k], points[(k + 1) % n]);
        if (a.1 > y) != (b.1 > y) && x < a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            inside = !inside;
        }
    }
    inside
}

fn map_all<T: Send>(elements: &[DrawElement], f: impl Fn(&DrawElement) -> T + Sync + Send) -> Vec<T> {
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        elements.par_iter().map(f).collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        elements.iter().map(f).collect()
    }
}

impl SceneIndex {
    /// Arma el índice de `elements` (cuyo bbox total es `bbox`). Con la
    /// feature `parallel`, el cálculo por elemento usa todos los núcleos.
    /// `outline_layer` dice qué capas se dibujan solo en contorno: en la
    /// pirámide se marcan sus bordes, no su interior.
    pub fn build(elements: &[DrawElement], bbox: &BoundingBox, outline_layer: &dyn Fn(Layer) -> bool) -> Self {
        let (x0, y0, side) = if bbox.is_empty() {
            (0.0, 0.0, 1.0)
        } else {
            (bbox.min_x, bbox.min_y, bbox.width().max(bbox.height()).max(f64::MIN_POSITIVE))
        };

        let per_element = map_all(elements, |el| {
            let b = el.bounding_box();
            let bb = if b.is_empty() {
                [f32::NAN; 4]
            } else {
                [to_f32(b.min_x, false), to_f32(b.min_y, false), to_f32(b.max_x, true), to_f32(b.max_y, true)]
            };
            let width = match el {
                DrawElement::Polygon { points, .. } if points.len() >= 3 => {
                    let n = points.len();
                    let (mut area2, mut perimeter) = (0.0_f64, 0.0_f64);
                    for k in 0..n {
                        let (a, c) = (points[k], points[(k + 1) % n]);
                        area2 += a.0 * c.1 - c.0 * a.1;
                        perimeter += (c.0 - a.0).hypot(c.1 - a.1);
                    }
                    if perimeter > 0.0 {
                        (area2.abs() / perimeter) as f32
                    } else {
                        0.0
                    }
                }
                // Textos: su alto (para descartar los ilegibles en la consulta).
                DrawElement::Text { size, .. } => *size as f32,
                _ if b.is_empty() => 0.0,
                _ => b.width().min(b.height()) as f32,
            };
            let (fill, tris) = match el {
                DrawElement::Polygon { points, filled: true, .. } if points.len() >= 3 => {
                    if is_convex(points) {
                        (Fill::Convex, None)
                    } else {
                        let t = triangulate(points);
                        if t.is_empty() {
                            (Fill::None, None)
                        } else {
                            (Fill::Triangles, Some(t.into_iter().map(|i| i as u32).collect::<Box<[u32]>>()))
                        }
                    }
                }
                _ => (Fill::None, None),
            };
            (bb, width, fill, tris)
        });

        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let mut index = SceneIndex {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            x0,
            y0,
            side,
            bboxes: Vec::with_capacity(elements.len()),
            width: Vec::with_capacity(elements.len()),
            grids: Vec::new(),
            bucket_counts: Vec::new(),
            always: Vec::new(),
            coverage: [Vec::new(), Vec::new()],
            summary: [Vec::new(), Vec::new()],
            fill: Vec::with_capacity(elements.len()),
            triangles: HashMap::new(),
        };
        for (i, (bb, width, fill, tris)) in per_element.into_iter().enumerate() {
            index.bboxes.push(bb);
            index.width.push(width);
            index.fill.push(fill);
            if let Some(t) = tris {
                index.triangles.insert(i as u32, t);
            }
        }

        // Cubetas por tamaño (lado mayor) y, por pirámide, nivel desde el que
        // cada elemento entra en ella: el primero donde mide menos de
        // `2^SPANS[s]` celdas, o donde su ancho ya no llega a una celda (un
        // cable largo y fino se resume aunque sea largo).
        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); LEVELS + 1];
        let mut adds: [Vec<Vec<u32>>; 2] = [vec![Vec::new(); LEVELS], vec![Vec::new(); LEVELS]];
        index.summary = [vec![u8::MAX; elements.len()], vec![u8::MAX; elements.len()]];
        for (i, el) in elements.iter().enumerate() {
            let bb = index.bboxes[i];
            if matches!(el, DrawElement::Text { .. }) || bb[0].is_nan() {
                index.always.push(i as u32);
                continue;
            }
            let size = f64::from((bb[2] - bb[0]).max(bb[3] - bb[1]));
            let b = (0..LEVELS).find(|&l| size < index.cell(l)).unwrap_or(LEVELS);
            let thin = f64::from(index.width[i]);
            // "Fino" solo en polígonos rellenos: una línea o un contorno ya se
            // dibujan con 1 px, no hay área que resumir.
            let thin_from = matches!(el, DrawElement::Polygon { filled: true, .. })
                .then(|| (0..LEVELS).find(|&l| thin < index.cell(l)))
                .flatten();
            for (s, shift) in SPANS.iter().enumerate() {
                let small_from = (b < LEVELS).then(|| b.saturating_sub(*shift));
                if let Some(level) = small_from.into_iter().chain(thin_from).min() {
                    index.summary[s][i] = level as u8;
                    adds[s][level].push(i as u32);
                }
            }
            buckets[b].push(i as u32);
        }

        index.bucket_counts = buckets.iter().map(Vec::len).collect();
        index.grids =
            buckets.iter().enumerate().map(|(b, items)| (!items.is_empty()).then(|| index.build_grid(b, items))).collect();
        let coverage = [0, 1].map(|s| index.build_coverage(elements, &adds[s], outline_layer, SPANS[s]));
        index.coverage = coverage;
        index
    }

    /// Lado de la celda del nivel `level`.
    fn cell(&self, level: usize) -> f64 {
        self.side / (BASE_CELLS >> level) as f64
    }

    /// Celdas (en una grilla de `n × n` y lado `cell`) que cubre el intervalo.
    fn span(&self, n: usize, cell: f64, lo: f64, hi: f64, origin: f64) -> (usize, usize) {
        let idx = |v: f64| (((v - origin) / cell).floor().max(0.0) as usize).min(n - 1);
        (idx(lo), idx(hi))
    }

    fn build_grid(&self, bucket: usize, items: &[u32]) -> Grid {
        // Celdas del tamaño de la cubeta, pero no más de ~4 por elemento: una
        // cubeta con pocos elementos no reserva millones de celdas vacías (con
        // celdas más grandes, cada elemento sigue tocando a lo sumo 2×2).
        let max_n = BASE_CELLS >> bucket.min(LEVELS - 1);
        let fit = ((items.len() * 4) as f64).sqrt().ceil() as usize;
        let n = if bucket < LEVELS { max_n.min(fit.next_power_of_two()).max(1) } else { 1 };
        let cell = self.side / n as f64;
        let cells_of = |i: u32| {
            let b = self.bboxes[i as usize];
            let (cx0, cx1) = self.span(n, cell, f64::from(b[0]), f64::from(b[2]), self.x0);
            let (cy0, cy1) = self.span(n, cell, f64::from(b[1]), f64::from(b[3]), self.y0);
            (cx0, cx1, cy0, cy1)
        };
        let mut counts = vec![0u32; n * n + 1];
        for &i in items {
            let (cx0, cx1, cy0, cy1) = cells_of(i);
            for cy in cy0..=cy1 {
                for cx in cx0..=cx1 {
                    counts[cy * n + cx + 1] += 1;
                }
            }
        }
        for c in 1..counts.len() {
            counts[c] += counts[c - 1];
        }
        let mut fill_at = counts.clone();
        let mut out = vec![0u32; *counts.last().unwrap_or(&0) as usize];
        for &i in items {
            let (cx0, cx1, cy0, cy1) = cells_of(i);
            for cy in cy0..=cy1 {
                for cx in cx0..=cx1 {
                    let slot = &mut fill_at[cy * n + cx];
                    out[*slot as usize] = i;
                    *slot += 1;
                }
            }
        }
        Grid { n, cell, offsets: counts, items: out }
    }

    /// El nivel `ℓ` resume los elementos con `summary ≤ ℓ`. Cada nivel sale
    /// del anterior (cada celda es la unión de 2×2) más los que entran en él
    /// (`adds[ℓ]`); las capas quedan en el orden de la escena.
    fn build_coverage(
        &self,
        elements: &[DrawElement],
        adds: &[Vec<u32>],
        outline_layer: &dyn Fn(Layer) -> bool,
        shift: usize,
    ) -> Vec<Coverage> {
        // Orden de pintado de las capas: el de su primera aparición.
        let mut order: HashMap<Layer, usize> = HashMap::new();
        for el in elements {
            let next = order.len();
            order.entry(el.layer()).or_insert(next);
        }
        let mut levels: Vec<Coverage> = Vec::with_capacity(LEVELS);
        for level in 0..LEVELS {
            let n = BASE_CELLS >> level;
            let words = (n * n).div_ceil(64);
            let mut cov = Coverage { n, layers: Vec::new() };
            let mut slot: HashMap<Layer, usize> = HashMap::new();
            let mut layer_bits = |cov: &mut Coverage, layer: Layer| -> usize {
                *slot.entry(layer).or_insert_with(|| {
                    cov.layers.push((layer, vec![0u64; words]));
                    cov.layers.len() - 1
                })
            };
            if let Some(prev) = levels.last() {
                for (layer, bits) in &prev.layers {
                    let k = layer_bits(&mut cov, *layer);
                    for (w, &word) in bits.iter().enumerate() {
                        let mut rest = word;
                        while rest != 0 {
                            let p = w * 64 + rest.trailing_zeros() as usize;
                            rest &= rest - 1;
                            let c = (p / prev.n / 2) * n + (p % prev.n) / 2;
                            cov.layers[k].1[c / 64] |= 1 << (c % 64);
                        }
                    }
                }
            }
            let cell = self.cell(level);
            for &i in &adds[level] {
                let b = self.bboxes[i as usize];
                let el = &elements[i as usize];
                let k = layer_bits(&mut cov, el.layer());
                let bits = &mut cov.layers[k].1;
                let cell_of = |x: f64, y: f64| (self.span(n, cell, x, x, self.x0).0, self.span(n, cell, y, y, self.y0).0);
                let small = f64::from((b[2] - b[0]).max(b[3] - b[1])) < (1 << shift) as f64 * cell;
                let corners;
                let points: &[(f64, f64)] = match el {
                    DrawElement::Polygon { points, .. } => points,
                    DrawElement::Line { x1, y1, x2, y2, .. } => {
                        corners = [(*x1, *y1), (*x2, *y2)];
                        &corners
                    }
                    _ => &[],
                };
                let edges = |bits: &mut [u64]| {
                    let np = points.len();
                    for (k, &a) in points.iter().enumerate() {
                        if np == 2 && k == 1 {
                            break; // una línea: un solo segmento
                        }
                        trace(a, points[(k + 1) % np], cell, &mut |x, y| {
                            let (cx, cy) = cell_of(x, y);
                            set_bit(bits, n, cx, cy);
                        });
                    }
                };
                let (cx0, cx1) = self.span(n, cell, f64::from(b[0]), f64::from(b[2]), self.x0);
                let (cy0, cy1) = self.span(n, cell, f64::from(b[1]), f64::from(b[3]), self.y0);
                if !points.is_empty() && (outline_layer(el.layer()) || !small || points.len() == 2) {
                    // Capas de solo contorno, cables largos y finos y líneas:
                    // se marcan siguiendo sus bordes (una L no rellena su
                    // rectángulo; un boundary no tiñe toda la celda).
                    edges(bits);
                } else if points.len() < 3 || is_axis_rect(points) {
                    // Rectángulos (y lo que no es polígono): su bbox, exacto.
                    for cy in cy0..=cy1 {
                        for cx in cx0..=cx1 {
                            let c = cy * n + cx;
                            bits[c / 64] |= 1 << (c % 64);
                        }
                    }
                } else {
                    // Otros polígonos chicos: las celdas cuyo centro cae
                    // dentro; si ninguna (una astilla), la del centro del bbox.
                    let mut any = false;
                    for cy in cy0..=cy1 {
                        for cx in cx0..=cx1 {
                            let px = self.x0 + (cx as f64 + 0.5) * cell;
                            let py = self.y0 + (cy as f64 + 0.5) * cell;
                            if point_in_polygon(points, px, py) {
                                let c = cy * n + cx;
                                bits[c / 64] |= 1 << (c % 64);
                                any = true;
                            }
                        }
                    }
                    if !any {
                        let (cx, cy) = cell_of(f64::from(b[0] + b[2]) * 0.5, f64::from(b[1] + b[3]) * 0.5);
                        set_bit(bits, n, cx, cy);
                    }
                }
            }
            cov.layers.sort_by_key(|(layer, _)| order.get(layer).copied().unwrap_or(usize::MAX));
            levels.push(cov);
        }
        levels
    }

    /// Nivel de la pirámide para un píxel de `px_world`: el de celda más
    /// grande que no pase de `block_px` píxeles. `None` si ni el más fino
    /// alcanza (zoom cercano: todo uno a uno).
    pub fn lod_level(&self, px_world: f64, block_px: f64) -> Option<usize> {
        (0..LEVELS).rev().find(|&l| self.cell(l) <= block_px * px_world)
    }

    /// Nivel de la pirámide para la consulta, o `None` (todo uno a uno).
    ///
    /// Se estima cuántos elementos se verían uno a uno (los de cada cubeta,
    /// en proporción al área visible). Si caben en `q.budget`, no se resume
    /// nada: el dibujo es el de siempre. Si no, el nivel del tamaño de píxel
    /// (celda ≤ `block_px`), y se engrosa mientras no quepan.
    fn choose_level(&self, q: &LodQuery) -> Option<(usize, usize)> {
        let side = self.side;
        let w = (q.bbox.max_x.min(self.x0 + side) - q.bbox.min_x.max(self.x0)).max(0.0);
        let h = (q.bbox.max_y.min(self.y0 + side) - q.bbox.min_y.max(self.y0)).max(0.0);
        let fraction = (w * h / (side * side)).min(1.0);
        let estimate =
            |first_bucket: usize| -> f64 { self.bucket_counts.iter().skip(first_bucket).sum::<usize>() as f64 * fraction };
        if estimate(0) <= q.budget as f64 {
            return None;
        }
        let mut level = self.lod_level(q.px_world, q.block_px)?;
        let fits = |level: usize, span: usize| estimate(level + SPANS[span] + 1) <= q.budget as f64;
        if q.budget == 0 || fits(level, 0) {
            return Some((level, 0));
        }
        // Primero resumir más sin agrandar el texel; después, celdas más grandes.
        while level + 1 < LEVELS && !fits(level, 1) {
            level += 1;
        }
        Some((level, 1))
    }

    /// Un nivel de una de las pirámides (`span`, como en [`Visible::span`]).
    pub fn coverage(&self, level: usize, span: usize) -> Option<CoverageView<'_>> {
        self.coverage.get(span)?.get(level).map(|c| CoverageView {
            n: c.n,
            cell: self.cell(level),
            origin: (self.x0, self.y0),
            levels: c,
        })
    }

    /// Qué dibujar uno a uno en la región `q.bbox` (y qué nivel de la
    /// pirámide pinta el resto), omitiendo las capas para las que `hidden`
    /// dice `true`.
    pub fn visible(&self, elements: &[DrawElement], q: &LodQuery, hidden: &dyn Fn(Layer) -> bool) -> Visible {
        let mut out = Visible::default();
        if q.bbox.is_empty() {
            out.elements = (0..elements.len() as u32).filter(|&i| !hidden(elements[i as usize].layer())).collect();
            return out;
        }
        (out.level, out.span) = match q.lod.then(|| self.choose_level(q)).flatten() {
            Some((level, span)) => (Some(level), span),
            None => (None, 0),
        };
        let first_bucket = out.level.map_or(0, |l| (l + SPANS[out.span] + 1).min(self.grids.len()));
        let (qx0, qy0, qx1, qy1) = (q.bbox.min_x, q.bbox.min_y, q.bbox.max_x, q.bbox.max_y);
        let touches =
            |b: &[f32; 4]| f64::from(b[0]) <= qx1 && f64::from(b[2]) >= qx0 && f64::from(b[1]) <= qy1 && f64::from(b[3]) >= qy0;

        for grid in self.grids[first_bucket..].iter().flatten() {
            let (gx0, gx1) = self.span(grid.n, grid.cell, qx0, qx1, self.x0);
            let (gy0, gy1) = self.span(grid.n, grid.cell, qy0, qy1, self.y0);
            for cy in gy0..=gy1 {
                for cx in gx0..=gx1 {
                    let c = cy * grid.n + cx;
                    for &i in &grid.items[grid.offsets[c] as usize..grid.offsets[c + 1] as usize] {
                        let b = &self.bboxes[i as usize];
                        // Un elemento que toca varias celdas se entrega solo
                        // desde la primera dentro de la región.
                        let ex0 = self.span(grid.n, grid.cell, f64::from(b[0]), f64::from(b[0]), self.x0).0;
                        let ey0 = self.span(grid.n, grid.cell, f64::from(b[1]), f64::from(b[1]), self.y0).0;
                        if (ex0.max(gx0), ey0.max(gy0)) != (cx, cy) {
                            continue;
                        }
                        // Resumido en la pirámide de este nivel (chico o fino).
                        if out.level.is_some_and(|l| usize::from(self.summary[out.span][i as usize]) <= l) {
                            continue;
                        }
                        if touches(b) && !hidden(elements[i as usize].layer()) {
                            out.elements.push(i);
                        }
                    }
                }
            }
        }
        for &i in &self.always {
            let el = &elements[i as usize];
            let b = &self.bboxes[i as usize];
            let tiny_text =
                matches!(el, DrawElement::Text { .. }) && f64::from(self.width[i as usize]) < q.min_text_px * q.px_world;
            if !hidden(el.layer()) && !tiny_text && (b[0].is_nan() || touches(b)) {
                out.elements.push(i);
            }
        }
        out.elements.sort_unstable();
        out
    }

    /// Elementos (índices, en orden de la escena) cuyo bbox contiene el punto.
    /// Los textos no se incluyen.
    pub fn candidates_at(&self, x: f64, y: f64) -> Vec<u32> {
        let mut out = Vec::new();
        for grid in self.grids.iter().flatten() {
            let cx = self.span(grid.n, grid.cell, x, x, self.x0).0;
            let cy = self.span(grid.n, grid.cell, y, y, self.y0).0;
            let c = cy * grid.n + cx;
            for &i in &grid.items[grid.offsets[c] as usize..grid.offsets[c + 1] as usize] {
                let b = &self.bboxes[i as usize];
                if f64::from(b[0]) <= x && f64::from(b[2]) >= x && f64::from(b[1]) <= y && f64::from(b[3]) >= y {
                    out.push(i);
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// Relleno del elemento `i` y, si es cóncavo, sus triángulos (índices a
    /// sus puntos sin el vértice de cierre repetido).
    pub fn fill(&self, i: usize) -> (Fill, Option<&[u32]>) {
        let f = self.fill.get(i).copied().unwrap_or(Fill::None);
        (f, self.triangles.get(&(i as u32)).map(|t| &t[..]))
    }

    /// Identificador único de este índice (para cachés que duran más que la
    /// escena, como las texturas de la pirámide).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Ancho del elemento `i` (0 si no tiene): `2·área/perímetro` en polígonos
    /// (el ancho de un cable aunque haga curvas), alto en los textos, lado menor
    /// del bbox en el resto. Ver el campo `width`.
    pub fn min_size(&self, i: usize) -> f64 {
        self.width.get(i).map_or(0.0, |w| f64::from(*w))
    }

    /// Lado mayor del bbox del elemento `i` (0 si no tiene).
    pub fn size(&self, i: usize) -> f64 {
        self.bboxes.get(i).map_or(0.0, |b| if b[0].is_nan() { 0.0 } else { f64::from((b[2] - b[0]).max(b[3] - b[1])) })
    }

    pub fn len(&self) -> usize {
        self.bboxes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bboxes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, s: f64, layer: Layer) -> DrawElement {
        DrawElement::Polygon { points: vec![(x, y), (x + s, y), (x + s, y + s), (x, y + s)], layer, filled: true }
    }

    fn scene(elements: &[DrawElement]) -> BoundingBox {
        let mut b = BoundingBox::empty();
        for e in elements {
            b.expand(&e.bounding_box());
        }
        b
    }

    fn query(bbox: BoundingBox, px_world: f64, lod: bool) -> LodQuery {
        LodQuery { bbox, px_world, lod, block_px: BLOCK_PX, min_text_px: 0.0, budget: 0 }
    }

    #[test]
    fn each_index_has_its_own_id() {
        // La UI guarda texturas por escena: dos escenas no comparten id
        // aunque una reuse la memoria de la otra.
        let els = vec![rect(0.0, 0.0, 1.0, 1)];
        let bb = scene(&els);
        let a = SceneIndex::build(&els, &bb, &|_| false).id();
        let b = SceneIndex::build(&els, &bb, &|_| false).id();
        assert_ne!(a, b);
    }

    #[test]
    fn culling_returns_each_visible_element_once_in_scene_order() {
        // Una grilla de 100×100 cuadrados de lado 1 separados 2, más uno que
        // cruza muchas celdas.
        let mut els: Vec<DrawElement> =
            (0..10_000).map(|i| rect((i % 100) as f64 * 2.0, (i / 100) as f64 * 2.0, 1.0, 1)).collect();
        els.push(rect(-1.0, -1.0, 250.0, 2));
        let bb = scene(&els);
        let idx = SceneIndex::build(&els, &bb, &|_| false);
        let q = BoundingBox::from_points((10.5, 10.5), (20.5, 20.5));
        let got = idx.visible(&els, &query(q, 1e-9, true), &|_| false).elements;
        let want: Vec<u32> = (0..els.len() as u32)
            .filter(|&i| {
                let b = els[i as usize].bounding_box();
                b.min_x <= q.max_x && b.max_x >= q.min_x && b.min_y <= q.max_y && b.max_y >= q.min_y
            })
            .collect();
        assert_eq!(got, want);
        // Capas ocultas.
        let only_big = idx.visible(&els, &query(q, 1e-9, true), &|l| l == 1).elements;
        assert_eq!(only_big, vec![10_000]);
    }

    #[test]
    fn far_zoom_summarizes_small_elements_and_near_zoom_does_not() {
        let els: Vec<DrawElement> = (0..400).map(|i| rect((i % 20) as f64 * 10.0, (i / 20) as f64 * 10.0, 0.1, 3)).collect();
        let bb = scene(&els);
        let idx = SceneIndex::build(&els, &bb, &|_| false);
        // Un píxel = 5 unidades: los cuadrados de 0,1 no se dibujan uno a uno,
        // y la pirámide marca la celda de cada uno.
        let far = idx.visible(&els, &query(bb, 5.0, true), &|_| false);
        assert!(far.elements.is_empty());
        let view = idx.coverage(far.level.expect("nivel"), far.span).unwrap();
        assert!(view.cell <= 5.0 * BLOCK_PX);
        let layer = view.layers().next().unwrap();
        assert_eq!(layer.layer, 3);
        for e in &els {
            let b = e.bounding_box();
            let cx = ((b.min_x - view.origin.0) / view.cell).floor() as usize;
            let cy = ((b.min_y - view.origin.1) / view.cell).floor() as usize;
            assert!(layer.is_set(cx.min(view.n - 1), cy.min(view.n - 1)));
        }
        // Un cuadrado justo sobre un borde marca dos celdas.
        assert!((400..=800).contains(&layer.cells().count()));
        // Sin LOD, o de cerca, todo uno a uno.
        let off = idx.visible(&els, &query(bb, 5.0, false), &|_| false);
        assert_eq!((off.elements.len(), off.level), (400, None));
        let near = idx.visible(&els, &query(bb, 0.001, true), &|_| false);
        assert_eq!((near.elements.len(), near.level), (400, None));
    }

    #[test]
    fn elements_a_few_cells_wide_are_summarized_bigger_ones_are_not() {
        // Lado 3 celdas: entra en la pirámide. Lado 100 celdas: uno a uno.
        let els = vec![rect(0.0, 0.0, 1000.0, 1), rect(0.0, 0.0, 3.0, 2), rect(500.0, 500.0, 100.0, 3)];
        let bb = scene(&els);
        let idx = SceneIndex::build(&els, &bb, &|_| false);
        // Nivel con celda ≈ 1 unidad: un píxel = 1,1 unidades.
        let v = idx.visible(&els, &query(bb, 1.1, true), &|_| false);
        assert_eq!(v.elements, vec![0, 2]);
        let view = idx.coverage(v.level.unwrap(), v.span).unwrap();
        assert!(view.layers().any(|l| l.layer == 2));
    }

    #[test]
    fn texts_are_never_summarized() {
        let mut els: Vec<DrawElement> = vec![rect(0.0, 0.0, 100.0, 1)];
        els.push(DrawElement::Text {
            x: 50.0,
            y: 50.0,
            content: "VDD".into(),
            size: 1.0,
            angle_deg: 0.0,
            h_align: crate::element::HAlign::Start,
            v_align: crate::element::VAlign::Bottom,
            layer: 9,
        });
        let bb = scene(&els);
        let idx = SceneIndex::build(&els, &bb, &|_| false);
        let far = idx.visible(&els, &query(bb, 10.0, true), &|_| false);
        assert!(far.elements.contains(&1));
        // Un texto de 1 unidad con un píxel de 10: ilegible, no se entrega si se
        // piden textos de al menos 3 px.
        let q = LodQuery { min_text_px: 3.0, ..query(bb, 10.0, true) };
        assert!(!idx.visible(&els, &q, &|_| false).elements.contains(&1));
        let q = LodQuery { min_text_px: 3.0, ..query(bb, 0.1, true) };
        assert!(idx.visible(&els, &q, &|_| false).elements.contains(&1));
    }

    #[test]
    fn picking_candidates_and_cached_fill() {
        let l_shape = DrawElement::Polygon {
            points: vec![(0.0, 0.0), (3.0, 0.0), (3.0, 1.0), (1.0, 1.0), (1.0, 3.0), (0.0, 3.0)],
            layer: 1,
            filled: true,
        };
        let els = vec![rect(0.0, 0.0, 10.0, 1), l_shape, rect(20.0, 20.0, 1.0, 2)];
        let idx = SceneIndex::build(&els, &scene(&els), &|_| false);
        assert_eq!(idx.candidates_at(0.5, 0.5), vec![0, 1]);
        assert_eq!(idx.candidates_at(20.5, 20.5), vec![2]);
        assert!(idx.candidates_at(15.0, 15.0).is_empty());
        assert_eq!(idx.fill(0), (Fill::Convex, None));
        let (f, t) = idx.fill(1);
        assert_eq!(f, Fill::Triangles);
        assert_eq!(t.map(|t| t.len()), Some(12)); // 4 triángulos
    }

    #[test]
    fn f32_bboxes_never_shrink() {
        for v in [0.1_f64, 1e6 + 0.123_456_789, -3.333_333_333, 123_456.789_012] {
            assert!(f64::from(to_f32(v, false)) <= v);
            assert!(f64::from(to_f32(v, true)) >= v);
        }
    }

    #[test]
    fn long_thin_wires_are_summarized_along_their_path() {
        // Un cable en L de 0,1 de ancho y 100 de largo, y un bloque grande.
        let wire = DrawElement::Polygon {
            points: vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (99.9, 100.0), (99.9, 0.1), (0.0, 0.1)],
            layer: 5,
            filled: true,
        };
        let els = vec![rect(0.0, 0.0, 200.0, 1), wire];
        let bb = scene(&els);
        let idx = SceneIndex::build(&els, &bb, &|_| false);
        // Un píxel = 1 unidad: el cable es mucho más angosto que un píxel.
        let v = idx.visible(&els, &query(bb, 1.0, true), &|_| false);
        assert_eq!(v.elements, vec![0], "el cable no va uno a uno");
        let view = idx.coverage(v.level.unwrap(), v.span).unwrap();
        let layer = view.layers().find(|l| l.layer == 5).expect("el cable está en la pirámide");
        let cell_of = |x: f64, y: f64| (((x - view.origin.0) / view.cell) as usize, ((y - view.origin.1) / view.cell) as usize);
        let (ax, ay) = cell_of(50.0, 0.05);
        let (bx, by) = cell_of(99.95, 50.0);
        assert!(layer.is_set(ax, ay) && layer.is_set(bx, by), "marca su recorrido");
        let (cx, cy) = cell_of(50.0, 50.0);
        assert!(!layer.is_set(cx, cy), "no rellena el rectángulo de la L");
        // De cerca, uno a uno otra vez.
        assert_eq!(idx.visible(&els, &query(bb, 0.001, true), &|_| false).elements, vec![0, 1]);
    }

    #[test]
    fn small_views_are_never_summarized_and_crowded_ones_get_coarser() {
        let els: Vec<DrawElement> = (0..40_000).map(|i| rect((i % 200) as f64 * 5.0, (i / 200) as f64 * 5.0, 2.0, 1)).collect();
        let bb = scene(&els);
        let idx = SceneIndex::build(&els, &bb, &|_| false);
        let q = |bbox, px_world| LodQuery { budget: 30_000, ..query(bbox, px_world, true) };
        // Todo a la vista (40 000 > presupuesto) con 1 px = 1 unidad: los
        // cuadrados de 2 px van uno a uno en el nivel del píxel, así que se
        // engrosa hasta resumirlos.
        let far = idx.visible(&els, &q(bb, 1.0), &|_| false);
        assert!(far.level.is_some());
        assert!(far.elements.len() <= 30_000, "{}", far.elements.len());
        // Una ventana con ~1/16 de la escena: cabe, no se resume nada.
        let part = BoundingBox::from_points((0.0, 0.0), (250.0, 250.0));
        let near = idx.visible(&els, &q(part, 1.0), &|_| false);
        assert_eq!(near.level, None);
        assert_eq!(near.elements.len(), 51 * 51); // incluye los que tocan el borde
    }
}
