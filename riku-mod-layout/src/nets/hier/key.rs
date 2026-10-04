//! La huella de la extracción de cada celda (`NetKey`, ver
//! `docs/ronda-5/design.md`, D21): un árbol de Merkle como el de
//! `prints::tree_prints`, que además cubre lo que cambia las redes sin ser
//! geometría (etiquetas, puertos de Magic) y lo que las interpreta (las
//! reglas, la unidad, los nombres de las capas, el umbral para meter una
//! sub-celda en su padre y la versión de Riku).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use gdstk_rs::{Cell, Library};
use rayon::prelude::*;

use crate::prints::{polygon_hash, transform_hash};

/// La huella de la netlist de una celda: 128 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct NetKey(pub u128);

/// Lo común a todas las celdas de una librería que cambia su extracción.
pub(crate) fn salt(lib: &Library, rules_print: u64, inline: u64) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut names: Vec<(u32, u32, String)> = lib.layer_names().into_iter().map(|(t, n)| (t.layer, t.datatype, n)).collect();
    names.sort();
    (env!("CARGO_PKG_VERSION"), rules_print, inline, (lib.unit() * 1e15).round() as i64, names).hash(&mut h);
    h.finish()
}

fn wide(seed: u64, parts: &impl Hash) -> u128 {
    let half = |s: u64| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (s, parts).hash(&mut h);
        h.finish()
    };
    ((half(seed) as u128) << 64) | half(seed ^ 0x9e37_79b9_7f4a_7c15) as u128
}

/// Lo propio de una celda: sus polígonos, sus etiquetas y sus puertos de
/// Magic; y sus referencias (nombre de la hija y transformación). `None` en
/// las referencias si alguna transformación no se puede resumir.
type Local = (u64, Option<Vec<(String, u64)>>);

fn local(cell: &Cell<'_>, magic: Option<&gdstk_rs::magic::MagInfo>, buf: &mut Vec<(i64, i64)>) -> Local {
    let mut polys: Vec<u64> = cell.get_polygons().depth(0).build().polygons().map(|p| polygon_hash(&p, buf)).collect();
    polys.sort_unstable();
    let q = |v: f64| (v * 1e6).round() as i64;
    let mut labels: Vec<(u32, u32, String, i64, i64, u64)> = cell
        .labels()
        .map(|l| {
            let o = l.origin();
            (l.layer(), l.texttype(), l.text().into_owned(), q(o.x), q(o.y), l.repetition_count())
        })
        .collect();
    labels.sort();
    let ports: Vec<(String, [i64; 4])> = magic
        .and_then(|m| m.cells.iter().find(|c| c.name == cell.name()))
        .map(|c| c.ports.iter().map(|p| (p.name.clone(), p.rect_um.map(q))).collect())
        .unwrap_or_default();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (polys, labels, ports).hash(&mut h);
    let refs = cell.references().map(|r| Some((r.cell_name().to_string(), transform_hash(&r)?))).collect();
    (h.finish(), refs)
}

/// La `NetKey` de cada celda de `lib` (`None` si no se puede resumir: una
/// referencia circular o una repetición que no se sabe resumir).
pub(crate) fn net_keys(lib: &Library, magic: Option<&gdstk_rs::magic::MagInfo>, salt: u64) -> HashMap<String, Option<NetKey>> {
    let cells: Vec<Cell<'_>> = lib.cells().collect();
    let locals: Vec<Local> = cells.par_iter().map_init(Vec::new, |buf, c| local(c, magic, buf)).collect();
    let index: HashMap<&str, usize> = cells.iter().enumerate().map(|(i, c)| (c.name(), i)).collect();
    let mut memo: Vec<Option<Option<NetKey>>> = vec![None; cells.len()];
    let mut visiting = vec![false; cells.len()];

    fn resolve(
        i: usize,
        salt: u64,
        locals: &[Local],
        index: &HashMap<&str, usize>,
        memo: &mut Vec<Option<Option<NetKey>>>,
        visiting: &mut Vec<bool>,
    ) -> Option<NetKey> {
        if let Some(done) = memo[i] {
            return done;
        }
        if visiting[i] {
            return None;
        }
        visiting[i] = true;
        let (own, refs) = &locals[i];
        let result = refs.as_ref().and_then(|refs| {
            let mut children: Vec<(u128, u64)> = Vec::with_capacity(refs.len());
            for (name, t) in refs {
                let child = match index.get(name.as_str()) {
                    Some(&j) => resolve(j, salt, locals, index, memo, visiting)?.0,
                    None => wide(1, name),
                };
                children.push((child, *t));
            }
            children.sort_unstable();
            Some(NetKey(wide(salt, &(own, children))))
        });
        visiting[i] = false;
        memo[i] = Some(result);
        result
    }

    (0..cells.len()).map(|i| (cells[i].name().to_string(), resolve(i, salt, &locals, &index, &mut memo, &mut visiting))).collect()
}
