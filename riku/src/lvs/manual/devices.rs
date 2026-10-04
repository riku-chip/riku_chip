//! Los transistores de cada lado: los del esquemático (de su netlist SPICE, aplanada) y los del layout (de su extracción).

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};

/// Un transistor del esquemático: su instancia, modelo, redes (drenaje,
/// compuerta, fuente, cuerpo) y parámetros (µm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchDevice {
    pub name: String,
    pub model: String,
    pub pins: [String; 4],
    pub w: Option<f64>,
    pub l: Option<f64>,
    pub m: f64,
}

/// Un transistor (un dedo) del layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

pub(super) const D: usize = 0;
pub(super) const G: usize = 1;
pub(super) const S: usize = 2;
pub(super) const B: usize = 3;

/// Los transistores del `.subckt top` de una netlist del esquemático y los
/// de sus sub-circuitos (ver [`flatten`]). Los de arriba se nombran como sus
/// instancias (`XM1` → `M1`, con `instance`); los de adentro, con su ruta
/// (`x1/M3`).
pub fn schematic_devices(spice: &str, top: &str, instance: &dyn Fn(&str) -> Option<String>) -> Vec<SchDevice> {
    flatten(spice, top, instance).0
}

/// Un `.subckt`: sus pines, sus parámetros por defecto y sus líneas.
pub(super) struct Subckt {
    ports: Vec<String>,
    params: HashMap<String, String>,
    lines: Vec<String>,
}

/// Los `.subckt` de una netlist (con las `+` ya unidas) y sus redes globales.
pub(super) fn subckts(spice: &str) -> (HashMap<String, Subckt>, HashSet<String>) {
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
pub(super) fn flatten(spice: &str, top: &str, instance: &dyn Fn(&str) -> Option<String>) -> (Vec<SchDevice>, Vec<String>) {
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
pub(super) fn walk(
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
pub(super) fn is_transistor(toks: &[&str]) -> bool {
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

pub(super) fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

pub(super) fn is_mos(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.contains("fet") || m.contains("mos")
}

/// El mismo modelo, con o sin el prefijo de la librería (`sky130_fd_pr__`).
pub(super) fn same_model(a: &str, b: &str) -> bool {
    let short = |m: &str| m.rsplit("__").next().unwrap_or(m).to_ascii_lowercase();
    short(a) == short(b)
}

/// Un número SPICE en µm: con sufijo (`0.5u`, `500n`) se convierte; sin
/// sufijo es µm (SKY130 usa `.option scale=1e-6`). `None` si es un
/// parámetro (`W_N`).
pub(super) fn microns(s: &str) -> Option<f64> {
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
pub(super) fn auto_name(n: &str) -> bool {
    let num = |p: &str| n.strip_prefix(p).is_some_and(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()));
    num("net") || num("n") || n == "VSUBS"
}
