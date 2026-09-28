//! La netlist SPICE de una celda y sus transistores agrupados en fingers.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::Netlist;
use crate::devices::DeviceRules;

impl Netlist {
    /// El nombre de una red en SPICE: su etiqueta, `VSUBS` para el sustrato
    /// sin nombre (como Magic) o `n<i>`.
    pub fn net_name(&self, i: usize) -> String {
        let net = &self.nets[i];
        match &net.name {
            Some(n) => n.split_whitespace().collect::<Vec<_>>().join("_"),
            None if net.substrate => "VSUBS".into(),
            None => format!("n{i}"),
        }
    }

    /// Los pines de la celda: las redes con una etiqueta de pin, en orden alfabético.
    pub fn ports(&self) -> Vec<String> {
        let mut v: Vec<String> = (0..self.nets.len()).filter(|&i| self.nets[i].port).map(|i| self.net_name(i)).collect();
        v.sort();
        v.dedup();
        v
    }
}

/// Unos transistores iguales en paralelo: mismo modelo y L, misma
/// compuerta, cuerpo y par fuente/drenaje.
#[derive(Clone, Debug, PartialEq)]
pub struct Fingers {
    pub model: String,
    /// La suma de los W, µm.
    pub w_um: f64,
    pub l_um: f64,
    pub nf: usize,
    /// Fuente y drenaje (en orden de red), compuerta y cuerpo.
    pub s: usize,
    pub d: usize,
    pub g: usize,
    pub b: usize,
    /// Índices en `Netlist::devices`.
    pub devices: Vec<usize>,
}

/// Los transistores de la netlist agrupados en fingers.
pub fn fingers(nl: &Netlist) -> Vec<Fingers> {
    let mut groups: BTreeMap<(String, i64, usize, usize, usize, usize), Fingers> = BTreeMap::new();
    for (i, (dev, t)) in nl.devices.iter().enumerate() {
        let (s, d) = (t.s.min(t.d), t.s.max(t.d));
        let l_nm = (dev.l_um * 1000.0).round() as i64;
        let g = groups.entry((dev.model.clone(), l_nm, t.g, s, d, t.b)).or_insert_with(|| Fingers {
            model: dev.model.clone(),
            w_um: 0.0,
            l_um: dev.l_um,
            nf: 0,
            s,
            d,
            g: t.g,
            b: t.b,
            devices: Vec::new(),
        });
        g.w_um += dev.w_um;
        g.nf += 1;
        g.devices.push(i);
    }
    groups.into_values().collect()
}

/// La celda como `.subckt` SPICE, un transistor por finger. `unit`: el
/// sufijo de W y L en µm (`"u"`; `""` para las netlists de SKY130, que
/// usan `.option scale=1e-6`).
pub fn spice(cell: &str, nl: &Netlist, rules: &DeviceRules, unit: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, ".subckt {cell} {}", nl.ports().join(" "));
    for (i, (dev, t)) in nl.devices.iter().enumerate() {
        let x = if rules.device_type(&dev.magic).is_none_or(|k| k.subckt) { 'X' } else { 'M' };
        let n = |k: usize| nl.net_name(k);
        let _ = writeln!(
            out,
            "{x}{i} {} {} {} {} {} w={}{unit} l={}{unit}",
            n(t.d),
            n(t.g),
            n(t.s),
            n(t.b),
            dev.model,
            num(dev.w_um),
            num(dev.l_um)
        );
    }
    for (i, (r, [a, b])) in nl.resistors.iter().enumerate() {
        let x = if r.subckt { "XR" } else { "R" };
        let _ = writeln!(out, "{x}{i} {} {} {} w={}{unit} l={}{unit}", nl.net_name(*a), nl.net_name(*b), r.model, num(r.w_um), num(r.l_um));
    }
    out.push_str(".ends\n");
    out
}

/// Un número con hasta 4 decimales (0,1 nm), sin ceros de más.
fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::super::{Net, Terminals};
    use super::*;
    use crate::devices::Device;
    use gdstk_rs::OwnedPolygon;

    fn net(name: Option<&str>, port: bool, substrate: bool) -> Net {
        Net { name: name.map(String::from), labels: name.into_iter().map(String::from).collect(), port, substrate, bbox: [0.0; 4] }
    }

    fn dev(model: &str, w: f64) -> Device {
        Device {
            model: model.into(),
            magic: "nfet".into(),
            gate: OwnedPolygon { layer: 0, datatype: 0, points: Vec::new() },
            at: (0.0, 0.0),
            w_um: w,
            l_um: 0.15,
            sd_at: Vec::new(),
        }
    }

    fn sample() -> Netlist {
        Netlist {
            nets: vec![net(Some("Y"), true, false), net(Some("A"), true, false), net(None, false, true), net(None, false, false)],
            devices: vec![
                (dev("mini__nfet", 0.42), Terminals { d: 0, g: 1, s: 3, b: 2 }),
                // El mismo, con fuente y drenaje al revés: otro finger.
                (dev("mini__nfet", 0.42), Terminals { d: 3, g: 1, s: 0, b: 2 }),
                (dev("mini__nfet", 0.65), Terminals { d: 3, g: 1, s: 2, b: 2 }),
            ],
            resistors: Vec::new(),
            labels: Vec::new(),
            label_nets: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn writes_a_subckt_with_ports_and_one_line_per_finger() {
        let rules = DeviceRules::parse(crate::devices::rules::tests::TECH).unwrap();
        let s = spice("INV", &sample(), &rules, "u");
        assert_eq!(
            s,
            ".subckt INV A Y\nX0 Y A n3 VSUBS mini__nfet w=0.42u l=0.15u\nX1 n3 A Y VSUBS mini__nfet w=0.42u l=0.15u\nX2 n3 A VSUBS VSUBS mini__nfet w=0.65u l=0.15u\n.ends\n"
        );
    }

    #[test]
    fn parallel_fingers_are_grouped() {
        let g = fingers(&sample());
        let got: Vec<(usize, String)> = g.iter().map(|f| (f.nf, num(f.w_um))).collect();
        assert_eq!(got, [(2, "0.84".to_string()), (1, "0.65".to_string())]);
    }
}
