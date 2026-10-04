//! El resumen de cada celda (ver `docs/ronda-5/design.md`, D20): su
//! geometría propia extraída como siempre (`cell_nets_with`), sus
//! instancias con la transformación y, por instancia, a qué red de la celda
//! va cada red de la hija. Se arma de las hojas a la raíz.
//!
//! Las celdas chicas (menos de `inline` polígonos aplanados) se meten en su
//! padre como geometría propia, como `gds flatglob` en Magic: una celda de
//! contacto o de un solo transistor no tiene sentido sola (un `licon` sin la
//! difusión de abajo no es ningún tipo).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use gdstk_rs::{Cell, Library, OwnedPolygon};
use rayon::prelude::*;

use super::key::{net_keys, salt, NetKey};
use super::memo::memo;
use super::touch::{touching, Typed};
use super::xf::Xf;
use crate::box_grid::BoxGrid;
use crate::devices::extract::{bbox, point_in, Grid};
use crate::devices::{flat_polygon_estimate, DeviceRules};
use crate::labels::offsets;
use crate::nets::{Net, Netlist};

const EMPTY: [f64; 4] = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];

fn union_box(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

/// La intersección de dos cajas (vacía si no se tocan).
fn meet(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]
}

fn grow(b: [f64; 4], by: f64) -> [f64; 4] {
    [b[0] - by, b[1] - by, b[2] + by, b[3] + by]
}

/// Una instancia que no se metió en el padre.
#[derive(Clone, Debug)]
pub struct Inst {
    pub cell: String,
    /// La huella de la hija (su resumen en [`Cells`]).
    pub key: NetKey,
    pub xf: Xf,
    /// La caja de lo que conduce en la hija, en coordenadas del padre.
    pub bbox: [f64; 4],
}

/// Lo que se cuenta para medir.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Celdas armadas, y las que salieron de la memoria.
    pub cells: usize,
    pub remembered: usize,
    /// Instancias metidas en su padre (chicas o no Manhattan).
    pub inlined: usize,
    /// Vecindades calculadas y reusadas.
    pub neighbourhoods: usize,
    pub reused: usize,
}

/// El resumen de una celda.
pub struct CellNets {
    pub name: String,
    /// Lo propio (y lo de las instancias metidas), con todas sus redes.
    pub own: Netlist,
    /// Red propia → red de la celda.
    pub own_map: Vec<u32>,
    pub insts: Vec<Inst>,
    /// Por instancia: red de la hija → red de la celda.
    pub inst_maps: Vec<Vec<u32>>,
    /// Las redes de la celda: lo propio y todo lo de las hijas.
    pub nets: Vec<Net>,
    /// Por etiqueta propia, su red de la celda.
    pub label_nets: Vec<Option<usize>>,
    /// La caja de lo que conduce, con la sub-jerarquía.
    pub bbox: [f64; 4],
    /// Transistores de toda la sub-jerarquía.
    pub deep_devices: usize,
    /// Pedazos candidatos al sustrato (red, punto interior): pozos P que nada
    /// excluye todavía. En la raíz se unen al sustrato.
    pub open: Vec<(u32, (f64, f64))>,
    /// La red del sustrato de los cuerpos sin pozo, si hay.
    pub sub: Option<u32>,
    /// Dónde están esos cuerpos (coordenadas de la celda): la de arriba los
    /// lleva al pozo que tenga en ese punto.
    pub sub_points: Vec<(f64, f64)>,
    pub warnings: Vec<String>,
    /// Los pedazos propios por tipo, para buscar por caja.
    index: HashMap<String, (BoxGrid, Vec<usize>)>,
    inst_grid: BoxGrid,
}

/// Las celdas ya armadas, por huella.
pub type Cells = HashMap<NetKey, Arc<CellNets>>;

impl CellNets {
    /// Cuánta memoria ocupa, más o menos.
    pub(crate) fn bytes(&self) -> u64 {
        let pts: usize = self.own.pieces.iter().map(|p| p.poly.points.len()).sum();
        let maps: usize = self.inst_maps.iter().map(Vec::len).sum();
        (pts * 16
            + self.own.pieces.len() * 64
            + self.own.devices.len() * 300
            + self.nets.len() * 96
            + maps * 4
            + self.insts.len() * 96) as u64
            + 512
    }

    /// Los pedazos de `types` que tocan `area` (coordenadas de la celda), de
    /// toda la sub-jerarquía, con su red de la celda.
    pub(crate) fn pieces_in(&self, cells: &Cells, area: [f64; 4], types: &[String], out: &mut Vec<(u32, String, OwnedPolygon)>) {
        for t in types {
            let Some((grid, idx)) = self.index.get(t) else { continue };
            for k in grid.overlapping(area) {
                let p = &self.own.pieces[idx[k]];
                out.push((self.own_map[p.net], p.magic.clone(), p.poly.clone()));
            }
        }
        for k in self.inst_grid.overlapping(area) {
            let inst = &self.insts[k];
            let Some(child) = cells.get(&inst.key) else { continue };
            let mut sub = Vec::new();
            child.pieces_in(cells, inst.xf.inverse().bbox(&area), types, &mut sub);
            let map = &self.inst_maps[k];
            out.extend(sub.into_iter().map(|(n, t, p)| (map[n as usize], t, inst.xf.poly(&p))));
        }
    }
}

