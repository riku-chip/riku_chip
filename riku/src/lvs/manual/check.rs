//! Lo que se deduce de los vínculos: ubicarlos (también si el layout se movió), parámetros, cortos, abiertos y pines.

use super::*;
use crate::i18n::tr;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Un movimiento rígido: orientación (0–3: giro de 90° en sentido
/// antihorario, 4–7: además espejado en Y) y desplazamiento (µm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moved {
    pub orient: u8,
    pub dx: f64,
    pub dy: f64,
    /// Cuántos vínculos se reubicaron con él.
    pub count: usize,
}

/// Lo que se deduce de los vínculos.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Check {
    /// Cada vínculo ubicado: el transistor del esquemático y sus dedos del
    /// layout (índices en la lista del layout).
    pub bound: Vec<(String, Vec<usize>)>,
    /// Dedos de un vínculo que no se encontraron en el layout.
    pub lost: Vec<(String, LayoutRef)>,
    /// Vínculos a un transistor que el esquemático ya no tiene.
    pub unknown: Vec<String>,
    /// El movimiento rígido con que se reubicaron vínculos, si hubo.
    pub moved: Option<Moved>,
    /// Vínculos reubicados por conectividad.
    pub by_connectivity: Vec<String>,
    /// Vínculos reubicados por su sub-celda (se movió una instancia).
    pub by_cell: Vec<String>,
    /// Parámetros distintos: el transistor y qué (`W 4 ≠ 2`).
    pub params: Vec<(String, String)>,
    /// Modelos distintos.
    pub models: Vec<(String, String)>,
    /// Una red del layout a la que van varias del esquemático (un corto).
    pub shorts: Vec<(String, Vec<String>)>,
    /// Una red del esquemático repartida en varias del layout (un abierto).
    pub opens: Vec<(String, Vec<String>)>,
    /// Qué red del layout es cada una del esquemático.
    pub nets: BTreeMap<String, BTreeSet<String>>,
    pub unbound_schematic: Vec<String>,
    pub unbound_layout: Vec<usize>,
    /// Pines que no cuadran: el pin y qué le pasa.
    pub pins: Vec<(String, String)>,
    /// Lo que no se revisa (resistencias, capacitores, sub-circuitos).
    pub unchecked: Vec<String>,
    /// La celda parece movida, pero hay más de una forma de alinearla: no se
    /// reubicó nada (mejor perdido que mal vinculado).
    pub moved_ambiguous: bool,
    /// El archivo con las posiciones al día (si algo se reubicó).
    pub updated: MapFile,
}

impl Check {
    /// Los transistores todos vinculados y sin diferencias, sin cortos ni
    /// abiertos entre sus redes, y los pines en su lugar. No dice nada de
    /// lo que no se revisa ([`Check::unchecked`]).
    pub fn clean(&self) -> bool {
        self.lost.is_empty()
            && self.unknown.is_empty()
            && self.params.is_empty()
            && self.models.is_empty()
            && self.shorts.is_empty()
            && self.opens.is_empty()
            && self.unbound_schematic.is_empty()
            && self.unbound_layout.is_empty()
            && self.pins.is_empty()
    }

    /// Limpio y sin nada que quedara sin revisar.
    pub fn complete(&self) -> bool {
        self.clean() && self.unchecked.is_empty()
    }
}

/// Los pines: cada uno del esquemático tiene que estar en el layout y llegar
/// a la misma red que en el esquemático (según los vínculos); uno del
/// layout que el esquemático no tiene también se dice.
pub fn check_pins(c: &Check, sch_ports: &[String], lay_ports: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for p in sch_ports {
        match lay_ports.iter().find(|l| l.eq_ignore_ascii_case(p)) {
            None => out.push((p.clone(), tr!("lvs_map.pin_missing"))),
            Some(l) => {
                if let Some(set) = c.nets.get(p).filter(|set| !set.iter().any(|n| n.eq_ignore_ascii_case(l))) {
                    out.push((
                        p.clone(),
                        tr!("lvs_map.pin_elsewhere", nets = set.iter().cloned().collect::<Vec<_>>().join(", ")),
                    ));
                }
            }
        }
    }
    for l in lay_ports {
        if !sch_ports.iter().any(|p| p.eq_ignore_ascii_case(l)) {
            out.push((l.clone(), tr!("lvs_map.pin_extra")));
        }
    }
    out
}

pub(super) fn orient((x, y): (f64, f64), o: u8) -> (f64, f64) {
    let (x, y) = if o >= 4 { (x, -y) } else { (x, y) };
    match o % 4 {
        0 => (x, y),
        1 => (-y, x),
        2 => (-x, -y),
        _ => (y, -x),
    }
}

