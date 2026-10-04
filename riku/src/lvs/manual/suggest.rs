//! Las sugerencias: los vínculos que se deducen sin adivinar.

use super::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Vínculos que se deducen sin adivinar: desde las redes con el mismo
/// nombre en los dos lados (los pines) y las ya vinculadas, cada transistor
/// del esquemático que tiene un único grupo de dedos del layout que encaja.
/// Los simétricos (un par diferencial sin redes que los distingan) quedan
/// para elegir a mano.
pub fn suggest(map: &MapFile, sch: &[SchDevice], lay: &[LayDevice]) -> Vec<Bind> {
    let c = check(map, sch, lay);
    let mut used: HashSet<usize> = c.bound.iter().flat_map(|(_, f)| f.iter().copied()).collect();
    let mut done: HashSet<String> = c.bound.iter().map(|(n, _)| n.clone()).chain(c.lost.iter().map(|(n, _)| n.clone())).collect();
    let by_name: HashMap<&str, &SchDevice> = sch.iter().map(|d| (d.name.as_str(), d)).collect();
    let mut pairs: Vec<(&SchDevice, Vec<usize>)> = c.bound.iter().filter_map(|(n, f)| Some((*by_name.get(n.as_str())?, f.clone()))).collect();

    // Semillas: las redes con nombre en los dos lados.
    let lay_names: HashSet<&str> = lay.iter().flat_map(|d| d.pins.iter().map(String::as_str)).collect();
    let seeds: BTreeMap<String, BTreeSet<String>> = sch
        .iter()
        .flat_map(|d| d.pins.iter())
        .filter(|n| !auto_name(n) && lay_names.contains(n.as_str()))
        .map(|n| (n.clone(), BTreeSet::from([n.clone()])))
        .collect();

    // Los dedos del layout en paralelo (mismo modelo, L, compuerta, cuerpo y
    // par fuente/drenaje) van juntos.
    let mut by_key: BTreeMap<(String, i64, [String; 4]), Vec<usize>> = BTreeMap::new();
    for (i, d) in lay.iter().enumerate().filter(|(i, _)| !used.contains(i)) {
        let mut sd = [d.pins[D].clone(), d.pins[S].clone()];
        sd.sort();
        let key = (d.model.clone(), (d.l * 1000.0).round() as i64, [sd[0].clone(), d.pins[G].clone(), sd[1].clone(), d.pins[B].clone()]);
        by_key.entry(key).or_default().push(i);
    }
    let groups: Vec<((String, i64, [String; 4]), Vec<usize>)> = by_key.into_iter().collect();
    // Los grupos por (modelo, red de compuerta) y (modelo, red de fuente o
    // drenaje), con el mismo truco que `Index::connected`.
    let mut g_index: HashMap<(String, String), Vec<usize>> = HashMap::new();
    let mut sd_index: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (k, ((model, _, pins), _)) in groups.iter().enumerate() {
        let m = short(model);
        g_index.entry((m.clone(), pins[1].clone())).or_default().push(k);
        sd_index.entry((m.clone(), pins[0].clone())).or_default().push(k);
        if pins[2] != pins[0] {
            sd_index.entry((m, pins[2].clone())).or_default().push(k);
        }
    }

    let mut out: Vec<Bind> = Vec::new();
    loop {
        let mut nets = net_pairs(&pairs, lay);
        for (k, v) in &seeds {
            nets.entry(k.clone()).or_default().extend(v.iter().cloned());
        }
        let mut new: Vec<(&SchDevice, Vec<usize>)> = Vec::new();
        for s in sch.iter().filter(|s| !done.contains(&s.name)) {
            let m = short(&s.model);
            let list = |index: &HashMap<(String, String), Vec<usize>>, t: usize| -> Option<Vec<usize>> {
                let set = nets.get(&s.pins[t])?;
                let mut v: Vec<usize> = set.iter().flat_map(|n| index.get(&(m.clone(), n.clone())).into_iter().flatten().copied()).collect();
                v.sort_unstable();
                v.dedup();
                Some(v)
            };
            let Some(cands) = [list(&g_index, G), list(&sd_index, D), list(&sd_index, S)].into_iter().flatten().min_by_key(Vec::len) else { continue };
            let mut best: Vec<(usize, &Vec<usize>)> = Vec::new();
            for &k in &cands {
                let ((model, l, pins), fingers) = &groups[k];
                if fingers.iter().any(|i| used.contains(i)) || !same_model(model, &s.model) {
                    continue;
                }
                if s.l.is_some_and(|sl| ((sl * 1000.0).round() as i64 - l).abs() > 5) {
                    continue;
                }
                let Some(n) = agreement(s, &[pins[0].clone(), pins[1].clone(), pins[2].clone(), pins[3].clone()], &nets) else { continue };
                if n < 2 {
                    continue;
                }
                match best.first().map(|b| b.0) {
                    Some(m) if n < m => {}
                    Some(m) if n == m => best.push((n, fingers)),
                    _ => best = vec![(n, fingers)],
                }
            }
            if let [(_, fingers)] = best.as_slice() {
                new.push((s, (*fingers).clone()));
            }
        }
        // Dos transistores que eligieron el mismo grupo: ninguno es seguro.
        let mut claims: HashMap<usize, usize> = HashMap::new();
        for (_, f) in &new {
            *claims.entry(f[0]).or_default() += 1;
        }
        new.retain(|(_, f)| claims[&f[0]] == 1);
        if new.is_empty() {
            break;
        }
        for (s, fingers) in new {
            used.extend(fingers.iter().copied());
            done.insert(s.name.clone());
            out.push(Bind {
                schematic: s.name.clone(),
                layout: fingers.iter().map(|&i| LayoutRef::of(&lay[i])).collect(),
            });
            pairs.push((s, fingers));
        }
    }
    out
}