/// Opciones de la extracción.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Una sub-celda con menos polígonos aplanados se mete en su padre.
    pub inline: u64,
}

impl Default for Options {
    fn default() -> Self {
        let inline = std::env::var("RIKU_HIER_INLINE").ok().and_then(|v| v.parse().ok()).unwrap_or(256);
        Self { inline }
    }
}

struct Ctx<'a> {
    lib: &'a Library,
    rules: &'a DeviceRules,
    magic: Option<&'a gdstk_rs::magic::MagInfo>,
    opts: Options,
    unit_um: f64,
    /// Tipos que conducen con cada tipo (él incluido).
    conn: HashMap<String, Vec<String>>,
    conductors: Vec<String>,
    /// Polígonos aplanados de cada celda.
    sizes: HashMap<String, u64>,
    /// La huella de cada celda, y las que no se pueden guardar (sin huella
    /// propia: una por extracción).
    keys: HashMap<String, NetKey>,
    unique: HashSet<NetKey>,
    /// Vecindades de las celdas que no se guardan.
    local: Mutex<HashMap<super::memo::NeighbourKey, Arc<Vec<(u32, u32)>>>>,
    stats: Mutex<Stats>,
    profile: bool,
    /// Hasta qué distancia dos pedazos de celdas distintas pueden unirse
    /// (medio nanómetro, o lo que une un cierre: `DeviceRules::bridge`).
    reach: f64,
}

/// Las referencias de una celda que se meten en ella (índices).
fn inlined(ctx: &Ctx<'_>, cell: &Cell<'_>) -> Vec<bool> {
    cell.references()
        .map(|r| {
            let small = ctx.sizes.get(r.cell_name()).is_none_or(|&n| n < ctx.opts.inline);
            let odd = offsets(r.repetition_count(), |i| r.repetition_offset(i)).iter().any(|&o| Xf::of(&r, o).is_none());
            small || odd || ctx.lib.find_cell(r.cell_name()).is_none()
        })
        .collect()
}

/// El orden de armado: cada celda después de sus hijas no metidas, por niveles.
fn levels(ctx: &Ctx<'_>, root: &Cell<'_>) -> Vec<Vec<String>> {
    fn height(ctx: &Ctx<'_>, name: &str, memo: &mut HashMap<String, usize>, depth: usize) -> usize {
        if let Some(&h) = memo.get(name) {
            return h;
        }
        let mut h = 0;
        if let (Some(cell), true) = (ctx.lib.find_cell(name), depth < 64) {
            let inl = inlined(ctx, &cell);
            for (r, inl) in cell.references().zip(inl) {
                if !inl {
                    h = h.max(height(ctx, r.cell_name(), memo, depth + 1) + 1);
                }
            }
        }
        memo.insert(name.to_string(), h);
        h
    }
    let mut memo = HashMap::new();
    height(ctx, root.name(), &mut memo, 0);
    let mut out: Vec<Vec<String>> = Vec::new();
    for (name, h) in memo {
        if out.len() <= h {
            out.resize(h + 1, Vec::new());
        }
        out[h].push(name);
    }
    for l in &mut out {
        l.sort();
    }
    out
}

/// Union-find con la raíz menor (el resultado no depende del orden de las uniones).
struct Uf(Vec<u32>);

impl Uf {
    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            let p = self.0[x as usize];
            self.0[x as usize] = self.0[p as usize];
            x = p;
        }
        x
    }

    fn join(&mut self, a: u32, b: u32) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            let (lo, hi) = (a.min(b), a.max(b));
            self.0[hi as usize] = lo;
        }
    }
}