/// Los movimientos rígidos que vuelven a ubicar a más de la mitad de
/// `refs` (al menos dos), del que más ubica al que menos. Al estilo RANSAC:
/// cada hipótesis (un ancla sobre un dedo del layout, en una de las ocho
/// orientaciones) se descarta con dos testigos, y solo las que quedan se
/// cuentan con una muestra y, las mejores, con todos.
pub(super) fn rigid_candidates(refs: &[&LayoutRef], lay: &[LayDevice], idx: &Index, used: &HashSet<usize>) -> Vec<Moved> {
    if refs.len() < 2 {
        return Vec::new();
    }
    // Anclas del modelo menos repetido (menos hipótesis); testigos y muestra
    // repartidos entre todos.
    let mut order: Vec<&LayoutRef> = refs.to_vec();
    order.sort_by_key(|r| idx.model(&r.model).and_then(|m| idx.by_model.get(&m)).map_or(0, Vec::len));
    let anchors = &order[..order.len().min(3)];
    let witnesses: Vec<&LayoutRef> = refs.iter().copied().step_by((refs.len() / 3).max(1)).take(3).collect();
    let sample: Vec<&LayoutRef> = refs.iter().copied().step_by((refs.len() / 64).max(1)).take(64).collect();
    let hit = |r: &LayoutRef, o: u8, dx: f64, dy: f64| {
        let p = orient((r.at[0], r.at[1]), o);
        idx.find(lay, used, &r.model, (p.0 + dx, p.1 + dy)).is_some()
    };
    let mut seen: HashSet<(u8, i64, i64)> = HashSet::new();
    let mut scored: Vec<(usize, Moved)> = Vec::new();
    for anchor in anchors {
        let Some(cands) = idx.model(&anchor.model).and_then(|m| idx.by_model.get(&m)) else { continue };
        for o in 0..8u8 {
            let p0 = orient((anchor.at[0], anchor.at[1]), o);
            for &i in cands {
                if used.contains(&i) {
                    continue;
                }
                let (dx, dy) = (lay[i].at.0 - p0.0, lay[i].at.1 - p0.1);
                if !seen.insert((o, cell(dx), cell(dy))) {
                    continue;
                }
                let misses = witnesses.iter().filter(|w| !std::ptr::eq(**w, *anchor) && !hit(w, o, dx, dy)).count();
                if misses > 1 {
                    continue;
                }
                let n = sample.iter().filter(|r| hit(r, o, dx, dy)).count();
                if n * 2 > sample.len() {
                    scored.push((n, Moved { orient: o, dx, dy, count: 0 }));
                }
            }
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.truncate(8);
    // Con todos: cada dedo a lo sumo una vez.
    let mut all: Vec<Moved> = scored
        .into_iter()
        .filter_map(|(_, m)| {
            let mut taken = used.clone();
            let mut count = 0;
            for r in refs {
                let p = orient((r.at[0], r.at[1]), m.orient);
                if let Some(j) = idx.find(lay, &taken, &r.model, (p.0 + m.dx, p.1 + m.dy)) {
                    taken.insert(j);
                    count += 1;
                }
            }
            (count >= 2 && count * 2 > refs.len()).then_some(Moved { count, ..m })
        })
        .collect();
    all.sort_by(|a, b| b.count.cmp(&a.count));
    all
}

/// Cuántas contradicciones hay entre las redes de unos vínculos: redes del
/// esquemático repartidas en varias del layout, redes del layout con varias
/// del esquemático, y redes con nombre en los dos lados vinculadas a otra
/// con otro nombre (en un layout simétrico, el espejo encaja todos los
/// dedos pero cambia `Vp` por `Vn`).
pub(super) fn conflicts(pairs: &[(&SchDevice, Vec<usize>)], lay: &[LayDevice]) -> usize {
    let map = net_pairs(pairs, lay);
    let mut back: HashMap<&String, HashSet<&String>> = HashMap::new();
    let mut renamed = 0;
    for (s, ls) in &map {
        for l in ls {
            back.entry(l).or_default().insert(s);
            if !auto_name(s) && !auto_name(l) && !s.eq_ignore_ascii_case(l) {
                renamed += 1;
            }
        }
    }
    map.values().filter(|v| v.len() > 1).count() + back.values().filter(|v| v.len() > 1).count() + renamed
}

/// Qué red del layout es cada una del esquemático, según los vínculos
/// ubicados (fuente y drenaje se pueden intercambiar).
pub(super) fn net_pairs(pairs: &[(&SchDevice, Vec<usize>)], lay: &[LayDevice]) -> BTreeMap<String, BTreeSet<String>> {
    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let add = |map: &mut BTreeMap<String, BTreeSet<String>>, a: &str, b: &str| {
        map.entry(a.to_string()).or_default().insert(b.to_string());
    };
    // Compuerta y cuerpo no se intercambian: van primero.
    let fingers: Vec<(&SchDevice, &LayDevice)> = pairs.iter().flat_map(|(s, f)| f.iter().map(move |&i| (*s, &lay[i]))).collect();
    for (s, l) in &fingers {
        add(&mut map, &s.pins[G], &l.pins[G]);
        add(&mut map, &s.pins[B], &l.pins[B]);
    }
    // Fuente y drenaje: con una cola. Un dedo se decide por lo ya vinculado;
    // al decidirlo, solo se revisan los que comparten esas redes. Si no se
    // decide ninguno, el primero va derecho y se sigue.
    let mut touch: HashMap<&str, Vec<usize>> = HashMap::new();
    for (k, (s, _)) in fingers.iter().enumerate() {
        touch.entry(s.pins[D].as_str()).or_default().push(k);
        touch.entry(s.pins[S].as_str()).or_default().push(k);
    }
    let has = |map: &BTreeMap<String, BTreeSet<String>>, a: &str, b: &str| map.get(a).is_some_and(|v| v.contains(b));
    let mut decided = vec![false; fingers.len()];
    let mut queue: std::collections::VecDeque<usize> = (0..fingers.len()).collect();
    let mut first = 0;
    loop {
        while let Some(k) = queue.pop_front() {
            if decided[k] {
                continue;
            }
            let (s, l) = fingers[k];
            let straight = has(&map, &s.pins[D], &l.pins[D]) as u8 + has(&map, &s.pins[S], &l.pins[S]) as u8;
            let crossed = has(&map, &s.pins[D], &l.pins[S]) as u8 + has(&map, &s.pins[S], &l.pins[D]) as u8;
            if straight == crossed {
                continue;
            }
            let (ld, ls) = if crossed > straight { (S, D) } else { (D, S) };
            add(&mut map, &s.pins[D], &l.pins[ld]);
            add(&mut map, &s.pins[S], &l.pins[ls]);
            decided[k] = true;
            for net in [s.pins[D].as_str(), s.pins[S].as_str()] {
                queue.extend(touch[net].iter().copied().filter(|&j| !decided[j]));
            }
        }
        while first < fingers.len() && decided[first] {
            first += 1;
        }
        let Some(&(s, l)) = fingers.get(first) else { break };
        add(&mut map, &s.pins[D], &l.pins[D]);
        add(&mut map, &s.pins[S], &l.pins[S]);
        decided[first] = true;
        for net in [s.pins[D].as_str(), s.pins[S].as_str()] {
            queue.extend(touch[net].iter().copied().filter(|&j| !decided[j]));
        }
    }
    map
}

/// Cuántas redes ya vinculadas de `s` coinciden con las de `l`, o `None` si
/// alguna se contradice (con fuente y drenaje en el orden que mejor encaje).
pub(super) fn agreement(s: &SchDevice, l: &[String; 4], map: &BTreeMap<String, BTreeSet<String>>) -> Option<usize> {
    let score = |pairs: [(usize, usize); 4]| -> Option<usize> {
        let mut n = 0;
        for (a, b) in pairs {
            if let Some(set) = map.get(&s.pins[a]) {
                if !set.contains(&l[b]) {
                    return None;
                }
                n += 1;
            }
        }
        Some(n)
    };
    let straight = score([(G, G), (B, B), (D, D), (S, S)]);
    let crossed = score([(G, G), (B, B), (D, S), (S, D)]);
    straight.max(crossed)
}

/// Lo que se deduce de `map` con los transistores de cada lado.
pub fn check(map: &MapFile, sch: &[SchDevice], lay: &[LayDevice]) -> Check {
    let mut c = Check { updated: map.clone(), ..Check::default() };
    let by_name: HashMap<&str, &SchDevice> = sch.iter().map(|d| (d.name.as_str(), d)).collect();
    let idx = Index::new(lay);
    let mut used: HashSet<usize> = HashSet::new();

    // 1. Cada dedo en su lugar.
    let mut found: Vec<(usize, Vec<Option<usize>>)> = Vec::new();
    for (bi, b) in map.binds.iter().enumerate() {
        if !by_name.contains_key(b.schematic.as_str()) {
            c.unknown.push(b.schematic.clone());
            continue;
        }
        let refs = b
            .layout
            .iter()
            .map(|r| {
                let i = idx.find(lay, &used, &r.model, (r.at[0], r.at[1]));
                used.extend(i);
                i
            })
            .collect();
        found.push((bi, refs));
    }

    // 1b. Un dedo de una sub-celda que no está donde estaba: el único de esa
    // celda en la misma posición local (se movió su instancia).
    for (bi, refs) in found.iter_mut() {
        let b = &map.binds[*bi];
        let mut moved = false;
        for (slot, r) in refs.iter_mut().zip(&b.layout) {
            let (None, Some(cell), Some(local)) = (*slot, r.cell.as_deref(), r.local) else { continue };
            let cands = idx.find_local(lay, &used, &r.model, cell, local);
            if let [i] = cands[..] {
                *slot = Some(i);
                used.insert(i);
                moved = true;
            }
        }
        if moved {
            c.by_cell.push(b.schematic.clone());
        }
    }

    // 2. Lo que no está donde estaba: ¿se movió todo junto?
    let missing: Vec<(usize, usize)> = found
        .iter()
        .enumerate()
        .flat_map(|(k, (_, refs))| refs.iter().enumerate().filter(|(_, r)| r.is_none()).map(move |(j, _)| (k, j)))
        .collect();
    if missing.len() >= 2 {
        let refs: Vec<&LayoutRef> = missing.iter().map(|&(k, j)| &map.binds[found[k].0].layout[j]).collect();
        // Cada candidato: qué ubicaría y cuántas contradicciones dejaría. Un
        // arreglo regular de dedos admite varios (correr todo "un dedo"
        // también encaja casi todo): gana el que menos contradice, y si dos
        // empatan no se reubica nada.
        let cands = rigid_candidates(&refs, lay, &idx, &used);
        let best_count = cands.first().map_or(0, |m| m.count);
        let mut scored: Vec<(usize, Moved, Vec<(usize, usize, usize)>)> = cands
            .into_iter()
            .filter(|m| m.count * 5 >= best_count * 4)
            .map(|m| {
                let mut taken = used.clone();
                let mut assign = Vec::new();
                for &(k, j) in &missing {
                    let r = &map.binds[found[k].0].layout[j];
                    let p = orient((r.at[0], r.at[1]), m.orient);
                    if let Some(i) = idx.find(lay, &taken, &r.model, (p.0 + m.dx, p.1 + m.dy)) {
                        taken.insert(i);
                        assign.push((k, j, i));
                    }
                }
                let pairs: Vec<(&SchDevice, Vec<usize>)> = found
                    .iter()
                    .enumerate()
                    .map(|(k, (bi, refs))| {
                        let mut f: Vec<usize> = refs.iter().flatten().copied().collect();
                        f.extend(assign.iter().filter(|a| a.0 == k).map(|a| a.2));
                        (by_name[map.binds[*bi].schematic.as_str()], f)
                    })
                    .collect();
                (conflicts(&pairs, lay), Moved { count: assign.len(), ..m }, assign)
            })
            .collect();
        scored.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.count.cmp(&a.1.count)));
        let tied: Vec<usize> = scored
            .iter()
            .enumerate()
            .filter(|(_, x)| scored.first().is_some_and(|f| x.0 == f.0 && x.1.count == f.1.count))
            .map(|(i, _)| i)
            .collect();
        // Entre empatados, un desplazamiento sin giro es lo más común: si es
        // uno solo, ese.
        // Empatados que asignan exactamente lo mismo (p. ej. un espejo sobre
        // puntos alineados) no son una ambigüedad.
        let same_assignment = |a: usize, b: usize| {
            let key = |i: usize| {
                let mut v = scored[i].2.clone();
                v.sort_unstable();
                v
            };
            key(a) == key(b)
        };
        let chosen = match tied.as_slice() {
            [] => None,
            [only] => Some(*only),
            [first, rest @ ..] if rest.iter().all(|&r| same_assignment(*first, r)) => Some(*first),
            many => {
                let plain: Vec<usize> = many.iter().copied().filter(|&i| scored[i].1.orient == 0).collect();
                match plain.as_slice() {
                    [only] => Some(*only),
                    _ => None,
                }
            }
        };
        match chosen {
            Some(i) => {
                let (_, m, assign) = &scored[i];
                for &(k, j, idx) in assign {
                    used.insert(idx);
                    found[k].1[j] = Some(idx);
                }
                c.moved = Some(*m);
            }
            None => c.moved_ambiguous = !scored.is_empty(),
        }
    }

    // 3. Lo que sigue sin aparecer: el único candidato que encaja con las
    // redes ya vinculadas (todos los dedos que faltan de un vínculo juntos).
    loop {
        let pairs: Vec<(&SchDevice, Vec<usize>)> = found
            .iter()
            .map(|(bi, refs)| (by_name[map.binds[*bi].schematic.as_str()], refs.iter().flatten().copied().collect()))
            .collect();
        let nets = net_pairs(&pairs, lay);
        let mut changed = false;
        for (bi, refs) in found.iter_mut() {
            let want = refs.iter().filter(|r| r.is_none()).count();
            if want == 0 {
                continue;
            }
            let s = by_name[map.binds[*bi].schematic.as_str()];
            let model = &map.binds[*bi]
                .layout
                .iter()
                .zip(refs.iter())
                .find(|(_, r)| r.is_none())
                .map(|(l, _)| l.model.clone())
                .unwrap_or_default();
            let cands: Vec<usize> = idx
                .connected(s, model, &nets)
                .unwrap_or_default()
                .into_iter()
                .filter(|i| !used.contains(i))
                .filter(|&i| agreement(s, &lay[i].pins, &nets).is_some_and(|n| n >= 2))
                .collect();
            if cands.len() == want {
                for (slot, i) in refs.iter_mut().filter(|r| r.is_none()).zip(cands) {
                    *slot = Some(i);
                    used.insert(i);
                }
                c.by_connectivity.push(s.name.clone());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // 4. Lo que se deduce.
    for (bi, refs) in &found {
        let b = &map.binds[*bi];
        for (r, i) in b.layout.iter().zip(refs) {
            if i.is_none() {
                c.lost.push((b.schematic.clone(), r.clone()));
            }
        }
        let fingers: Vec<usize> = refs.iter().flatten().copied().collect();
        // Al día: cada dedo ubicado en su posición de ahora.
        c.updated.binds[*bi].layout = b
            .layout
            .iter()
            .zip(refs)
            .map(|(r, i)| match i {
                Some(i) => LayoutRef::of(&lay[*i]),
                None => r.clone(),
            })
            .collect();
        if fingers.is_empty() {
            continue;
        }
        let s = by_name[b.schematic.as_str()];
        if let Some(l) = fingers.iter().map(|&i| &lay[i]).find(|l| !same_model(&l.model, &s.model)) {
            c.models.push((s.name.clone(), format!("{} ≠ {}", s.model, l.model)));
        }
        let w_layout: f64 = fingers.iter().map(|&i| lay[i].w).sum();
        if let Some(w) = s.w.map(|w| w * s.m) {
            if (w - w_layout).abs() > 0.005_f64.max(w * 1e-3) {
                c.params.push((s.name.clone(), format!("W {} ≠ {}", fmt(w), fmt(w_layout))));
            }
        }
        if let Some(l) = s.l {
            if let Some(other) = fingers.iter().map(|&i| lay[i].l).find(|x| (x - l).abs() > 0.005_f64.max(l * 1e-3)) {
                c.params.push((s.name.clone(), format!("L {} ≠ {}", fmt(l), fmt(other))));
            }
        }
        c.bound.push((s.name.clone(), fingers));
    }
    let pairs: Vec<(&SchDevice, Vec<usize>)> = c.bound.iter().map(|(n, f)| (by_name[n.as_str()], f.clone())).collect();
    c.nets = net_pairs(&pairs, lay);
    let mut back: BTreeMap<&String, BTreeSet<&String>> = BTreeMap::new();
    for (s, ls) in &c.nets {
        if ls.len() > 1 {
            c.opens.push((s.clone(), ls.iter().cloned().collect()));
        }
        for l in ls {
            back.entry(l).or_default().insert(s);
        }
    }
    for (l, ss) in back {
        if ss.len() > 1 {
            c.shorts.push((l.clone(), ss.into_iter().cloned().collect()));
        }
    }
    let bound: HashSet<&str> = c.bound.iter().map(|(n, _)| n.as_str()).chain(c.lost.iter().map(|(n, _)| n.as_str())).collect();
    c.unbound_schematic = sch.iter().filter(|d| !bound.contains(d.name.as_str())).map(|d| d.name.clone()).collect();
    c.unbound_layout = (0..lay.len()).filter(|i| !used.contains(i)).collect();
    c
}

pub(super) fn fmt(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
