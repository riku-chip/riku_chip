//! LVS manual y progresivo: qué transistor del esquemático es cuál del
//! layout lo dice el diseñador (o lo acepta de las sugerencias), queda en un
//! archivo versionado junto al diseño, y Riku comprueba lo que se deduce de
//! eso: los parámetros de cada par, las redes (un corto o un abierto
//! aparecen en cuanto dos vínculos se contradicen) y cuánto falta vincular.
//!
//! Un transistor del layout se nombra por su modelo y un punto de su
//! compuerta, en µm y en coordenadas de la celda comparada: no cambia si se
//! renumera nada ni si la celda se mueve en el chip. Si se movió todo dentro
//! de la celda, se busca el movimiento rígido que vuelve a alinear los
//! vínculos; lo que siga sin aparecer se busca por conectividad (el único
//! transistor del modelo correcto conectado a las redes ya vinculadas).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// Versión del archivo de vínculos.
pub const SCHEMA: &str = "riku-lvs-map/v1";

/// Distancia (µm) hasta la que un punto del archivo es la compuerta.
const TOL: f64 = 0.01;

// ─── Archivo ─────────────────────────────────────────────────────────────────

/// El archivo de vínculos de un par.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapFile {
    pub schema: String,
    pub schematic: String,
    pub layout: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(default, rename = "bind")]
    pub binds: Vec<Bind>,
}

/// Un transistor del esquemático y los del layout que lo forman (sus dedos).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bind {
    pub schematic: String,
    pub layout: Vec<LayoutRef>,
}

/// Un transistor del layout: su modelo y un punto de su compuerta (µm, en
/// coordenadas de la celda).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutRef {
    pub model: String,
    pub at: [f64; 2],
}

impl MapFile {
    pub fn new(schematic: &str, layout: &str, cell: Option<&str>) -> Self {
        MapFile { schema: SCHEMA.into(), schematic: schematic.into(), layout: layout.into(), cell: cell.map(str::to_string), binds: Vec::new() }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let map: MapFile = toml::from_str(text).map_err(|e| e.to_string())?;
        if map.schema != SCHEMA {
            return Err(format!("versión del archivo desconocida: {} (se espera {SCHEMA})", map.schema));
        }
        Ok(map)
    }

    /// El archivo como texto: un vínculo por bloque, en orden de nombre,
    /// para que el diff de un commit se lea bien.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# Qué transistor del esquemático es cuál del layout (riku: LVS manual).\n");
        out.push_str("# Cada transistor del layout: su modelo y un punto de su compuerta, en µm de la celda.\n");
        out.push_str(&format!("schema = {:?}\nschematic = {:?}\nlayout = {:?}\n", self.schema, self.schematic, self.layout));
        if let Some(c) = &self.cell {
            out.push_str(&format!("cell = {c:?}\n"));
        }
        let mut binds = self.binds.clone();
        binds.sort_by(|a, b| natural(&a.schematic).cmp(&natural(&b.schematic)));
        for b in &binds {
            out.push_str(&format!("\n[[bind]]\nschematic = {:?}\nlayout = [\n", b.schematic));
            let mut refs = b.layout.clone();
            refs.sort_by(|a, b| (a.at[0], a.at[1]).partial_cmp(&(b.at[0], b.at[1])).unwrap_or(std::cmp::Ordering::Equal));
            for r in refs {
                out.push_str(&format!("  {{ model = {:?}, at = [{:.3}, {:.3}] }},\n", r.model, r.at[0], r.at[1]));
            }
            out.push_str("]\n");
        }
        out
    }
}

/// Orden natural: `M2` antes que `M10`.
fn natural(s: &str) -> (String, u64, String) {
    let digits = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    let end = s[digits..].find(|c: char| !c.is_ascii_digit()).map_or(s.len(), |e| digits + e);
    (s[..digits].to_string(), s[digits..end].parse().unwrap_or(0), s[end..].to_string())
}

// ─── Transistores de cada lado ───────────────────────────────────────────────

