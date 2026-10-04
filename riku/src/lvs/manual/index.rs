//! El índice de los dedos del layout (por posición, por posición en su sub-celda y por red).

use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Los dedos del layout indexados: por modelo y posición (en una grilla del
/// tamaño de la tolerancia), por modelo y posición en su sub-celda, por
/// modelo, y por modelo y red (de compuerta, de fuente o drenaje). Ubicar o
/// buscar candidatos mira unas pocas entradas en vez de todos los dedos.
pub(super) struct Index {
    pub(super) models: HashMap<String, u32>,
    pub(super) at: HashMap<(u32, i64, i64), Vec<usize>>,
    pub(super) local: HashMap<(u32, String, i64, i64), Vec<usize>>,
    pub(super) by_model: HashMap<u32, Vec<usize>>,
    pub(super) gate: HashMap<(u32, String), Vec<usize>>,
    pub(super) sd: HashMap<(u32, String), Vec<usize>>,
}

/// El modelo sin el prefijo de la librería, en minúsculas.
pub(super) fn short(m: &str) -> String {
    m.rsplit("__").next().unwrap_or(m).to_ascii_lowercase()
}

/// La casilla de la grilla de un valor (µm).
pub(super) fn cell(v: f64) -> i64 {
    (v / TOL).floor() as i64
}

impl Index {
    pub(super) fn new(lay: &[LayDevice]) -> Self {
        let mut ix = Index {
            models: HashMap::new(),
            at: HashMap::new(),
            local: HashMap::new(),
            by_model: HashMap::new(),
            gate: HashMap::new(),
            sd: HashMap::new(),
        };
        for (i, d) in lay.iter().enumerate() {
            let next = ix.models.len() as u32;
            let m = *ix.models.entry(short(&d.model)).or_insert(next);
            ix.at.entry((m, cell(d.at.0), cell(d.at.1))).or_default().push(i);
            if let Some(c) = &d.cell {
                ix.local.entry((m, c.clone(), cell(d.local.0), cell(d.local.1))).or_default().push(i);
            }
            ix.by_model.entry(m).or_default().push(i);
            ix.gate.entry((m, d.pins[G].clone())).or_default().push(i);
            ix.sd.entry((m, d.pins[D].clone())).or_default().push(i);
            if d.pins[S] != d.pins[D] {
                ix.sd.entry((m, d.pins[S].clone())).or_default().push(i);
            }
        }
        ix
    }

    pub(super) fn model(&self, m: &str) -> Option<u32> {
        self.models.get(&short(m)).copied()
    }

    /// Los dedos libres de ese modelo a menos de la tolerancia de `p` (en la
    /// grilla `grid`, con la posición que da `pos`).
    pub(super) fn near<K: std::hash::Hash + Eq>(
        grid: &HashMap<K, Vec<usize>>,
        key: impl Fn(i64, i64) -> K,
        p: (f64, f64),
        pos: impl Fn(usize) -> (f64, f64),
        used: &HashSet<usize>,
    ) -> Vec<(usize, f64)> {
        let (cx, cy) = (cell(p.0), cell(p.1));
        let mut out = Vec::new();
        for kx in cx - 1..=cx + 1 {
            for ky in cy - 1..=cy + 1 {
                for &i in grid.get(&key(kx, ky)).into_iter().flatten() {
                    let q = pos(i);
                    let d = (q.0 - p.0).hypot(q.1 - p.1);
                    if !used.contains(&i) && d <= TOL {
                        out.push((i, d));
                    }
                }
            }
        }
        out
    }

    /// El dedo libre de ese modelo en `at` (el más cercano).
    pub(super) fn find(&self, lay: &[LayDevice], used: &HashSet<usize>, model: &str, at: (f64, f64)) -> Option<usize> {
        let m = self.model(model)?;
        Self::near(&self.at, |x, y| (m, x, y), at, |i| lay[i].at, used).into_iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
    }

    /// Los dedos libres de ese modelo en la posición `local` de la sub-celda.
    pub(super) fn find_local(&self, lay: &[LayDevice], used: &HashSet<usize>, model: &str, cell_name: &str, local: [f64; 2]) -> Vec<usize> {
        let Some(m) = self.model(model) else { return Vec::new() };
        Self::near(&self.local, |x, y| (m, cell_name.to_string(), x, y), (local[0], local[1]), |i| lay[i].local, used).into_iter().map(|(i, _)| i).collect()
    }

    /// Los dedos de ese modelo que pueden ser `s` según las redes ya
    /// vinculadas: tienen que coincidir en cada terminal vinculado, así que
    /// alcanza con la lista del más selectivo. `None` si no hay ninguno
    /// vinculado (aparte del cuerpo).
    pub(super) fn connected(&self, s: &SchDevice, model: &str, nets: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<usize>> {
        let m = self.model(model)?;
        let list = |index: &HashMap<(u32, String), Vec<usize>>, t: usize| -> Option<Vec<usize>> {
            let set = nets.get(&s.pins[t])?;
            let mut v: Vec<usize> = set.iter().flat_map(|n| index.get(&(m, n.clone())).into_iter().flatten().copied()).collect();
            v.sort_unstable();
            v.dedup();
            Some(v)
        };
        [list(&self.gate, G), list(&self.sd, D), list(&self.sd, S)].into_iter().flatten().min_by_key(Vec::len)
    }
}
