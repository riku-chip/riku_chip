//! Redes de un layout: qué metal, poly y difusión están unidos por contactos
//! y vías, con el nombre de sus etiquetas, y a qué red va cada terminal de
//! los transistores. Con las reglas del `.tech` de Magic del PDK, como los
//! transistores (ver `docs/electrico.md`, nivel 3).

mod extract;
mod netlist;

pub use extract::{build, Net, NetLabel, Netlist, Resistor, Terminals};
pub use netlist::{fingers, spice, Fingers};

use std::collections::HashMap;

use gdstk_rs::{Cell, Library, OwnedPolygon};
use rayon::prelude::*;

use crate::devices::{self, DeviceRules, LayerPolys, RegionEval};

/// Los tipos cuya región hace falta: los que conducen, los del sustrato y
/// los que lo excluyen.
fn wanted_types(rules: &DeviceRules) -> Vec<String> {
    let mut types = rules.conductors();
    let resistors = rules.resistors.iter().map(|r| &r.magic);
    for t in rules.substrate.0.iter().chain(&rules.substrate.1).chain(resistors) {
        if t != "space" && !types.contains(t) {
            types.push(t.clone());
        }
    }
    types
}

/// La región de cada tipo buscado sin lo que le pintan encima los tipos
/// posteriores de su plano (ver [`DeviceRules::layer_index`]).
fn paint_order(rules: &DeviceRules, types: &[String], evaluated: &[(String, Vec<OwnedPolygon>)]) -> Vec<(String, Vec<OwnedPolygon>)> {
    evaluated
        .par_iter()
        .filter(|(t, r)| types.contains(t) && !r.is_empty())
        .map(|(t, r)| {
            let (plane, idx) = (rules.plane(t), rules.layer_index(t));
            let over: Vec<OwnedPolygon> = evaluated
                .iter()
                .filter(|(u, ur)| u != t && !ur.is_empty() && plane.is_some() && rules.plane(u) == plane && rules.layer_index(u) > idx)
                .flat_map(|(_, ur)| ur.iter().cloned())
                .collect();
            if over.is_empty() {
                return (t.clone(), r.clone());
            }
            let tag = gdstk_rs::GdsTag { layer: 0, datatype: 0 };
            (t.clone(), gdstk_rs::boolean_owned(r, &over, gdstk_rs::BoolOp::Not, tag).unwrap_or_else(|_| r.clone()))
        })
        .collect()
}

