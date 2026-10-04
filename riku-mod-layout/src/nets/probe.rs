//! La red bajo un punto del visor: los pedazos conductores de cada red en
//! una grilla por tipo, con las capas de la escena donde se dibuja cada tipo
//! (para elegir la red del metal que está bajo el cursor y no la del pozo de
//! abajo).
//!
//! También ubica lo que nombra el LVS: las redes y los dispositivos por su
//! nombre en la netlist que escribe [`super::spice`] (la que compara Netgen).

use std::collections::HashMap;

use gdstk_rs::OwnedPolygon;
use viewer_core::element::Layer;
use viewer_core::{NetHit, NetProbe};

use super::{net_label, parallel_groups, Netlist};
use crate::devices::extract::Grid;

/// Los pedazos de un tipo, la red de cada uno y las capas donde se ve.
struct TypePieces {
    layers: Vec<Layer>,
    grid: Grid,
    nets: Vec<usize>,
}

/// La sonda de redes de una celda (ver [`NetProbe`]).
pub struct LayoutNets {
    types: Vec<TypePieces>,
    names: Vec<String>,
    /// Por red, sus pedazos: `(tipo, polígono)`.
    by_net: Vec<Vec<(usize, usize)>>,
    /// Nombre en la netlist SPICE (`Netlist::net_name`) → redes. Más de una si
    /// dos redes separadas tienen la misma etiqueta: en la netlist son una.
    spice_nets: HashMap<String, Vec<usize>>,
    /// Por transistor (índice en `Netlist::devices`, el número de `X<i>`): su compuerta.
    gates: Vec<Vec<(f64, f64)>>,
    /// Por transistor: los que están en paralelo con él, él incluido
    /// ([`parallel_groups`]: también con otro L). Netgen
    /// los junta y nombra al grupo por uno solo.
    group_of: Vec<Vec<usize>>,
    /// Por resistor (índice en `Netlist::resistors`): su cuerpo.
    bodies: Vec<Vec<(f64, f64)>>,
}

/// Un dispositivo nombrado por el LVS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeviceRef {
    Transistor(usize),
    Resistor(usize),
}

/// `19`, `X19` o `M19` → transistor 19; `R3` o `XR3` → resistor 3 (sin
/// distinguir mayúsculas). Netgen saca la `X` de las instancias de
/// sub-circuito, que es como los PDK abiertos escriben los transistores.
fn parse_device(name: &str) -> Option<DeviceRef> {
    let lower = name.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix('x').unwrap_or(&lower);
    let (resistor, digits) = match rest.strip_prefix('r') {
        Some(d) => (true, d),
        None => (false, rest.strip_prefix('m').unwrap_or(rest)),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let i = digits.parse().ok()?;
    Some(if resistor { DeviceRef::Resistor(i) } else { DeviceRef::Transistor(i) })
}

fn points(p: &OwnedPolygon) -> Vec<(f64, f64)> {
    p.points.iter().map(|q| (q.x, q.y)).collect()
}

impl std::fmt::Debug for LayoutNets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutNets").field("nets", &self.names.len()).field("types", &self.types.len()).finish()
    }
}

impl LayoutNets {
    /// `layers_of(tipo)`: las capas de la escena donde se dibuja ese tipo de
    /// Magic. `unit_um`: µm por unidad, para nombrar las redes sin etiqueta.
    pub fn new(nl: &Netlist, layers_of: &dyn Fn(&str) -> Vec<Layer>, unit_um: f64) -> Self {
        let mut grouped: HashMap<&str, (Vec<gdstk_rs::OwnedPolygon>, Vec<usize>)> = HashMap::new();
        for p in &nl.pieces {
            let e = grouped.entry(p.magic.as_str()).or_default();
            e.0.push(p.poly.clone());
            e.1.push(p.net);
        }
        let mut keys: Vec<&str> = grouped.keys().copied().collect();
        keys.sort_unstable();
        let mut by_net = vec![Vec::new(); nl.nets.len()];
        let mut types = Vec::new();
        for (ti, t) in keys.into_iter().enumerate() {
            let (polys, nets) = grouped.remove(t).unwrap_or_default();
            for (pi, &n) in nets.iter().enumerate() {
                by_net[n].push((ti, pi));
            }
            types.push(TypePieces { layers: layers_of(t), grid: Grid::new(polys), nets });
        }
        let names = (0..nl.nets.len()).map(|i| net_label(nl, i, unit_um)).collect();

        let mut spice_nets: HashMap<String, Vec<usize>> = HashMap::new();
        for i in 0..nl.nets.len() {
            spice_nets.entry(nl.net_name(i)).or_default().push(i);
        }
        let gates = nl.devices.iter().map(|(d, _)| points(&d.gate)).collect();
        let mut group_of: Vec<Vec<usize>> = (0..nl.devices.len()).map(|i| vec![i]).collect();
        for g in parallel_groups(nl) {
            for &d in &g {
                group_of[d] = g.clone();
            }
        }
        let bodies = nl.resistors.iter().map(|(r, _)| points(&r.body)).collect();
        Self { types, names, by_net, spice_nets, gates, group_of, bodies }
    }