/// Un transistor del esquemático: su instancia, modelo, redes (drenaje,
/// compuerta, fuente, cuerpo) y parámetros (µm).
#[derive(Clone, Debug, PartialEq)]
pub struct SchDevice {
    pub name: String,
    pub model: String,
    pub pins: [String; 4],
    pub w: Option<f64>,
    pub l: Option<f64>,
    pub m: f64,
}

/// Un transistor (un dedo) del layout.
#[derive(Clone, Debug, PartialEq)]
pub struct LayDevice {
    pub model: String,
    /// Un punto de la compuerta, en µm de la celda.
    pub at: (f64, f64),
    pub w: f64,
    pub l: f64,
    pub pins: [String; 4],
}

const D: usize = 0;
const G: usize = 1;
const S: usize = 2;
const B: usize = 3;

/// Los transistores del `.subckt top` de una netlist del esquemático. Los
/// nombres son los de sus instancias (`XM1` → `M1`, con `instance`).
pub fn schematic_devices(spice: &str, top: &str, instance: &dyn Fn(&str) -> Option<String>) -> Vec<SchDevice> {
    let mut lines: Vec<String> = Vec::new();
    for raw in spice.lines() {
        let t = raw.trim();
        match (t.strip_prefix('+'), lines.last_mut()) {
            (Some(rest), Some(last)) => {
                last.push(' ');
                last.push_str(rest);
            }
            _ => lines.push(t.to_string()),
        }
    }
    let mut inside = false;
    let mut out = Vec::new();
    for l in &lines {
        let low = l.to_ascii_lowercase();
        if low.starts_with(".subckt") {
            inside = l.split_whitespace().nth(1) == Some(top);
            continue;
        }
        if low.starts_with(".ends") {
            inside = false;
            continue;
        }
        let toks: Vec<&str> = l.split_whitespace().collect();
        let Some(first) = toks.first().filter(|_| inside) else { continue };
        if !matches!(first.chars().next(), Some('X' | 'x' | 'M' | 'm')) {
            continue;
        }
        let k = toks.iter().position(|t| t.contains('=')).unwrap_or(toks.len());
        // Nombre, cuatro redes y el modelo.
        if k != 6 || !is_mos(toks[k - 1]) {
            continue;
        }
        let params: HashMap<String, &str> = toks[k..]
            .iter()
            .filter_map(|t| t.split_once('='))
            .map(|(a, b)| (a.to_ascii_lowercase(), b))
            .collect();
        let num = |key: &str| params.get(key).and_then(|v| microns(v));
        out.push(SchDevice {
            name: instance(first).unwrap_or_else(|| first.to_string()),
            model: toks[k - 1].to_string(),
            pins: [toks[1].into(), toks[2].into(), toks[3].into(), toks[4].into()],
            w: num("w"),
            l: num("l"),
            m: params.get("m").or(params.get("mult")).and_then(|v| v.parse().ok()).unwrap_or(1.0),
        });
    }
    out
}

/// Los transistores de una netlist del layout, con sus redes por el nombre
/// con que aparecen en su SPICE.
pub fn layout_devices(n: &riku_mod_layout::nets::LayoutNetlist) -> Vec<LayDevice> {
    let nl = &n.netlist;
    nl.devices
        .iter()
        .map(|(d, t)| LayDevice {
            model: d.model.clone(),
            at: (round3(d.at.0 * n.unit_um), round3(d.at.1 * n.unit_um)),
            w: d.w_um,
            l: d.l_um,
            pins: [nl.net_name(t.d), nl.net_name(t.g), nl.net_name(t.s), nl.net_name(t.b)],
        })
        .collect()
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn is_mos(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.contains("fet") || m.contains("mos")
}

/// El mismo modelo, con o sin el prefijo de la librería (`sky130_fd_pr__`).
fn same_model(a: &str, b: &str) -> bool {
    let short = |m: &str| m.rsplit("__").next().unwrap_or(m).to_ascii_lowercase();
    short(a) == short(b)
}

/// Un número SPICE en µm: con sufijo (`0.5u`, `500n`) se convierte; sin
/// sufijo es µm (SKY130 usa `.option scale=1e-6`). `None` si es un
/// parámetro (`W_N`).
fn microns(s: &str) -> Option<f64> {
    let s = s.trim().trim_matches('\'');
    let end = (1..=s.len()).rev().find(|&i| s.is_char_boundary(i) && s[..i].parse::<f64>().is_ok())?;
    let v: f64 = s[..end].parse().ok()?;
    let suffix = s[end..].to_ascii_lowercase();
    if suffix.is_empty() {
        return Some(v);
    }
    let mult = if suffix.starts_with("meg") {
        1e6
    } else {
        match suffix.chars().next()? {
            'u' => 1e-6,
            'n' => 1e-9,
            'p' => 1e-12,
            'f' => 1e-15,
            'm' => 1e-3,
            'k' => 1e3,
            _ => return None,
        }
    };
    // Redondeado al pm: `500n` es 0.5, no 0.5000000000000001.
    Some((v * mult * 1e12).round() / 1e6)
}

/// Redes que pone la extracción o el netlister cuando no tienen nombre:
/// no sirven para emparejar por nombre.
fn auto_name(n: &str) -> bool {
    let num = |p: &str| n.strip_prefix(p).is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()));
    num("net") || num("n") || n == "VSUBS"
}