/// Las redes de `cell` (aplanada) según `rules`. Las nombran las etiquetas
/// de la propia celda (las de sus sub-celdas no: en un GDS no hay nombres de
/// instancia para distinguirlas). Un layout de Magic ya trae cada tipo como
/// una capa con su nombre; un GDS/OASIS se evalúa con las reglas de
/// `cifinput`. `magic`: lo que el lector de Magic sabe además de la
/// geometría; sus `port` dicen qué etiquetas son pines (sin él, todas).
pub fn cell_nets(lib: &Library, cell: &Cell<'_>, rules: &DeviceRules, magic: Option<&gdstk_rs::magic::MagInfo>) -> Netlist {
    let names: HashMap<(u32, u32), String> = lib.layer_names().into_iter().map(|(t, n)| ((t.layer, t.datatype), n)).collect();
    let types = wanted_types(rules);
    let own_labels: Vec<((u32, u32), String, (f64, f64))> = cell
        .labels()
        .map(|l| {
            let o = l.origin();
            ((l.layer(), l.texttype()), l.text().into_owned(), (o.x, o.y))
        })
        .collect();

    let (regions, mut labels, devices) = if names.values().any(|n| rules.device_type(n).is_some()) {
        // Magic: cada capa es un tipo.
        let tags: Vec<(u32, u32)> = names.iter().filter(|(_, n)| types.iter().any(|t| t == rules.canonical(n))).map(|(&t, _)| t).collect();
        let mut by_type: HashMap<String, Vec<OwnedPolygon>> = HashMap::new();
        for p in devices::flatten(cell, &tags) {
            if let Some(n) = names.get(&(p.layer, p.datatype)) {
                by_type.entry(rules.canonical(n).to_string()).or_default().push(p);
            }
        }
        let ports: Option<Vec<String>> =
            magic.and_then(|m| m.cells.iter().find(|c| c.name == cell.name())).map(|c| c.ports.iter().map(|p| p.name.clone()).collect());
        let labels: Vec<NetLabel> = own_labels
            .into_iter()
            .filter_map(|(tag, text, at)| {
                let t = rules.canonical(names.get(&tag)?).to_string();
                let port = ports.as_ref().is_some_and(|p| p.contains(&text));
                Some(NetLabel { text, at, types: rules.with_contacts(&t), port })
            })
            .collect();
        let mut regions: Vec<(String, Vec<OwnedPolygon>)> = by_type.into_iter().collect();
        regions.sort_by(|a, b| a.0.cmp(&b.0));
        (regions, labels, devices::cell_devices(lib, cell, rules))
    } else {
        let mut wanted = rules.type_layers(&types);
        wanted.extend(rules.used_layers());
        let pin_layers = rules.port_layers();
        // 1 nm: una etiqueta justo en el borde de su pin.
        let near = 1e-3 / (lib.unit() / 1e-6);
        wanted.extend(pin_layers.iter().copied());
        let tags: Vec<(u32, u32)> = lib
            .layers()
            .into_iter()
            .map(|t| (t.layer, t.datatype))
            .filter(|&(l, d)| wanted.iter().any(|&(wl, wd)| wl == l && wd.is_none_or(|wd| wd == d)))
            .collect();
        let layers = LayerPolys::new(devices::flatten(cell, &tags));
        let devs = devices::extract(rules, &layers, lib.unit() / 1e-6);
        // Magic pinta los tipos en el orden de `cifinput`, y en un mismo plano
        // el posterior tapa al anterior: la regla de un contacto a la toma N
        // puede cubrir también los de la difusión P (IHP), que se pintan
        // después. Hacen falta también los tipos que tapan a los buscados.
        let planes: Vec<&str> = types.iter().filter_map(|t| rules.plane(t)).collect();
        let mut all: Vec<String> = types.clone();
        for t in rules.layer_types() {
            let c = rules.canonical(t).to_string();
            if rules.plane(&c).is_some_and(|p| planes.contains(&p)) && !all.contains(&c) {
                all.push(c);
            }
        }
        // Un tipo por hilo: cada uno con su evaluador (las capas intermedias
        // que comparten se evalúan más de una vez, pero en paralelo).
        let unit_um = lib.unit() / 1e-6;
        let evaluated: Vec<(String, Vec<OwnedPolygon>)> = all
            .par_iter()
            .map_init(|| RegionEval::new(rules, &layers, unit_um), |ev, t| (t.clone(), ev.type_region(t)))
            .collect();
        let regions = paint_order(rules, &types, &evaluated);
        let labels: Vec<NetLabel> = own_labels
            .into_iter()
            .filter_map(|(tag, text, at)| {
                let lt = rules.label_types(tag);
                // Un pin: la etiqueta en una capa de pines o sobre un polígono
                // de una (el texto de SKY130 va en 67/5 y el pin en 67/16).
                let port = lt.iter().any(|(_, p)| *p)
                    || [(0.0, 0.0), (-near, 0.0), (near, 0.0), (0.0, -near), (0.0, near)]
                        .iter()
                        .any(|(dx, dy)| pin_layers.iter().any(|&gl| layers.contains(gl, at.0 + dx, at.1 + dy)));
                let mut types: Vec<String> = Vec::new();
                for (t, _) in &lt {
                    for x in rules.with_contacts(t) {
                        if !types.contains(&x) {
                            types.push(x);
                        }
                    }
                }
                (!lt.is_empty()).then_some(NetLabel { text, at, types, port })
            })
            .collect();
        (regions, labels, devs)
    };
    // Una celda sin etiquetas de pin (solo texto, o Magic): todas son pines.
    if !labels.iter().any(|l| l.port) {
        for l in &mut labels {
            l.port = true;
        }
    }
    build(rules, regions, &labels, devices, lib.unit() / 1e-6)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Pdk;

    /// Sin el PDK instalado (la CI): la SRAM de SKY130 del repo, con la tabla
    /// compilada.
    #[test]
    fn the_sram_example_has_nets_for_every_terminal() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/GDS/sram_16x8_sky130.gds");
        let lib = Library::from_bytes(&std::fs::read(&path).expect("sram")).expect("gds");
        let rules = devices::compiled(Pdk::Sky130).expect("sky130");
        let top = crate::select_top_cell(&lib).expect("top");
        let t = std::time::Instant::now();
        let nl = cell_nets(&lib, &top, rules, None);
        eprintln!("{} redes, {} transistores en {:?}; avisos: {:?}", nl.nets.len(), nl.devices.len(), t.elapsed(), nl.warnings);
        assert!(nl.devices.len() > 500);
        // Cada transistor tiene compuerta distinta de fuente y drenaje (salvo diodos).
        let odd = nl.devices.iter().filter(|(_, t)| t.g == t.s && t.g == t.d).count();
        assert!(odd * 20 < nl.devices.len(), "{odd} con los tres terminales en la misma red");
        // Todo está en un pozo N profundo: el pozo P aislado es una sola red
        // (unida por `grow`), la de tierra; los pfet, en vccd1.
        let bodies: std::collections::BTreeSet<String> = nl.devices.iter().map(|(_, t)| nl.net_name(t.b)).collect();
        assert_eq!(bodies.into_iter().collect::<Vec<_>>(), ["vccd1", "vssd1"]);
    }
}