    fn hit(&self, net: usize) -> NetHit {
        NetHit { name: self.names[net].clone(), outline: self.outline(net) }
    }

    fn outline(&self, net: usize) -> Vec<Vec<(f64, f64)>> {
        self.by_net[net].iter().map(|&(t, p)| points(&self.types[t].grid.polys[p])).collect()
    }

    /// Las redes con ese nombre SPICE: el exacto o, si no, el único que
    /// coincide sin distinguir mayúsculas.
    fn spice_net(&self, name: &str) -> Option<&[usize]> {
        if let Some(v) = self.spice_nets.get(name) {
            return Some(v);
        }
        let mut same = self.spice_nets.iter().filter(|(k, _)| k.eq_ignore_ascii_case(name));
        match (same.next(), same.next()) {
            (Some((_, v)), None) => Some(v),
            _ => None,
        }
    }
}

impl NetProbe for LayoutNets {
    fn at(&self, x: f64, y: f64, layer: Option<Layer>) -> Option<NetHit> {
        let net = self
            .types
            .iter()
            .filter(|t| layer.is_none_or(|l| t.layers.contains(&l)))
            .find_map(|t| t.grid.find(x, y).map(|i| t.nets[i as usize]))?;
        Some(self.hit(net))
    }

    fn net_named(&self, spice_name: &str) -> Option<NetHit> {
        let outline: Vec<_> = self.spice_net(spice_name)?.iter().flat_map(|&n| self.outline(n)).collect();
        // Una red sin pedazos (solo los terminales de un dispositivo) no se puede mostrar.
        (!outline.is_empty()).then(|| NetHit { name: spice_name.to_string(), outline })
    }

    fn device_named(&self, spice_name: &str) -> Option<NetHit> {
        let outline = match parse_device(spice_name)? {
            DeviceRef::Transistor(i) => self.group_of.get(i)?.iter().map(|&d| self.gates[d].clone()).collect(),
            DeviceRef::Resistor(i) => vec![self.bodies.get(i)?.clone()],
        };
        Some(NetHit { name: spice_name.to_string(), outline })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Net, NetPiece, Netlist};
    use super::*;
    use gdstk_rs::{OwnedPolygon, Point2D};

    fn square(x: f64) -> OwnedPolygon {
        let p = |x, y| Point2D { x, y };
        OwnedPolygon { layer: 0, datatype: 0, points: vec![p(x, 0.0), p(x + 1.0, 0.0), p(x + 1.0, 1.0), p(x, 1.0)] }
    }

    #[test]
    fn finds_the_net_of_the_layer_under_the_cursor() {
        let net = |n: &str| Net { name: Some(n.into()), labels: vec![n.into()], port: true, substrate: false, bbox: [0.0; 4] };
        let nl = Netlist {
            pieces: vec![
                NetPiece { magic: "metal1".into(), poly: square(0.0), net: 0 },
                NetPiece { magic: "metal1".into(), poly: square(5.0), net: 0 },
                // Un pozo debajo del mismo punto, de otra red.
                NetPiece { magic: "nwell".into(), poly: square(0.0), net: 1 },
            ],
            nets: vec![net("Y"), net("VPB")],
            ..Default::default()
        };
        let probe = LayoutNets::new(&nl, &|t| if t == "metal1" { vec![3] } else { vec![1] }, 1.0);
        let hit = probe.at(0.5, 0.5, Some(3)).expect("metal1");
        assert_eq!((hit.name.as_str(), hit.outline.len()), ("Y", 2), "la red entera: sus dos pedazos");
        assert_eq!(probe.at(0.5, 0.5, Some(1)).map(|h| h.name), Some("VPB".into()), "sobre el pozo, su red");
        assert_eq!(probe.at(0.5, 0.5, Some(9)), None, "una capa que no conduce");
        assert_eq!(probe.at(3.0, 0.5, None), None);
    }

    use super::super::{Resistor, Terminals};
    use crate::devices::{Device, DeviceRules};

    fn net(name: Option<&str>, substrate: bool) -> Net {
        Net {
            name: name.map(String::from),
            labels: name.into_iter().map(String::from).collect(),
            port: true,
            substrate,
            bbox: [0.0; 4],
        }
    }

    fn dev(w: f64, gate_x: f64) -> Device {
        Device {
            model: "mini__nfet".into(),
            magic: "nfet".into(),
            gate: square(gate_x),
            at: (gate_x + 0.5, 0.5),
            w_um: w,
            l_um: 0.15,
            sd_at: Vec::new(),
        }
    }