// ─── Chequeo ─────────────────────────────────────────────────────────────────

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
    /// El archivo con las posiciones al día (si algo se reubicó).
    pub updated: MapFile,
}

impl Check {
    /// Todo vinculado, sin contradicciones ni diferencias.
    pub fn clean(&self) -> bool {
        self.lost.is_empty()
            && self.unknown.is_empty()
            && self.params.is_empty()
            && self.models.is_empty()
            && self.shorts.is_empty()
            && self.opens.is_empty()
            && self.unbound_schematic.is_empty()
            && self.unbound_layout.is_empty()
    }
}

/// El transistor del layout en `at` (sin los ya usados).
fn find(lay: &[LayDevice], used: &HashSet<usize>, model: &str, at: (f64, f64)) -> Option<usize> {
    lay.iter()
        .enumerate()
        .filter(|(i, d)| !used.contains(i) && same_model(&d.model, model))
        .map(|(i, d)| (i, (d.at.0 - at.0).hypot(d.at.1 - at.1)))
        .filter(|&(_, dist)| dist <= TOL)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

fn orient((x, y): (f64, f64), o: u8) -> (f64, f64) {
    let (x, y) = if o >= 4 { (x, -y) } else { (x, y) };
    match o % 4 {
        0 => (x, y),
        1 => (-y, x),
        2 => (-x, -y),
        _ => (y, -x),
    }
}

/// El movimiento rígido que vuelve a ubicar la mayor cantidad de `refs`
/// (al menos dos y más de la mitad).
fn rigid(refs: &[&LayoutRef], lay: &[LayDevice], used: &HashSet<usize>) -> Option<Moved> {
    let mut best: Option<Moved> = None;
    for anchor in refs.iter().take(3) {
        for o in 0..8u8 {
            let p0 = orient((anchor.at[0], anchor.at[1]), o);
            for (i, d) in lay.iter().enumerate() {
                if used.contains(&i) || !same_model(&d.model, &anchor.model) {
                    continue;
                }
                let (dx, dy) = (d.at.0 - p0.0, d.at.1 - p0.1);
                let mut taken = used.clone();
                let mut count = 0;
                for r in refs {
                    let p = orient((r.at[0], r.at[1]), o);
                    if let Some(j) = find(lay, &taken, &r.model, (p.0 + dx, p.1 + dy)) {
                        taken.insert(j);
                        count += 1;
                    }
                }
                if best.is_none_or(|b| count > b.count) {
                    best = Some(Moved { orient: o, dx, dy, count });
                }
            }
        }
    }
    best.filter(|b| b.count >= 2 && b.count * 2 > refs.len())
}

/// Qué red del layout es cada una del esquemático, según los vínculos
/// ubicados (fuente y drenaje se pueden intercambiar).
fn net_pairs(pairs: &[(&SchDevice, Vec<usize>)], lay: &[LayDevice]) -> BTreeMap<String, BTreeSet<String>> {
    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (s, fingers) in pairs {
        for &i in fingers {
            let l = &lay[i];
            let has = |map: &BTreeMap<String, BTreeSet<String>>, a: &str, b: &str| map.get(a).is_some_and(|v| v.contains(b));
            let straight = has(&map, &s.pins[D], &l.pins[D]) as u8 + has(&map, &s.pins[S], &l.pins[S]) as u8;
            let crossed = has(&map, &s.pins[D], &l.pins[S]) as u8 + has(&map, &s.pins[S], &l.pins[D]) as u8;
            let (ld, ls) = if crossed > straight { (S, D) } else { (D, S) };
            for (a, b) in [(G, G), (B, B), (D, ld), (S, ls)] {
                map.entry(s.pins[a].clone()).or_default().insert(l.pins[b].clone());
            }
        }
    }
    map
}

/// Cuántas redes ya vinculadas de `s` coinciden con las de `l`, o `None` si
/// alguna se contradice (con fuente y drenaje en el orden que mejor encaje).
fn agreement(s: &SchDevice, l: &[String; 4], map: &BTreeMap<String, BTreeSet<String>>) -> Option<usize> {
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
                let i = find(lay, &used, &r.model, (r.at[0], r.at[1]));
                used.extend(i);
                i
            })
            .collect();
        found.push((bi, refs));
    }

    // 2. Lo que no está donde estaba: ¿se movió todo junto?
    let missing: Vec<(usize, usize)> =
        found.iter().enumerate().flat_map(|(k, (_, refs))| refs.iter().enumerate().filter(|(_, r)| r.is_none()).map(move |(j, _)| (k, j))).collect();
    if missing.len() >= 2 {
        let refs: Vec<&LayoutRef> = missing.iter().map(|&(k, j)| &map.binds[found[k].0].layout[j]).collect();
        if let Some(m) = rigid(&refs, lay, &used) {
            let mut count = 0;
            for &(k, j) in &missing {
                let r = &map.binds[found[k].0].layout[j];
                let p = orient((r.at[0], r.at[1]), m.orient);
                if let Some(i) = find(lay, &used, &r.model, (p.0 + m.dx, p.1 + m.dy)) {
                    used.insert(i);
                    found[k].1[j] = Some(i);
                    count += 1;
                }
            }
            c.moved = Some(Moved { count, ..m });
        }
    }

    // 3. Lo que sigue sin aparecer: el único candidato que encaja con las
    // redes ya vinculadas (todos los dedos que faltan de un vínculo juntos).
    loop {
        let pairs: Vec<(&SchDevice, Vec<usize>)> =
            found.iter().map(|(bi, refs)| (by_name[map.binds[*bi].schematic.as_str()], refs.iter().flatten().copied().collect())).collect();
        let nets = net_pairs(&pairs, lay);
        let mut changed = false;
        for (bi, refs) in found.iter_mut() {
            let want = refs.iter().filter(|r| r.is_none()).count();
            if want == 0 {
                continue;
            }
            let s = by_name[map.binds[*bi].schematic.as_str()];
            let model = &map.binds[*bi].layout.iter().zip(refs.iter()).find(|(_, r)| r.is_none()).map(|(l, _)| l.model.clone()).unwrap_or_default();
            let cands: Vec<usize> = (0..lay.len())
                .filter(|i| !used.contains(i) && same_model(&lay[*i].model, model))
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
                Some(i) => LayoutRef { model: lay[*i].model.clone(), at: [lay[*i].at.0, lay[*i].at.1] },
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

fn fmt(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

// ─── Sugerencias ─────────────────────────────────────────────────────────────

/// Vínculos que se deducen sin adivinar: desde las redes con el mismo
/// nombre en los dos lados (los pines) y las ya vinculadas, cada transistor
/// del esquemático que tiene un único grupo de dedos del layout que encaja.
/// Los simétricos (un par diferencial sin redes que los distingan) quedan
/// para elegir a mano.
pub fn suggest(map: &MapFile, sch: &[SchDevice], lay: &[LayDevice]) -> Vec<Bind> {
    let c = check(map, sch, lay);
    let mut used: HashSet<usize> = c.bound.iter().flat_map(|(_, f)| f.iter().copied()).collect();
    let mut done: HashSet<String> = c.bound.iter().map(|(n, _)| n.clone()).chain(c.lost.iter().map(|(n, _)| n.clone())).collect();
    let mut pairs: Vec<(&SchDevice, Vec<usize>)> = c.bound.iter().filter_map(|(n, f)| Some((sch.iter().find(|d| &d.name == n)?, f.clone()))).collect();

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
    let mut groups: BTreeMap<(String, i64, [String; 4]), Vec<usize>> = BTreeMap::new();
    for (i, d) in lay.iter().enumerate().filter(|(i, _)| !used.contains(i)) {
        let mut sd = [d.pins[D].clone(), d.pins[S].clone()];
        sd.sort();
        let key = (d.model.clone(), (d.l * 1000.0).round() as i64, [sd[0].clone(), d.pins[G].clone(), sd[1].clone(), d.pins[B].clone()]);
        groups.entry(key).or_default().push(i);
    }

    let mut out: Vec<Bind> = Vec::new();
    loop {
        let mut nets = net_pairs(&pairs, lay);
        for (k, v) in &seeds {
            nets.entry(k.clone()).or_default().extend(v.iter().cloned());
        }
        let mut new: Vec<(&SchDevice, Vec<usize>)> = Vec::new();
        for s in sch.iter().filter(|s| !done.contains(&s.name)) {
            let mut best: Vec<(usize, &Vec<usize>)> = Vec::new();
            for ((model, l, pins), fingers) in &groups {
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
                layout: fingers.iter().map(|&i| LayoutRef { model: lay[i].model.clone(), at: [lay[i].at.0, lay[i].at.1] }).collect(),
            });
            pairs.push((s, fingers));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: &str = "sky130_fd_pr__nfet_01v8";
    const P: &str = "sky130_fd_pr__pfet_01v8";

    fn sd(name: &str, model: &str, pins: [&str; 4], w: f64) -> SchDevice {
        SchDevice { name: name.into(), model: model.into(), pins: pins.map(str::to_string), w: Some(w), l: Some(0.5), m: 1.0 }
    }

    fn ld(model: &str, at: (f64, f64), pins: [&str; 4], w: f64) -> LayDevice {
        LayDevice { model: model.into(), at, w, l: 0.5, pins: pins.map(str::to_string) }
    }

    /// Un inversor con la salida por un buffer: M1/M2 el inversor, M3 un
    /// nfet que lleva `out` a `x`.
    fn sides() -> (Vec<SchDevice>, Vec<LayDevice>) {
        let sch = vec![
            sd("M1", N, ["out", "in", "VSS", "VSS"], 1.0),
            sd("M2", P, ["out", "in", "VDD", "VDD"], 2.0),
            sd("M3", N, ["x", "out", "VSS", "VSS"], 1.0),
        ];
        let lay = vec![
            ld(N, (1.0, 1.0), ["n1", "in", "VSS", "VSS"], 1.0),
            // M2 en dos dedos.
            ld(P, (1.0, 5.0), ["VDD", "in", "n1", "VDD"], 1.0),
            ld(P, (2.0, 5.0), ["n1", "in", "VDD", "VDD"], 1.0),
            ld(N, (4.0, 1.0), ["x", "n1", "VSS", "VSS"], 1.0),
        ];
        (sch, lay)
    }

    fn bind(s: &str, refs: &[(&str, f64, f64)]) -> Bind {
        Bind { schematic: s.into(), layout: refs.iter().map(|&(m, x, y)| LayoutRef { model: m.into(), at: [x, y] }).collect() }
    }

    fn full() -> MapFile {
        let mut m = MapFile::new("a.sch", "a.gds", None);
        m.binds = vec![
            bind("M1", &[(N, 1.0, 1.0)]),
            bind("M2", &[(P, 1.0, 5.0), (P, 2.0, 5.0)]),
            bind("M3", &[(N, 4.0, 1.0)]),
        ];
        m
    }

    #[test]
    fn todo_vinculado_y_coherente_es_limpio() {
        let (sch, lay) = sides();
        let c = check(&full(), &sch, &lay);
        assert!(c.clean(), "{c:#?}");
        assert_eq!(c.nets["out"], BTreeSet::from(["n1".to_string()]));
    }

    #[test]
    fn avanza_de_a_poco() {
        let (sch, lay) = sides();
        let mut m = full();
        m.binds.truncate(1);
        let c = check(&m, &sch, &lay);
        assert_eq!(c.unbound_schematic, ["M2", "M3"]);
        assert_eq!(c.unbound_layout.len(), 3);
        assert!(c.shorts.is_empty() && c.opens.is_empty() && c.params.is_empty());
    }

    #[test]
    fn un_corto_y_un_ancho_se_ven_con_los_vinculos() {
        let (sch, mut lay) = sides();
        // En el layout, la compuerta de M3 quedó en `in` (corto out–in) y
        // M1 es más angosto.
        lay[3].pins[G] = "in".into();
        lay[0].w = 0.5;
        let c = check(&full(), &sch, &lay);
        assert_eq!(c.shorts, [("in".to_string(), vec!["in".to_string(), "out".to_string()])]);
        assert_eq!(c.params, [("M1".to_string(), "W 1 ≠ 0.5".to_string())]);
    }

    #[test]
    fn mover_todo_dentro_de_la_celda_se_reubica_solo() {
        let (sch, lay) = sides();
        // Todo girado 90° y corrido (10, 20).
        let moved: Vec<LayDevice> = lay
            .iter()
            .map(|d| {
                let p = orient(d.at, 1);
                LayDevice { at: (round3(p.0 + 10.0), round3(p.1 + 20.0)), ..d.clone() }
            })
            .collect();
        let c = check(&full(), &sch, &moved);
        assert!(c.clean(), "{c:#?}");
        let m = c.moved.unwrap();
        assert_eq!((m.orient, m.count), (1, 4));
        assert_eq!(c.updated.binds[0].layout[0].at, [9.0, 21.0]);
    }

    #[test]
    fn uno_movido_solo_se_encuentra_por_conectividad() {
        let (sch, mut lay) = sides();
        lay[3].at = (40.0, 7.0);
        let c = check(&full(), &sch, &lay);
        assert!(c.clean(), "{c:#?}");
        assert_eq!(c.by_connectivity, ["M3"]);
    }

    #[test]
    fn sugiere_desde_los_pines_sin_adivinar() {
        let (sch, lay) = sides();
        let got = suggest(&MapFile::new("a.sch", "a.gds", None), &sch, &lay);
        let names: Vec<&str> = got.iter().map(|b| b.schematic.as_str()).collect();
        assert_eq!(names.len(), 3, "{got:#?}");
        let m2 = got.iter().find(|b| b.schematic == "M2").unwrap();
        assert_eq!(m2.layout.len(), 2, "los dos dedos juntos");
        // Con lo sugerido, todo queda limpio.
        let mut m = MapFile::new("a.sch", "a.gds", None);
        m.binds = got;
        assert!(check(&m, &sch, &lay).clean());
    }

    #[test]
    fn el_archivo_ida_y_vuelta() {
        let m = full();
        let text = m.to_text();
        assert!(text.contains("[[bind]]\nschematic = \"M2\""), "{text}");
        assert_eq!(MapFile::parse(&text).unwrap(), m);
    }

    #[test]
    fn lee_la_netlist_del_esquematico() {
        let spice = "** t\n.subckt t in out\nXM1 out in VSS VSS sky130_fd_pr__nfet_01v8 L=0.5 W=18 nf=4\n+ m=2\nR1 a b 1k\n.ends\n.subckt otra a\nXM9 a a a a sky130_fd_pr__nfet_01v8 L=1 W=1\n.ends\n";
        let d = schematic_devices(spice, "t", &|n| n.strip_prefix('X').map(str::to_string));
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].name.as_str(), d[0].w, d[0].l, d[0].m), ("M1", Some(18.0), Some(0.5), 2.0));
        assert_eq!(microns("0.5u"), Some(0.5));
        assert_eq!(microns("500n"), Some(0.5));
        assert_eq!(microns("1e-6"), Some(1e-6));
        assert_eq!(microns("W_N"), None);
    }
}