/// Arma el resumen de `cell` con los de sus hijas ya en `cells`.
fn build_cell(ctx: &Ctx<'_>, cells: &Cells, cell: &Cell<'_>) -> CellNets {
    let inl = inlined(ctx, cell);
    let refs: Vec<_> = cell.references().collect();
    let n_inl: usize = refs.iter().zip(&inl).filter(|(_, i)| **i).map(|(r, _)| r.repetition_count().max(1) as usize).sum();
    ctx.stats.lock().unwrap().inlined += n_inl;
    // Lo propio: la geometría de profundidad 0 y la de las referencias metidas.
    let polys = |tags: &[(u32, u32)]| -> Vec<OwnedPolygon> {
        let mut out = Vec::new();
        for &(l, d) in tags {
            out.extend(cell.get_polygons().depth(0).with_filter(l, d).build().polygons().map(|p| OwnedPolygon {
                layer: p.layer(),
                datatype: p.datatype(),
                points: p.points().collect(),
            }));
            for (r, _) in refs.iter().zip(&inl).filter(|(_, i)| **i) {
                out.extend(r.get_polygons().with_filter(l, d).build().polygons().map(|p| OwnedPolygon {
                    layer: p.layer(),
                    datatype: p.datatype(),
                    points: p.points().collect(),
                }));
            }
        }
        out
    };
    let clock = std::time::Instant::now();
    let mark = |what: &str| {
        if ctx.profile {
            eprintln!("[hier] {} {what}: {:?}", cell.name(), clock.elapsed());
        }
    };
    let (own, own_open) = crate::nets::cell_nets_with(ctx.lib, cell, ctx.rules, ctx.magic, None, &polys, true, true);
    mark("propio");
    if ctx.profile {
        for p in own.pieces.iter().filter(|p| p.poly.points.len() > 500) {
            eprintln!("[hier]   {}: pedazo {} de {} vértices", cell.name(), p.magic, p.poly.points.len());
        }
    }

    // Las instancias no metidas.
    let mut insts = Vec::new();
    for (r, _) in refs.iter().zip(&inl).filter(|(_, i)| !**i) {
        let Some(&key) = ctx.keys.get(r.cell_name()) else { continue };
        let Some(child) = cells.get(&key) else { continue };
        for off in offsets(r.repetition_count(), |i| r.repetition_offset(i)) {
            let Some(xf) = Xf::of(r, off) else { continue };
            insts.push(Inst { cell: r.cell_name().to_string(), key, xf, bbox: xf.bbox(&child.bbox) });
        }
    }
    let children: Vec<Arc<CellNets>> = insts.iter().map(|i| cells[&i.key].clone()).collect();

    // Nodos: las redes propias y, por instancia, las de la hija.
    let n_own = own.nets.len() as u32;
    let mut base = Vec::with_capacity(insts.len());
    let mut n = n_own;
    for c in &children {
        base.push(n);
        n += c.nets.len() as u32;
    }
    let mut uf = Uf((0..n).collect());
    let inst_grid = BoxGrid::new(&insts.iter().map(|i| i.bbox).collect::<Vec<_>>());
    let touch = ctx.reach;

    mark("instancias");
    // Lo propio contra las hijas: por instancia, en paralelo, una unión chica
    // con los pedazos propios que tocan su caja y los de la hija debajo.
    let own_grid = BoxGrid::new(&own.pieces.iter().map(|p| grow(bbox(&p.poly), touch)).collect::<Vec<_>>());
    let edges: Vec<(u64, u64)> = insts
        .par_iter()
        .enumerate()
        .flat_map_iter(|(k, inst)| {
            let near = own_grid.overlapping(inst.bbox);
            if near.is_empty() {
                return Vec::new();
            }
            let inv = inst.xf.inverse();
            let zone = grow(inst.bbox, touch);
            let t0 = std::time::Instant::now();
            let mut typed: Vec<Typed> = Vec::new();
            let mut seen: HashSet<(u32, u64, u64, usize)> = HashSet::new();
            for &i in &near {
                let p = &own.pieces[i];
                let types = ctx.conn.get(&p.magic).cloned().unwrap_or_else(|| vec![p.magic.clone()]);
                let area = meet(grow(bbox(&p.poly), touch), zone);
                let mut sub = Vec::new();
                children[k].pieces_in(cells, inv.bbox(&area), &types, &mut sub);
                for (net, t, poly) in sub {
                    let (x, y) = poly.points.first().map_or((0, 0), |q| (q.x.to_bits(), q.y.to_bits()));
                    if seen.insert((net, x, y, poly.points.len())) {
                        typed.push(Typed { magic: t, poly: inst.xf.poly(&poly), node: (base[k] + net) as u64 });
                    }
                }
            }
            if typed.is_empty() {
                return Vec::new();
            }
            let n_child = typed.len();
            let t1 = t0.elapsed();
            typed.extend(near.iter().map(|&i| {
                let p = &own.pieces[i];
                Typed { magic: p.magic.clone(), poly: p.poly.clone(), node: p.net as u64 }
            }));
            let e = touching(ctx.rules, &typed, Some(zone), ctx.unit_um);
            if ctx.profile && t0.elapsed().as_millis() > 100 {
                eprintln!(
                    "[hier]   {} → {}: {} propios, {} de la hija en {t1:?}, unión {:?}",
                    cell.name(),
                    inst.cell,
                    near.len(),
                    n_child,
                    t0.elapsed() - t1
                );
            }
            e
        })
        .collect();
    for (a, b) in edges {
        uf.join(a as u32, b as u32);
    }

    mark("propio-hijas");
    // Las hijas entre sí: por vecindad, memorizada.
    let close: Vec<(usize, usize)> = (0..insts.len())
        .flat_map(|k| inst_grid.overlapping(grow(insts[k].bbox, touch)).into_iter().filter(move |&m| m > k).map(move |m| (k, m)))
        .collect();
    let found: Vec<(usize, usize, Arc<Vec<(u32, u32)>>)> =
        close.par_iter().map(|&(k, m)| (k, m, neighbourhood(ctx, cells, &insts[k], &insts[m]))).collect();
    for (k, m, pairs) in found {
        for &(a, b) in pairs.iter() {
            uf.join(base[k] + a, base[m] + b);
        }
    }

    mark("vecindades");
    // El sustrato de los cuerpos sin pozo. Una hija sola no ve el pozo que
    // pone esta celda (el pozo P aislado de un `dnwell` dibujado arriba): sus
    // cuerpos "en el sustrato" (`sub_points`) van al pozo que haya acá en ese
    // punto, propio o de otra hija; los que no caen en ninguno siguen en el
    // sustrato y suben.
    let sub_types: Vec<String> =
        ctx.rules.substrate.0.iter().filter(|t| *t != "space").map(|t| ctx.rules.canonical(t).to_string()).collect();
    let own_wells: Vec<usize> = (0..own.pieces.len()).filter(|&i| sub_types.contains(&own.pieces[i].magic)).collect();
    let wells = Grid::new(own_wells.iter().map(|&i| own.pieces[i].poly.clone()).collect());
    let well_at = |p: (f64, f64), skip: Option<usize>| -> Option<u32> {
        if let Some(i) = wells.find(p.0, p.1) {
            return Some(own.pieces[own_wells[i as usize]].net as u32);
        }
        for m in inst_grid.overlapping([p.0, p.1, p.0, p.1]) {
            if Some(m) == skip {
                continue;
            }
            let local = insts[m].xf.inverse().apply(p);
            let mut found = Vec::new();
            children[m].pieces_in(cells, [local.0, local.1, local.0, local.1], &sub_types, &mut found);
            if let Some((n, _, _)) = found.iter().find(|(_, _, q)| point_in(&q.points, local.0, local.1)) {
                return Some(base[m] + n);
            }
        }
        None
    };
    let mut sub: Option<u32> = None;
    let mut sub_points: Vec<(f64, f64)> = Vec::new();
    let to_sub = |uf: &mut Uf, x: u32, sub: &mut Option<u32>| match *sub {
        Some(s) => uf.join(s, x),
        None => *sub = Some(x),
    };
    let own_sub: Vec<usize> = (0..own.nets.len()).filter(|&i| own.nets[i].substrate).collect();
    for &i in &own_sub {
        let pts: Vec<(f64, f64)> = own.devices.iter().filter(|(_, t)| t.b == i).map(|(d, _)| d.at).collect();
        let mut unresolved = pts.is_empty();
        for pt in pts {
            match well_at(pt, None) {
                Some(w) => uf.join(i as u32, w),
                None => {
                    unresolved = true;
                    sub_points.push(pt);
                }
            }
        }
        if unresolved {
            to_sub(&mut uf, i as u32, &mut sub);
        }
    }
    for (k, c) in children.iter().enumerate() {
        let Some(cs) = c.sub else { continue };
        let node = base[k] + cs;
        let mut unresolved = c.sub_points.is_empty();
        for &pt in &c.sub_points {
            let p = insts[k].xf.apply(pt);
            match well_at(p, Some(k)) {
                Some(w) => uf.join(node, w),
                None => {
                    unresolved = true;
                    sub_points.push(p);
                }
            }
        }
        if unresolved {
            to_sub(&mut uf, node, &mut sub);
        }
    }

    // Candidatos al sustrato (los pozos P): los propios y los de las hijas,
    // salvo los que algo de esta celda excluye (un `dnwell` propio o de otra
    // hija). Los que quedan, los decide la celda de arriba.
    let not_sub: Vec<String> = ctx.rules.substrate.1.iter().map(|t| ctx.rules.canonical(t).to_string()).collect();
    let excl = Grid::new(own.pieces.iter().filter(|p| not_sub.contains(&p.magic)).map(|p| p.poly.clone()).collect());
    let excluded_by_insts = |p: (f64, f64), skip: Option<usize>| {
        inst_grid.overlapping([p.0, p.1, p.0, p.1]).into_iter().filter(|&m| Some(m) != skip).any(|m| {
            let local = insts[m].xf.inverse().apply(p);
            let mut sub = Vec::new();
            children[m].pieces_in(cells, [local.0, local.1, local.0, local.1], &not_sub, &mut sub);
            sub.iter().any(|(_, _, q)| point_in(&q.points, local.0, local.1))
        })
    };
    let mut open_nodes: Vec<(u32, (f64, f64))> = Vec::new();
    for &(n, pt) in &own_open {
        if !excluded_by_insts(pt, None) {
            open_nodes.push((n as u32, pt));
        }
    }
    for (k, c) in children.iter().enumerate() {
        for &(n, pt) in &c.open {
            let p = insts[k].xf.apply(pt);
            if excl.find(p.0, p.1).is_none() && !excluded_by_insts(p, Some(k)) {
                open_nodes.push((base[k] + n, p));
            }
        }
    }

    // Las etiquetas propias que no cayeron en un pedazo propio: en las hijas.
    let near = 1e-3 / ctx.unit_um;
    let mut label_nodes: Vec<Option<u32>> = own.label_nets.iter().map(|n| n.map(|n| n as u32)).collect();
    for (li, l) in own.labels.iter().enumerate() {
        if label_nodes[li].is_some() {
            continue;
        }
        let mut probes: Vec<(f64, f64)> = vec![l.at];
        for (dx, dy) in [(0.0, -near), (0.0, near), (-near, 0.0), (near, 0.0)] {
            probes.push((l.at.0 + dx, l.at.1 + dy));
        }
        if let Some(a) = &l.area {
            probes.extend(area_points(a));
        }
        'found: for pt in probes {
            for k in inst_grid.overlapping([pt.0, pt.1, pt.0, pt.1]) {
                let mut sub = Vec::new();
                let local = insts[k].xf.inverse().apply(pt);
                children[k].pieces_in(cells, [local.0, local.1, local.0, local.1], &l.types, &mut sub);
                if let Some((net, _, _)) = sub.iter().find(|(_, _, p)| point_in(&p.points, local.0, local.1)) {
                    label_nodes[li] = Some(base[k] + net);
                    break 'found;
                }
            }
        }
    }

    mark("etiquetas");
    // Las redes de la celda: las raíces, en orden.
    let mut index: HashMap<u32, u32> = HashMap::new();
    let mut roots = Vec::new();
    for x in 0..n {
        let r = uf.find(x);
        if !index.contains_key(&r) {
            index.insert(r, roots.len() as u32);
            roots.push(r);
        }
    }
    let mut map = |x: u32| index[&uf.find(x)];
    let own_map: Vec<u32> = (0..n_own).map(&mut map).collect();
    let inst_maps: Vec<Vec<u32>> =
        children.iter().enumerate().map(|(k, c)| (0..c.nets.len() as u32).map(|i| map(base[k] + i)).collect()).collect();
    let label_nets: Vec<Option<usize>> = label_nodes.iter().map(|n| n.map(|n| map(n) as usize)).collect();
    let open: Vec<(u32, (f64, f64))> = open_nodes.iter().map(|&(n, pt)| (map(n), pt)).collect();
    let sub = sub.map(&mut map);
    let mut nets: Vec<Net> =
        roots.iter().map(|_| Net { name: None, labels: Vec::new(), port: false, substrate: false, bbox: EMPTY }).collect();
    for (i, net) in own.nets.iter().enumerate() {
        let t = &mut nets[own_map[i] as usize];
        t.bbox = union_box(t.bbox, net.bbox);
    }
    for (k, c) in children.iter().enumerate() {
        for (i, net) in c.nets.iter().enumerate() {
            let t = &mut nets[inst_maps[k][i] as usize];
            t.bbox = union_box(t.bbox, insts[k].xf.bbox(&net.bbox));
        }
    }
    if let Some(s) = sub {
        nets[s as usize].substrate = true;
    }
    let mut ports: HashSet<(usize, String)> = HashSet::new();
    for (l, ln) in own.labels.iter().zip(&label_nets) {
        let Some(i) = *ln else { continue };
        if !nets[i].labels.contains(&l.text) {
            nets[i].labels.push(l.text.clone());
        }
        if l.port {
            nets[i].port = true;
            ports.insert((i, l.text.clone()));
        }
    }
    let mut warnings = Vec::new();
    for (i, net) in nets.iter_mut().enumerate() {
        net.labels.sort();
        net.name = net.labels.iter().find(|t| ports.contains(&(i, (*t).clone()))).or(net.labels.first()).cloned();
        if net.labels.len() > 1 {
            warnings.push(format!("una red tiene más de un nombre: {}", net.labels.join(", ")));
        }
    }
    let mut unplaced: Vec<&str> =
        own.labels.iter().zip(&label_nets).filter(|(_, n)| n.is_none()).map(|(l, _)| l.text.as_str()).collect();
    unplaced.sort_unstable();
    unplaced.dedup();
    if !unplaced.is_empty() {
        warnings.push(format!("etiquetas fuera de una capa conductora de su tipo: {}", unplaced.join(", ")));
    }

    let mut by_type: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, p) in own.pieces.iter().enumerate() {
        by_type.entry(p.magic.clone()).or_default().push(i);
    }
    let index = by_type
        .into_iter()
        .map(|(t, idx)| {
            let boxes: Vec<[f64; 4]> = idx.iter().map(|&i| bbox(&own.pieces[i].poly)).collect();
            (t, (BoxGrid::new(&boxes), idx))
        })
        .collect();
    let bbox_all = own.pieces.iter().map(|p| bbox(&p.poly)).chain(insts.iter().map(|i| i.bbox)).fold(EMPTY, union_box);
    let deep_devices = own.devices.len() + children.iter().map(|c| c.deep_devices).sum::<usize>();
    mark("fin");
    CellNets {
        name: cell.name().to_string(),
        own,
        own_map,
        insts,
        inst_maps,
        nets,
        label_nets,
        bbox: bbox_all,
        deep_devices,
        open,
        sub,
        sub_points,
        warnings,
        index,
        inst_grid,
    }
}

