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

use crate::i18n::tr;

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

/// Un transistor del layout: su modelo y el centro de su compuerta (µm, en
/// coordenadas de la celda comparada). Si está dibujado en una sub-celda,
/// también esa celda y el punto en sus coordenadas: así se lo reencuentra
/// aunque se mueva su instancia.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutRef {
    pub model: String,
    pub at: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<[f64; 2]>,
}

impl LayoutRef {
    /// La referencia a un transistor del layout tal como está ahora.
    pub fn of(d: &LayDevice) -> Self {
        let sub = d.cell.is_some();
        LayoutRef { model: d.model.clone(), at: [d.at.0, d.at.1], cell: d.cell.clone(), local: sub.then_some([d.local.0, d.local.1]) }
    }
}

impl MapFile {
    pub fn new(schematic: &str, layout: &str, cell: Option<&str>) -> Self {
        MapFile { schema: SCHEMA.into(), schematic: schematic.into(), layout: layout.into(), cell: cell.map(str::to_string), binds: Vec::new() }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let map: MapFile = toml::from_str(text).map_err(|e| e.to_string())?;
        if map.schema != SCHEMA {
            return Err(tr!("lvs_map.bad_schema", found = map.schema, want = SCHEMA));
        }
        Ok(map)
    }

    /// Los vínculos en orden de nombre (`M2` antes que `M10`).
    pub fn sort(&mut self) {
        self.binds.sort_by(|a, b| natural(&a.schematic).cmp(&natural(&b.schematic)));
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
                let sub = match (&r.cell, r.local) {
                    (Some(c), Some(l)) => format!(", cell = {c:?}, local = [{:.3}, {:.3}]", l[0], l[1]),
                    _ => String::new(),
                };
                out.push_str(&format!("  {{ model = {:?}, at = [{:.3}, {:.3}]{sub} }},\n", r.model, r.at[0], r.at[1]));
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
    /// El centro del recuadro de la compuerta, en µm de la celda.
    pub at: (f64, f64),
    /// El recuadro de la compuerta (µm): `[x0, y0, x1, y1]`.
    pub gate: [f64; 4],
    /// La sub-celda donde está dibujado (`None`: en la celda comparada) y
    /// el centro de la compuerta en sus coordenadas (µm).
    pub cell: Option<String>,
    pub local: (f64, f64),
    pub w: f64,
    pub l: f64,
    pub pins: [String; 4],
}

const D: usize = 0;
const G: usize = 1;
const S: usize = 2;
const B: usize = 3;

/// Los transistores del `.subckt top` de una netlist del esquemático y los
/// de sus sub-circuitos (ver [`flatten`]). Los de arriba se nombran como sus
/// instancias (`XM1` → `M1`, con `instance`); los de adentro, con su ruta
/// (`x1/M3`).
pub fn schematic_devices(spice: &str, top: &str, instance: &dyn Fn(&str) -> Option<String>) -> Vec<SchDevice> {
    flatten(spice, top, instance).0
}

/// Un `.subckt`: sus pines, sus parámetros por defecto y sus líneas.
struct Subckt {
    ports: Vec<String>,
    params: HashMap<String, String>,
    lines: Vec<String>,
}

/// Los `.subckt` de una netlist (con las `+` ya unidas) y sus redes globales.
fn subckts(spice: &str) -> (HashMap<String, Subckt>, HashSet<String>) {
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
    let (mut all, mut globals): (HashMap<String, Subckt>, HashSet<String>) = (HashMap::new(), HashSet::from(["0".to_string()]));
    let mut current: Option<(String, Subckt)> = None;
    for l in lines {
        let low = l.to_ascii_lowercase();
        let toks: Vec<&str> = l.split_whitespace().collect();
        if low.starts_with(".subckt") && toks.len() >= 2 {
            let ports = toks[2..].iter().filter(|t| !t.contains('=')).map(|t| t.to_string()).collect();
            let params = toks[2..].iter().filter_map(|t| t.split_once('=')).map(|(a, b)| (a.to_string(), b.to_string())).collect();
            current = Some((toks[1].to_string(), Subckt { ports, params, lines: Vec::new() }));
        } else if low.starts_with(".ends") {
            if let Some((name, sub)) = current.take() {
                all.insert(name, sub);
            }
        } else if low.starts_with(".global") {
            globals.extend(toks[1..].iter().map(|t| t.to_string()));
        } else if let Some((_, sub)) = current.as_mut() {
            if !l.is_empty() && !l.starts_with('*') && !l.starts_with('.') {
                sub.lines.push(l);
            }
        }
    }
    (all, globals)
}

/// Los transistores del `.subckt top` y de todo lo que instancia, aplanados:
/// los de un sub-circuito con su ruta (`x1/M3`), sus redes internas con
/// prefijo (`x1/net2`) y sus pines y parámetros los de la instancia. Y lo
/// que no es un transistor (lo que no se revisa).
fn flatten(spice: &str, top: &str, instance: &dyn Fn(&str) -> Option<String>) -> (Vec<SchDevice>, Vec<String>) {
    let (all, globals) = subckts(spice);
    let mut devices = Vec::new();
    let mut others = Vec::new();
    if let Some(t) = all.get(top) {
        let nets: HashMap<String, String> = t.ports.iter().map(|p| (p.clone(), p.clone())).collect();
        walk(&all, &globals, t, "", &nets, &t.params, instance, 0, &mut devices, &mut others);
    }
    (devices, others)
}

#[allow(clippy::too_many_arguments)]
fn walk(
    all: &HashMap<String, Subckt>,
    globals: &HashSet<String>,
    cell: &Subckt,
    prefix: &str,
    nets: &HashMap<String, String>,
    params: &HashMap<String, String>,
    instance: &dyn Fn(&str) -> Option<String>,
    depth: usize,
    devices: &mut Vec<SchDevice>,
    others: &mut Vec<String>,
) {
    let net = |n: &str| match nets.get(n) {
        Some(m) => m.clone(),
        None if globals.contains(n) => n.to_string(),
        None => format!("{prefix}{n}"),
    };
    // El nombre de una instancia: arriba, el que da el netlister; adentro,
    // sin la `X` que Xschem antepone a los transistores (`XM3` → `M3`).
    let local_name = |n: &str| -> String {
        if prefix.is_empty() {
            return instance(n).unwrap_or_else(|| n.to_string());
        }
        let mut c = n.chars();
        match (c.next(), c.next()) {
            (Some('X' | 'x'), Some('M' | 'm')) => n[1..].to_string(),
            _ => n.to_string(),
        }
    };
    let value = |v: &str| -> String { params.get(v).cloned().unwrap_or_else(|| v.to_string()) };
    for l in &cell.lines {
        let toks: Vec<&str> = l.split_whitespace().collect();
        let Some(first) = toks.first() else { continue };
        let k = toks.iter().position(|t| t.contains('=')).unwrap_or(toks.len());
        let name = format!("{prefix}{}", local_name(first));
        if is_transistor(&toks) {
            let p: HashMap<String, String> = toks[k..].iter().filter_map(|t| t.split_once('=')).map(|(a, b)| (a.to_ascii_lowercase(), value(b))).collect();
            let num = |key: &str| p.get(key).and_then(|v| microns(v));
            devices.push(SchDevice {
                name,
                model: toks[k - 1].to_string(),
                pins: [net(toks[1]), net(toks[2]), net(toks[3]), net(toks[4])],
                w: num("w"),
                l: num("l"),
                m: p.get("m").or(p.get("mult")).and_then(|v| v.parse().ok()).unwrap_or(1.0),
            });
            continue;
        }
        // Un sub-circuito conocido: adentro, con sus pines a estas redes.
        let model = (k >= 2).then(|| toks[k - 1]);
        let sub = model.filter(|_| matches!(first.chars().next(), Some('X' | 'x'))).and_then(|m| all.get(m));
        match sub {
            Some(sub) if depth < 32 && sub.ports.len() == k - 2 => {
                let inner: HashMap<String, String> = sub.ports.iter().cloned().zip(toks[1..k - 1].iter().map(|n| net(n))).collect();
                let mut p = sub.params.clone();
                for (a, b) in toks[k..].iter().filter_map(|t| t.split_once('=')) {
                    p.insert(a.to_string(), value(b));
                }
                walk(all, globals, sub, &format!("{name}/"), &inner, &p, instance, depth + 1, devices, others);
            }
            _ => others.push(match model.filter(|_| matches!(first.chars().next(), Some('X' | 'x'))) {
                Some(m) => format!("{name} ({m})"),
                None => name,
            }),
        }
    }
}

/// Un transistor: `M`/`X`, cuatro redes y un modelo MOS.
fn is_transistor(toks: &[&str]) -> bool {
    let k = toks.iter().position(|t| t.contains('=')).unwrap_or(toks.len());
    matches!(toks.first().and_then(|t| t.chars().next()), Some('X' | 'x' | 'M' | 'm')) && k == 6 && is_mos(toks[k - 1])
}

/// Los pines del `.subckt top` y lo que tiene adentro que no es un
/// transistor (resistencias, capacitores, sub-circuitos): lo que el LVS
/// manual no revisa.
pub fn schematic_extras(spice: &str, top: &str, instance: &dyn Fn(&str) -> Option<String>) -> (Vec<String>, Vec<String>) {
    let ports = spice
        .lines()
        .map(str::trim)
        .find(|l| l.to_ascii_lowercase().starts_with(".subckt") && l.split_whitespace().nth(1) == Some(top))
        .map(|l| l.split_whitespace().skip(2).filter(|t| !t.contains('=')).map(str::to_string).collect())
        .unwrap_or_default();
    (ports, flatten(spice, top, instance).1)
}

/// Los transistores de una netlist del layout, con sus redes por el nombre
/// con que aparecen en su SPICE.
pub fn layout_devices(n: &riku_mod_layout::nets::LayoutNetlist) -> Vec<LayDevice> {
    let nl = &n.netlist;
    nl.devices
        .iter()
        .map(|(d, t)| {
            // El centro del recuadro de la compuerta: gira y se mueve con
            // ella (un punto cualquiera de adentro depende del orden de los
            // vértices).
            let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for q in &d.gate.points {
                (x0, y0, x1, y1) = (x0.min(q.x), y0.min(q.y), x1.max(q.x), y1.max(q.y));
            }
            let (at, gate) = if x0.is_finite() { (((x0 + x1) / 2.0, (y0 + y1) / 2.0), [x0, y0, x1, y1]) } else { (d.at, [d.at.0, d.at.1, d.at.0, d.at.1]) };
            (d, t, at, gate)
        })
        .zip(&n.owners)
        .map(|((d, t, at, gate), owner)| LayDevice {
            model: d.model.clone(),
            at: (round3(at.0 * n.unit_um), round3(at.1 * n.unit_um)),
            gate: gate.map(|v| v * n.unit_um),
            cell: (owner.cell != n.cell).then(|| owner.cell.clone()),
            local: (round3(owner.local.0 * n.unit_um), round3(owner.local.1 * n.unit_um)),
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
                    out.push((p.clone(), tr!("lvs_map.pin_elsewhere", nets = set.iter().cloned().collect::<Vec<_>>().join(", "))));
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

/// Los movimientos rígidos que vuelven a ubicar a más de la mitad de
/// `refs` (al menos dos), del que más ubica al que menos.
fn rigid_candidates(refs: &[&LayoutRef], lay: &[LayDevice], used: &HashSet<usize>) -> Vec<Moved> {
    let mut all: Vec<Moved> = Vec::new();
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
                let same = |m: &Moved| m.orient == o && (m.dx - dx).abs() <= TOL && (m.dy - dy).abs() <= TOL;
                if count >= 2 && count * 2 > refs.len() && !all.iter().any(same) {
                    all.push(Moved { orient: o, dx, dy, count });
                }
            }
        }
    }
    all.sort_by(|a, b| b.count.cmp(&a.count));
    all
}

/// Cuántas contradicciones hay entre las redes de unos vínculos: redes del
/// esquemático repartidas en varias del layout más redes del layout con
/// varias del esquemático.
fn conflicts(pairs: &[(&SchDevice, Vec<usize>)], lay: &[LayDevice]) -> usize {
    let map = net_pairs(pairs, lay);
    let mut back: HashMap<&String, HashSet<&String>> = HashMap::new();
    for (s, ls) in &map {
        for l in ls {
            back.entry(l).or_default().insert(s);
        }
    }
    map.values().filter(|v| v.len() > 1).count() + back.values().filter(|v| v.len() > 1).count()
}

/// Qué red del layout es cada una del esquemático, según los vínculos
/// ubicados (fuente y drenaje se pueden intercambiar).
fn net_pairs(pairs: &[(&SchDevice, Vec<usize>)], lay: &[LayDevice]) -> BTreeMap<String, BTreeSet<String>> {
    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let add = |map: &mut BTreeMap<String, BTreeSet<String>>, a: &str, b: &str| {
        map.entry(a.to_string()).or_default().insert(b.to_string());
    };
    // Compuerta y cuerpo no se intercambian: van primero.
    let mut pending: Vec<(&SchDevice, &LayDevice)> = Vec::new();
    for (s, fingers) in pairs {
        for &i in fingers {
            let l = &lay[i];
            add(&mut map, &s.pins[G], &l.pins[G]);
            add(&mut map, &s.pins[B], &l.pins[B]);
            pending.push((s, l));
        }
    }
    // Fuente y drenaje: primero los dedos que ya se deciden por lo
    // vinculado; si ninguno se decide, el primero va derecho y se sigue.
    let has = |map: &BTreeMap<String, BTreeSet<String>>, a: &str, b: &str| map.get(a).is_some_and(|v| v.contains(b));
    while !pending.is_empty() {
        let mut progress = false;
        let mut k = 0;
        while k < pending.len() {
            let (s, l) = pending[k];
            let straight = has(&map, &s.pins[D], &l.pins[D]) as u8 + has(&map, &s.pins[S], &l.pins[S]) as u8;
            let crossed = has(&map, &s.pins[D], &l.pins[S]) as u8 + has(&map, &s.pins[S], &l.pins[D]) as u8;
            if straight == crossed {
                k += 1;
                continue;
            }
            let (ld, ls) = if crossed > straight { (S, D) } else { (D, S) };
            add(&mut map, &s.pins[D], &l.pins[ld]);
            add(&mut map, &s.pins[S], &l.pins[ls]);
            pending.remove(k);
            progress = true;
        }
        if !progress {
            let (s, l) = pending.remove(0);
            add(&mut map, &s.pins[D], &l.pins[D]);
            add(&mut map, &s.pins[S], &l.pins[S]);
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

    // 1b. Un dedo de una sub-celda que no está donde estaba: el único de esa
    // celda en la misma posición local (se movió su instancia).
    for (bi, refs) in found.iter_mut() {
        let b = &map.binds[*bi];
        let mut moved = false;
        for (slot, r) in refs.iter_mut().zip(&b.layout) {
            let (None, Some(cell), Some(local)) = (*slot, r.cell.as_deref(), r.local) else { continue };
            let cands: Vec<usize> = (0..lay.len())
                .filter(|i| !used.contains(i) && same_model(&lay[*i].model, &r.model))
                .filter(|&i| lay[i].cell.as_deref() == Some(cell) && (lay[i].local.0 - local[0]).hypot(lay[i].local.1 - local[1]) <= TOL)
                .collect();
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
    let missing: Vec<(usize, usize)> =
        found.iter().enumerate().flat_map(|(k, (_, refs))| refs.iter().enumerate().filter(|(_, r)| r.is_none()).map(move |(j, _)| (k, j))).collect();
    if missing.len() >= 2 {
        let refs: Vec<&LayoutRef> = missing.iter().map(|&(k, j)| &map.binds[found[k].0].layout[j]).collect();
        // Cada candidato: qué ubicaría y cuántas contradicciones dejaría. Un
        // arreglo regular de dedos admite varios (correr todo "un dedo"
        // también encaja casi todo): gana el que menos contradice, y si dos
        // empatan no se reubica nada.
        let cands = rigid_candidates(&refs, lay, &used);
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
                    if let Some(i) = find(lay, &taken, &r.model, (p.0 + m.dx, p.1 + m.dy)) {
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
        let tied: Vec<usize> = scored.iter().enumerate().filter(|(_, x)| scored.first().is_some_and(|f| x.0 == f.0 && x.1.count == f.1.count)).map(|(i, _)| i).collect();
        // Entre empatados, un desplazamiento sin giro es lo más común: si es
        // uno solo, ese.
        let chosen = match tied.as_slice() {
            [] => None,
            [only] => Some(*only),
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

fn fmt(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

// ─── Un par en una versión ───────────────────────────────────────────────────

/// El archivo de vínculos de una celda, relativo a la raíz del proyecto.
pub fn map_path(cell: &str) -> String {
    format!("lvs/{cell}.toml")
}

/// Los dos lados de un par en una versión y su archivo de vínculos.
pub struct Session {
    /// La celda comparada del layout.
    pub cell: String,
    /// Dónde va el archivo (relativo a la raíz).
    pub map_path: String,
    /// El archivo (uno vacío si todavía no existe).
    pub map: MapFile,
    pub exists: bool,
    /// El archivo vino del disco porque la versión pedida no lo tiene.
    pub from_disk: bool,
    pub schematic: Vec<SchDevice>,
    pub layout: Vec<LayDevice>,
    pub warnings: Vec<String>,
    /// µm por unidad de la escena del layout (para dibujar sobre ella).
    pub unit_um: f64,
    /// Dónde está cada instancia del esquemático (para dibujar sobre él).
    pub places: xschem_viewer::spice::Places,
    /// Los pines de cada lado.
    pub sch_ports: Vec<String>,
    pub lay_ports: Vec<String>,
    /// Lo que no se revisa, de los dos lados.
    pub unchecked: Vec<String>,
}

/// [`Session`] de `pair` con los archivos de `tree`. Si esa versión no
/// tiene el archivo de vínculos y se da `disk` (la raíz del proyecto en el
/// disco), se usa el de ahí: los vínculos de hoy sirven para revisar un
/// commit anterior.
pub fn load(tree: &crate::lvs::Tree, pair: &crate::lvs::Pair, disk: Option<&std::path::Path>) -> Result<Session, String> {
    use std::sync::Arc;
    let files: Arc<dyn viewer_core::FileSource> = Arc::new(viewer_core::DiskFiles::new(tree.root.clone()));
    let s = crate::lvs::schematic_netlist(pair, files.clone())?;
    let places = s.netlist.places.clone();
    let schematic = schematic_devices(&s.netlist.text, &s.stem, &|n| places.instance(n).map(str::to_string));
    let (sch_ports, mut unchecked) = schematic_extras(&s.netlist.text, &s.stem, &|n| places.instance(n).map(str::to_string));
    let bytes = files.read(&pair.layout).ok_or_else(|| format!("{}: {}", pair.layout, tr!("lvs.cannot_read")))?;
    let ln = riku_mod_layout::nets::layout_netlist(&bytes, &pair.layout, Some(files.as_ref()), pair.cell.as_deref())?;
    let layout = layout_devices(&ln);
    let lay_ports = ln.netlist.ports();
    if !ln.netlist.resistors.is_empty() {
        unchecked.push(tr!("lvs_map.unchecked_lay_res", count = ln.netlist.resistors.len()));
    }
    let map_path = map_path(&ln.cell);
    let in_tree = std::fs::read_to_string(tree.root.join(&map_path)).ok();
    let (text, from_disk) = match in_tree {
        Some(t) => (Some(t), false),
        None => match disk.and_then(|d| std::fs::read_to_string(d.join(&map_path)).ok()) {
            Some(t) => (Some(t), true),
            None => (None, false),
        },
    };
    let (map, exists) = match text {
        Some(text) => (MapFile::parse(&text).map_err(|e| format!("{map_path}: {e}"))?, true),
        None => (MapFile::new(&pair.schematic, &pair.layout, pair.cell.as_deref()), false),
    };
    let mut warnings: Vec<String> = s.netlist.warnings.clone();
    warnings.extend(ln.netlist.warnings.iter().cloned());
    Ok(Session { cell: ln.cell, map_path, map, exists, from_disk, schematic, layout, warnings, unit_um: ln.unit_um, places, sch_ports, lay_ports, unchecked })
}

/// Todo lo que se deduce de una sesión: [`check`] más los pines y lo que no
/// se revisa.
pub fn check_session(s: &Session) -> Check {
    let mut c = check(&s.map, &s.schematic, &s.layout);
    c.pins = check_pins(&c, &s.sch_ports, &s.lay_ports);
    c.unchecked = s.unchecked.clone();
    c
}

/// Vincula el transistor `schematic` con los dedos `fingers` del layout. Lo
/// que ya hubiera de los dos (otro vínculo de ese transistor, o esos dedos
/// en otro vínculo) se reemplaza.
pub fn bind(map: &mut MapFile, schematic: &str, fingers: &[usize], lay: &[LayDevice]) {
    let refs: Vec<LayoutRef> = fingers.iter().map(|&i| LayoutRef::of(&lay[i])).collect();
    let taken = |r: &LayoutRef| refs.iter().any(|n| same_model(&n.model, &r.model) && (n.at[0] - r.at[0]).hypot(n.at[1] - r.at[1]) <= TOL);
    map.binds.retain(|b| b.schematic != schematic);
    for b in &mut map.binds {
        b.layout.retain(|r| !taken(r));
    }
    map.binds.retain(|b| !b.layout.is_empty());
    if !refs.is_empty() {
        map.binds.push(Bind { schematic: schematic.into(), layout: refs });
    }
}

/// Quita el vínculo de `schematic`.
pub fn unbind(map: &mut MapFile, schematic: &str) {
    map.binds.retain(|b| b.schematic != schematic);
}

/// El estado de un chequeo en pocos números (para el historial).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Summary {
    pub bound: usize,
    pub total: usize,
    pub fingers: usize,
    pub fingers_total: usize,
    /// Parámetros o modelos distintos.
    pub differences: usize,
    pub shorts: usize,
    pub opens: usize,
    pub lost: usize,
    pub pins: usize,
    /// Lo que no se revisa.
    pub unchecked: usize,
    pub clean: bool,
}

impl Summary {
    pub fn of(c: &Check, s: &Session) -> Self {
        Summary {
            bound: c.bound.len(),
            total: s.schematic.len(),
            fingers: c.bound.iter().map(|(_, f)| f.len()).sum(),
            fingers_total: s.layout.len(),
            differences: c.params.len() + c.models.len(),
            shorts: c.shorts.len(),
            opens: c.opens.len(),
            lost: c.lost.len(),
            pins: c.pins.len(),
            unchecked: c.unchecked.len(),
            clean: c.clean(),
        }
    }
}

/// Qué cambió de un commit (`older`) al siguiente (`newer`), lo que vale la
/// pena marcar en el historial.
pub fn transitions(older: &Summary, newer: &Summary) -> Vec<Transition> {
    let mut out = Vec::new();
    if newer.clean && !older.clean {
        out.push(Transition::Clean);
    } else if older.clean && !newer.clean {
        out.push(Transition::Broke);
    }
    if newer.shorts > older.shorts {
        out.push(Transition::NewShort);
    } else if newer.shorts < older.shorts {
        out.push(Transition::ShortFixed);
    }
    if newer.opens > older.opens {
        out.push(Transition::NewOpen);
    } else if newer.opens < older.opens {
        out.push(Transition::OpenFixed);
    }
    if newer.bound > older.bound {
        out.push(Transition::Linked(newer.bound - older.bound));
    }
    out
}

/// Un cambio de estado del LVS manual entre dos commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    Clean,
    /// Estaba limpio y dejó de estarlo.
    Broke,
    NewShort,
    ShortFixed,
    NewOpen,
    OpenFixed,
    Linked(usize),
}

/// Esquema del resultado en JSON.
pub const CHECK_SCHEMA: &str = "riku-lvs-check/v1";

/// El resultado en JSON: los transistores del layout como en el archivo
/// (modelo y posición).
pub fn check_json(c: &Check, s: &Session) -> serde_json::Value {
    let refs = |idx: &[usize]| -> Vec<serde_json::Value> {
        idx.iter().map(|&i| serde_json::json!({ "model": s.layout[i].model, "at": [s.layout[i].at.0, s.layout[i].at.1] })).collect()
    };
    let pairs = |v: &[(String, String)]| -> Vec<serde_json::Value> { v.iter().map(|(d, w)| serde_json::json!({ "device": d, "what": w })).collect() };
    let groups = |v: &[(String, Vec<String>)]| -> Vec<serde_json::Value> { v.iter().map(|(n, ns)| serde_json::json!({ "net": n, "nets": ns })).collect() };
    serde_json::json!({
        "cell": s.cell,
        "map": s.map_path,
        "map_from_disk": s.from_disk,
        "clean": c.clean(),
        "complete": c.complete(),
        "pins": pairs(&c.pins),
        "unchecked": c.unchecked,
        "moved_ambiguous": c.moved_ambiguous,
        "schematic_devices": s.schematic.len(),
        "layout_devices": s.layout.len(),
        "bound": c.bound.iter().map(|(n, f)| serde_json::json!({ "schematic": n, "layout": refs(f) })).collect::<Vec<_>>(),
        "moved": c.moved.map(|m| serde_json::json!({ "angle": (m.orient % 4) as u32 * 90, "mirrored": m.orient >= 4, "dx": m.dx, "dy": m.dy, "count": m.count })),
        "by_connectivity": c.by_connectivity,
        "by_cell": c.by_cell,
        "models": pairs(&c.models),
        "params": pairs(&c.params),
        "shorts": groups(&c.shorts),
        "opens": groups(&c.opens),
        "lost": c.lost.iter().map(|(n, r)| serde_json::json!({ "schematic": n, "model": r.model, "at": r.at })).collect::<Vec<_>>(),
        "unknown": c.unknown,
        "unbound_schematic": c.unbound_schematic,
        "unbound_layout": refs(&c.unbound_layout),
        "nets": c.nets,
        "warnings": s.warnings,
    })
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
                layout: fingers.iter().map(|&i| LayoutRef::of(&lay[i])).collect(),
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
        LayDevice { model: model.into(), at, gate: [at.0 - 0.25, at.1 - 0.5, at.0 + 0.25, at.1 + 0.5], cell: None, local: at, w, l: 0.5, pins: pins.map(str::to_string) }
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
        Bind { schematic: s.into(), layout: refs.iter().map(|&(m, x, y)| LayoutRef { model: m.into(), at: [x, y], cell: None, local: None }).collect() }
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
    fn fuente_y_drenaje_se_deciden_por_lo_vinculado() {
        // M2 primero: su primer dedo tiene la fuente y el drenaje al revés
        // que el esquemático, y todavía no hay nada vinculado. No debe
        // inventar un corto out–VDD.
        let (sch, lay) = sides();
        let mut m = full();
        m.binds.rotate_left(1);
        let c = check(&m, &sch, &lay);
        assert!(c.clean(), "{c:#?}");
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
    fn vincular_reemplaza_lo_anterior_de_los_dos_lados() {
        let (sch, lay) = sides();
        let mut m = full();
        // M3 pasa a ser el primer dedo de M2: M2 se queda con el otro.
        super::bind(&mut m, "M3", &[1], &lay);
        let m3 = m.binds.iter().find(|b| b.schematic == "M3").unwrap();
        assert_eq!(m3.layout.len(), 1);
        assert_eq!(m3.layout[0].at, [1.0, 5.0]);
        assert_eq!(m.binds.iter().find(|b| b.schematic == "M2").unwrap().layout.len(), 1);
        // Y lo que se deduce lo dice: modelo distinto en M3.
        assert!(!check(&m, &sch, &lay).models.is_empty());
        unbind(&mut m, "M3");
        assert!(m.binds.iter().all(|b| b.schematic != "M3"));
    }

    #[test]
    fn el_historial_marca_cortos_y_avance() {
        let base = Summary { bound: 5, total: 9, shorts: 0, opens: 0, clean: false, ..Summary::default() };
        let worse = Summary { shorts: 1, ..base.clone() };
        assert_eq!(transitions(&base, &worse), [Transition::NewShort]);
        assert_eq!(transitions(&worse, &base), [Transition::ShortFixed]);
        let done = Summary { bound: 9, clean: true, ..base.clone() };
        assert_eq!(transitions(&base, &done), [Transition::Clean, Transition::Linked(4)]);
        assert_eq!(transitions(&done, &Summary { differences: 2, clean: false, ..done.clone() }), [Transition::Broke]);
    }

    /// Seis transistores en fila (uno por columna, cada uno con sus redes).
    fn row(n: usize, y: f64) -> (Vec<SchDevice>, Vec<LayDevice>, MapFile) {
        let pins = |i: usize| [format!("d{i}"), format!("g{i}"), format!("s{i}"), "VSS".to_string()];
        let sch = (0..n).map(|i| SchDevice { name: format!("M{i}"), model: N.into(), pins: pins(i), w: Some(1.0), l: Some(0.5), m: 1.0 }).collect();
        let lay = (0..n).map(|i| LayDevice { model: N.into(), at: (i as f64, y), gate: [0.0; 4], cell: None, local: (i as f64, y), w: 1.0, l: 0.5, pins: pins(i) }).collect();
        let mut m = MapFile::new("a.sch", "a.gds", None);
        m.binds = (0..n).map(|i| Bind { schematic: format!("M{i}"), layout: vec![LayoutRef { model: N.into(), at: [i as f64, 0.0], cell: None, local: None }] }).collect();
        (sch, lay, m)
    }

    #[test]
    fn en_un_arreglo_regular_gana_el_movimiento_que_no_contradice() {
        // Todo subió 10: correrlo además "un dedo" también encaja 5 de 6,
        // pero vincularía cada transistor con las redes del vecino.
        let (sch, lay, m) = row(6, 10.0);
        let c = check(&m, &sch, &lay);
        let mv = c.moved.expect("movido");
        assert_eq!((mv.dx, mv.dy, mv.count), (0.0, 10.0, 6), "{c:#?}");
        assert!(c.shorts.is_empty() && c.opens.is_empty(), "{c:#?}");
    }

    #[test]
    fn un_movimiento_ambiguo_no_reubica_nada() {
        // Dos transistores iguales (mismas redes) a 2 µm, y en el layout
        // tres dedos a 2 µm: correr 10 o 12 encaja igual. No se elige.
        let sch: Vec<SchDevice> = ["M1", "M2"].iter().map(|n| sd(n, N, ["d", "g", "s", "VSS"], 1.0)).collect();
        let lay: Vec<LayDevice> = [10.0, 12.0, 14.0].iter().map(|&x| ld(N, (x, 0.0), ["d", "g", "s", "VSS"], 1.0)).collect();
        let mut m = MapFile::new("a.sch", "a.gds", None);
        m.binds = vec![bind("M1", &[(N, 0.0, 0.0)]), bind("M2", &[(N, 2.0, 0.0)])];
        let c = check(&m, &sch, &lay);
        assert!(c.moved.is_none() && c.moved_ambiguous, "{c:#?}");
        assert_eq!(c.lost.len(), 2);
    }

    #[test]
    fn los_pines_se_comparan_por_las_redes_vinculadas() {
        let (sch, lay) = sides();
        let mut c = check(&full(), &sch, &lay);
        // `in` del esquemático es `in` en el layout; `out` no tiene pin en
        // el layout; `x` es pin del layout pero llega a otra red; `EN` sobra.
        let sch_ports: Vec<String> = ["in", "out", "x"].map(str::to_string).to_vec();
        let lay_ports: Vec<String> = ["in", "x", "EN"].map(str::to_string).to_vec();
        c.nets.insert("x".into(), BTreeSet::from(["n9".to_string()]));
        c.pins = check_pins(&c, &sch_ports, &lay_ports);
        let names: Vec<&str> = c.pins.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(names, ["out", "x", "EN"]);
        assert!(!c.clean());
    }

    #[test]
    fn limpio_no_es_completo_si_hay_algo_sin_revisar() {
        let (sch, lay) = sides();
        let mut c = check(&full(), &sch, &lay);
        assert!(c.clean() && c.complete());
        c.unchecked = vec!["R1".into()];
        assert!(c.clean() && !c.complete());
    }

    #[test]
    fn lo_que_no_es_transistor_queda_sin_revisar() {
        let spice = ".subckt t in out VDD\nXM1 out in VSS VSS sky130_fd_pr__nfet_01v8 L=0.5 W=1\nR1 a b 1k\nx2 a b amp W=2\n* nota\n.ends\n";
        let (ports, others) = schematic_extras(spice, "t", &|n| n.strip_prefix('X').map(str::to_string));
        assert_eq!(ports, ["in", "out", "VDD"]);
        assert_eq!(others, ["R1", "x2 (amp)"]);
    }

    #[test]
    fn una_instancia_movida_se_reencuentra_por_su_celda() {
        // Dos inversores iguales (celda `inv`), cada uno con un transistor en
        // (1, 1) de la celda. La instancia de M2 se movió: su dedo no está
        // donde estaba, pero es el único de `inv` en (1, 1) que queda libre.
        let sch = vec![sd("M1", N, ["a", "x", "VSS", "VSS"], 1.0), sd("M2", N, ["b", "y", "VSS", "VSS"], 1.0)];
        let dev = |at: (f64, f64), pins: [&str; 4]| LayDevice { cell: Some("inv".into()), local: (1.0, 1.0), ..ld(N, at, pins, 1.0) };
        let lay = vec![dev((11.0, 1.0), ["a", "x", "VSS", "VSS"]), dev((51.0, 31.0), ["b", "y", "VSS", "VSS"])];
        let r = |x: f64, y: f64| LayoutRef { model: N.into(), at: [x, y], cell: Some("inv".into()), local: Some([1.0, 1.0]) };
        let mut m = MapFile::new("a.sch", "a.gds", None);
        m.binds = vec![Bind { schematic: "M1".into(), layout: vec![r(11.0, 1.0)] }, Bind { schematic: "M2".into(), layout: vec![r(21.0, 1.0)] }];
        let c = check(&m, &sch, &lay);
        assert!(c.clean(), "{c:#?}");
        assert_eq!(c.by_cell, ["M2"]);
        assert_eq!(c.updated.binds[1].layout[0].at, [51.0, 31.0]);
        // Y el archivo lo guarda con su celda.
        assert!(c.updated.to_text().contains("cell = \"inv\", local = [1.000, 1.000]"), "{}", c.updated.to_text());
    }

    #[test]
    fn el_esquematico_se_aplana_con_rutas_y_parametros() {
        let spice = ".subckt top a b\nXM1 a b VSS VSS sky130_fd_pr__nfet_01v8 L=0.5 W=1\nx1 b c inv W=2\nR9 a c 1k\n.ends\n\
.subckt inv in out W=1\nXM2 out in mid VSS sky130_fd_pr__nfet_01v8 L=0.5 W=W\nx2 mid out buf\n.ends\n\
.subckt buf p q\nXM3 q p VSS VSS sky130_fd_pr__nfet_01v8 L=0.5 W=1\n.ends\n.GLOBAL VSS\n";
        let (d, others) = flatten(spice, "top", &|n| n.strip_prefix('X').map(str::to_string));
        let names: Vec<&str> = d.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["M1", "x1/M2", "x1/x2/M3"]);
        // Los pines del sub-circuito son las redes de la instancia; lo
        // interno lleva la ruta; VSS es global.
        assert_eq!(d[1].pins, ["c", "b", "x1/mid", "VSS"]);
        assert_eq!(d[2].pins, ["c", "x1/mid", "VSS", "VSS"]);
        // W=W toma el valor de la instancia (2), no el de la definición (1).
        assert_eq!(d[1].w, Some(2.0));
        assert_eq!(others, ["R9"]);
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