    /// Dos transistores en paralelo (un grupo de fingers) y un tercero en
    /// paralelo con otro L (como los rellenos del demo `ota`), uno suelto, un
    /// resistor; redes con nombre, sin nombre, el sustrato, dos redes
    /// separadas con la misma etiqueta y nombres que solo difieren en mayúsculas.
    fn sample() -> Netlist {
        let other_l = Device { l_um: 1.0, ..dev(1.0, 106.0) };
        let piece = |net: usize, x: f64| NetPiece { magic: "metal1".into(), poly: square(x), net };
        Netlist {
            pieces: vec![piece(0, 0.0), piece(3, 10.0), piece(4, 20.0), piece(5, 30.0), piece(6, 40.0), piece(7, 50.0)],
            nets: vec![
                net(Some("Y"), false),
                net(Some("A"), false),
                net(None, true),
                net(None, false),
                net(Some("Y"), false),
                net(Some("out put"), false),
                net(Some("Vdd"), false),
                net(Some("VDD"), false),
            ],
            devices: vec![
                (dev(0.42, 100.0), Terminals { d: 0, g: 1, s: 3, b: 2 }),
                (dev(0.42, 102.0), Terminals { d: 3, g: 1, s: 0, b: 2 }),
                (dev(0.65, 104.0), Terminals { d: 3, g: 1, s: 2, b: 2 }),
                (other_l, Terminals { d: 0, g: 1, s: 3, b: 2 }),
            ],
            resistors: vec![(
                Resistor {
                    model: "mini__res".into(),
                    magic: "rm1".into(),
                    body: square(200.0),
                    at: (200.5, 0.5),
                    w_um: 1.0,
                    l_um: 5.0,
                    subckt: true,
                },
                [0, 3],
            )],
            ..Default::default()
        }
    }

    fn probe() -> LayoutNets {
        LayoutNets::new(&sample(), &|_| vec![1], 1.0)
    }

    #[test]
    fn device_names_as_netgen_or_spice_write_them() {
        use DeviceRef::*;
        for (name, want) in [
            ("19", Some(Transistor(19))),
            ("X19", Some(Transistor(19))),
            ("M19", Some(Transistor(19))),
            ("m19", Some(Transistor(19))),
            (" 7 ", Some(Transistor(7))),
            ("R3", Some(Resistor(3))),
            ("XR3", Some(Resistor(3))),
            ("xr3", Some(Resistor(3))),
            ("X", None),
            ("19a", None),
            ("Q1", None),
            ("", None),
            ("M", None),
        ] {
            assert_eq!(parse_device(name), want, "{name:?}");
        }
    }

    #[test]
    fn nets_by_their_spice_name() {
        let p = probe();
        let count = |n: &str| p.net_named(n).map(|h| h.outline.len());
        assert_eq!(count("n3"), Some(1), "sin etiqueta: n<i>");
        assert_eq!(count("out_put"), Some(1), "espacios cambiados por _ como en spice()");
        assert_eq!(count("Y"), Some(2), "dos redes con la misma etiqueta son una en la netlist");
        assert_eq!(count("y"), Some(2), "sin distinguir mayúsculas si hay una sola");
        assert_eq!(count("VDD"), Some(1));
        assert_eq!(count("vdd"), None, "Vdd y VDD: ambiguo");
        assert_eq!(count("A"), None, "una red sin pedazos no se puede mostrar");
        assert_eq!(count("nope"), None);
        assert_eq!(p.net_named("n3").map(|h| h.name), Some("n3".into()), "el nombre pedido");
        assert!(p.at(10.5, 0.5, None).is_some_and(|h| h.name != "n3"), "el tooltip sigue con net_label");
    }

    #[test]
    fn a_device_brings_everything_netgen_joined_in_parallel() {
        let p = probe();
        let gates = |n: &str| p.device_named(n).map(|h| h.outline);
        let x = |o: Vec<Vec<(f64, f64)>>| -> Vec<f64> { o.iter().map(|g| g[0].0).collect() };
        let joined = Some(vec![100.0, 102.0, 106.0]);
        assert_eq!(gates("0").map(x), joined, "los dos fingers y el de otro L, en paralelo");
        assert_eq!(gates("X1").map(x), joined, "nombrado por otro del grupo, el mismo grupo");
        assert_eq!(gates("3").map(x), joined);
        assert_eq!(gates("M2").map(x), Some(vec![104.0]), "suelto");
        assert_eq!(gates("R0").map(x), Some(vec![200.0]), "el cuerpo del resistor");
        assert_eq!(gates("4"), None, "fuera de rango");
        assert_eq!(gates("XR1"), None);
        assert_eq!(gates("Q1"), None);
    }

    /// Lo que escribe `spice()` (y compara Netgen) está en la sonda: si
    /// alguien cambia cómo se nombra en la netlist, esto falla.
    #[test]
    fn every_name_in_the_spice_netlist_is_in_the_probe() {
        let rules = DeviceRules::parse(crate::devices::rules::tests::TECH).unwrap();
        let nl = sample();
        let p = LayoutNets::new(&nl, &|_| vec![1], 1.0);
        let text = super::super::spice("C", &nl, &rules, "u");
        let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('.')).collect();
        assert_eq!(lines.len(), nl.devices.len() + nl.resistors.len(), "{text}");
        for line in lines {
            let mut words = line.split_whitespace();
            let name = words.next().unwrap();
            assert!(p.device_named(name).is_some(), "{name} en\n{text}");
            let terminals = if name.to_ascii_uppercase().contains('R') { 2 } else { 4 };
            for n in words.take(terminals) {
                assert!(p.spice_nets.contains_key(n), "red {n} de {name} en\n{text}");
            }
        }
    }
}