/// Puntos dentro de un rectángulo de etiqueta (como `extract::area_points`).
fn area_points(a: &[f64; 4]) -> Vec<(f64, f64)> {
    let at = |i: usize, lo: f64, hi: f64| lo + (hi - lo) * (i as f64 + 0.5) / 5.0;
    std::iter::once(((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0))
        .chain((0..25).map(|k| (at(k % 5, a[0], a[2]), at(k / 5, a[1], a[3]))))
        .collect()
}

/// Los pares (red de `a`, red de `b`) que se tocan entre dos instancias.
fn neighbourhood(ctx: &Ctx<'_>, cells: &Cells, a: &Inst, b: &Inst) -> Arc<Vec<(u32, u32)>> {
    let rel = a.xf.inverse().then(&b.xf);
    let key = (a.key, b.key, rel.key(1e-4 / ctx.unit_um));
    let shared = !ctx.unique.contains(&a.key) && !ctx.unique.contains(&b.key);
    let known = if shared { memo().neighbour(&key) } else { ctx.local.lock().unwrap().get(&key).cloned() };
    if let Some(v) = known {
        ctx.stats.lock().unwrap().reused += 1;
        return v;
    }
    let clock = std::time::Instant::now();
    let (ca, cb) = (&cells[&a.key], &cells[&b.key]);
    let touch = ctx.reach;
    let region = {
        let bb = rel.bbox(&cb.bbox);
        let r = [ca.bbox[0].max(bb[0]), ca.bbox[1].max(bb[1]), ca.bbox[2].min(bb[2]), ca.bbox[3].min(bb[3])];
        grow(r, 2.0 * touch)
    };
    let mut pa = Vec::new();
    ca.pieces_in(cells, region, &ctx.conductors, &mut pa);
    let mut pb = Vec::new();
    cb.pieces_in(cells, rel.inverse().bbox(&region), &ctx.conductors, &mut pb);
    let side = 1u64 << 40;
    let mut typed: Vec<Typed> = pa.into_iter().map(|(n, t, p)| Typed { magic: t, poly: p, node: n as u64 }).collect();
    typed.extend(pb.into_iter().map(|(n, t, p)| Typed { magic: t, poly: rel.poly(&p), node: side + n as u64 }));
    let n_pieces = typed.len();
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    // Por componente (una cadena puede pasar por varios pedazos de los dos
    // lados): la primera red de `a` con cada una de `b`, y al revés.
    let edges = touching(ctx.rules, &typed, Some(region), ctx.unit_um);
    if !edges.is_empty() {
        let mut ids: Vec<u64> = edges.iter().flat_map(|&(x, y)| [x, y]).collect();
        ids.sort_unstable();
        ids.dedup();
        let pos = |x: u64| ids.binary_search(&x).map_or(0, |i| i as u32);
        let mut uf = Uf((0..ids.len() as u32).collect());
        for &(x, y) in &edges {
            uf.join(pos(x), pos(y));
        }
        let mut comps: BTreeMap<u32, (Vec<u32>, Vec<u32>)> = BTreeMap::new();
        for (i, &id) in ids.iter().enumerate() {
            let e = comps.entry(uf.find(i as u32)).or_default();
            if id < side {
                e.0.push(id as u32);
            } else {
                e.1.push((id - side) as u32);
            }
        }
        for (xs, ys) in comps.values() {
            if let (Some(&x0), Some(&y0)) = (xs.first(), ys.first()) {
                pairs.extend(ys.iter().map(|&y| (x0, y)));
                pairs.extend(xs.iter().map(|&x| (x, y0)));
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
    }
    if ctx.profile && clock.elapsed().as_millis() > 100 {
        eprintln!(
            "[hier]   vecindad {} ~ {}: {} pedazos, {} pares en {:?}",
            a.cell,
            b.cell,
            n_pieces,
            pairs.len(),
            clock.elapsed()
        );
    }
    let v = Arc::new(pairs);
    if shared {
        memo().put_neighbour(key, v.clone());
    } else {
        ctx.local.lock().unwrap().insert(key, v.clone());
    }
    ctx.stats.lock().unwrap().neighbourhoods += 1;
    v
}

/// La extracción jerárquica de una celda: su resumen, los de toda su
/// sub-jerarquía (por huella) y lo medido.
pub struct HierNets {
    pub root: Arc<CellNets>,
    pub cells: Cells,
    pub stats: Stats,
    /// La huella de cada celda de la librería.
    pub keys: HashMap<String, NetKey>,
}

impl HierNets {
    /// La `Netlist` plana (ver [`flatten`]).
    pub fn flatten(&self, with_pieces: bool) -> Netlist {
        flatten(&self.root, &self.cells, with_pieces)
    }
}

/// Las redes de `root` por celdas, con la memoria del proceso: una celda
/// con una huella ya vista (en esta librería u otra) no se vuelve a armar.
pub fn extract(
    lib: &Library,
    root: &Cell<'_>,
    rules: &DeviceRules,
    magic: Option<&gdstk_rs::magic::MagInfo>,
    opts: Options,
) -> HierNets {
    let mut conn: HashMap<String, Vec<String>> = HashMap::new();
    for (a, b) in &rules.connect {
        for x in a.iter().chain(b) {
            let e = conn.entry(x.clone()).or_insert_with(|| vec![x.clone()]);
            for y in a.iter().chain(b) {
                if y != "space" && !e.contains(y) {
                    e.push(y.clone());
                }
            }
        }
    }
    let conductors: Vec<String> = crate::nets::wanted_types(rules).iter().map(|t| rules.canonical(t).to_string()).collect();
    let unit_um = lib.unit() / 1e-6;
    let reach = conductors.iter().map(|t| rules.bridge(t)).fold(5e-4, f64::max) / unit_um;
    let mut sizes = HashMap::new();
    for c in lib.cells() {
        sizes.insert(c.name().to_string(), flat_polygon_estimate(lib, &c));
    }
    // Las huellas; una celda sin huella recibe una única para esta extracción.
    static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let nonce = NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut unique = HashSet::new();
    let keys: HashMap<String, NetKey> = net_keys(lib, magic, salt(lib, rules.fingerprint, opts.inline))
        .into_iter()
        .map(|(name, k)| {
            let k = k.unwrap_or_else(|| {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (nonce, &name).hash(&mut h);
                let k = NetKey(((h.finish() as u128) << 64) | nonce as u128);
                unique.insert(k);
                k
            });
            (name, k)
        })
        .collect();
    let ctx = Ctx {
        lib,
        rules,
        magic,
        opts,
        unit_um,
        conn,
        conductors,
        sizes,
        keys,
        unique,
        local: Mutex::new(HashMap::new()),
        stats: Mutex::new(Stats::default()),
        profile: std::env::var_os("RIKU_PROFILE").is_some(),
        reach,
    };
    let mut cells: Cells = HashMap::new();
    for level in levels(&ctx, root) {
        // Una celda por huella (dos nombres con el mismo contenido son una).
        let mut todo: Vec<(&String, NetKey)> = Vec::new();
        for name in &level {
            let k = ctx.keys[name];
            if !cells.contains_key(&k) && !todo.iter().any(|(_, t)| *t == k) {
                todo.push((name, k));
            }
        }
        let built: Vec<(NetKey, Arc<CellNets>, bool)> = todo
            .par_iter()
            .filter_map(|&(name, k)| {
                let cell = lib.find_cell(name)?;
                let build = || build_cell(&ctx, &cells, &cell);
                let (c, known) = if ctx.unique.contains(&k) { (Arc::new(build()), false) } else { memo().cell(k, build) };
                Some((k, c, known))
            })
            .collect();
        let mut st = ctx.stats.lock().unwrap();
        for (k, c, known) in built {
            if known {
                st.remembered += 1;
            } else {
                st.cells += 1;
            }
            cells.insert(k, c);
        }
    }
    let stats = *ctx.stats.lock().unwrap();
    let root = cells[&ctx.keys[root.name()]].clone();
    HierNets { root, cells, stats, keys: ctx.keys }
}

/// La `Netlist` plana de `root`: sus redes, y los transistores de toda la
/// jerarquía llevados a sus coordenadas, con sus terminales en esas redes.
pub fn flatten(root: &CellNets, cells: &Cells, with_pieces: bool) -> Netlist {
    // La raíz decide el sustrato: sus candidatos se unen a él.
    let n = root.nets.len();
    let mut uf = Uf((0..n as u32).collect());
    if let Some(&(first, _)) = root.open.first() {
        let target = root.sub.unwrap_or(first);
        for &(x, _) in &root.open {
            uf.join(target, x);
        }
    }
    let open: HashSet<u32> = root.open.iter().map(|&(x, _)| x).collect();
    let mut fin: Vec<u32> = Vec::with_capacity(n);
    let mut index: HashMap<u32, u32> = HashMap::new();
    let mut nets: Vec<Net> = Vec::new();
    for i in 0..n as u32 {
        let r = uf.find(i);
        let j = *index.entry(r).or_insert_with(|| {
            nets.push(Net { name: None, labels: Vec::new(), port: false, substrate: false, bbox: EMPTY });
            (nets.len() - 1) as u32
        });
        fin.push(j);
        let (src, dst) = (&root.nets[i as usize], &mut nets[j as usize]);
        for l in &src.labels {
            if !dst.labels.contains(l) {
                dst.labels.push(l.clone());
            }
        }
        dst.port |= src.port;
        dst.substrate |= src.substrate || open.contains(&i);
        dst.bbox = union_box(dst.bbox, src.bbox);
        if dst.name.is_none() {
            dst.name = src.name.clone();
        }
    }
    for net in &mut nets {
        net.labels.sort();
    }
    let mut out = Netlist {
        pieces: Vec::new(),
        nets,
        devices: Vec::new(),
        resistors: Vec::new(),
        labels: root.own.labels.clone(),
        label_nets: root.label_nets.iter().map(|l| l.map(|i| fin[i] as usize)).collect(),
        warnings: root.warnings.clone(),
    };
    fn walk(c: &CellNets, cells: &Cells, xf: Xf, to_root: &[u32], out: &mut Netlist, with_pieces: bool) {
        let m = |own: usize| to_root[c.own_map[own] as usize] as usize;
        for (d, t) in &c.own.devices {
            let mut d = d.clone();
            d.gate = xf.poly(&d.gate);
            d.at = xf.apply(d.at);
            d.sd_at = d.sd_at.iter().map(|&p| xf.apply(p)).collect();
            out.devices.push((d, crate::nets::Terminals { d: m(t.d), g: m(t.g), s: m(t.s), b: m(t.b) }));
        }
        for (r, e) in &c.own.resistors {
            let mut r = r.clone();
            r.body = xf.poly(&r.body);
            r.at = xf.apply(r.at);
            out.resistors.push((r, [m(e[0]), m(e[1])]));
        }
        if with_pieces {
            for p in &c.own.pieces {
                out.pieces.push(crate::nets::NetPiece { magic: p.magic.clone(), poly: xf.poly(&p.poly), net: m(p.net) });
            }
        }
        for (k, inst) in c.insts.iter().enumerate() {
            let child = &cells[&inst.key];
            let map: Vec<u32> = c.inst_maps[k].iter().map(|&n| to_root[n as usize]).collect();
            walk(child, cells, xf.then(&inst.xf), &map, out, with_pieces);
        }
    }
    walk(root, cells, Xf::IDENTITY, &fin, &mut out, with_pieces);
    // Orden determinista: por posición.
    let q = |v: f64| (v * 1e4).round() as i64;
    out.devices.sort_by_key(|(d, _)| (q(d.at.1), q(d.at.0), d.model.clone()));
    out
}

/// Para depurar: el camino de celdas hasta el transistor cuya compuerta
/// contiene `at` (coordenadas de `root`), con la red de cada terminal en
/// cada nivel.
pub fn locate(root: &CellNets, cells: &Cells, at: (f64, f64)) -> Vec<String> {
    fn walk(c: &CellNets, cells: &Cells, xf: Xf, at: (f64, f64), path: &mut Vec<String>) -> bool {
        let local = xf.inverse().apply(at);
        for (d, t) in &c.own.devices {
            if point_in(&d.gate.points, local.0, local.1) {
                let m = |i: usize| c.own_map[i];
                path.push(format!(
                    "{}: {} d={} g={} s={} b={} (propias {:?}; abiertas {:?}, sub {:?})",
                    c.name,
                    d.model,
                    m(t.d),
                    m(t.g),
                    m(t.s),
                    m(t.b),
                    (t.d, t.g, t.s, t.b),
                    c.open.iter().filter(|(n, _)| *n == m(t.b)).count(),
                    c.sub
                ));
                return true;
            }
        }
        for (k, inst) in c.insts.iter().enumerate() {
            let child = &cells[&inst.key];
            let x = xf.then(&inst.xf);
            if !point_in(&rect_points(&x.bbox(&child.bbox)), at.0, at.1) {
                continue;
            }
            if walk(child, cells, x, at, path) {
                let last = path.len() - 1;
                path.push(format!("{} [{k}] mapa: {:?}", c.name, &c.inst_maps[k][..c.inst_maps[k].len().min(40)]));
                let _ = last;
                return true;
            }
        }
        false
    }
    fn rect_points(b: &[f64; 4]) -> Vec<gdstk_rs::Point2D> {
        let p = |x, y| gdstk_rs::Point2D { x, y };
        vec![p(b[0], b[1]), p(b[2], b[1]), p(b[2], b[3]), p(b[0], b[3])]
    }
    let mut path = Vec::new();
    walk(root, cells, Xf::IDENTITY, at, &mut path);
    path
}
