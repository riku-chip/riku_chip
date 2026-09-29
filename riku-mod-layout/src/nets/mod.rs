//! Redes de un layout: qué metal, poly y difusión están unidos por contactos
//! y vías, con el nombre de sus etiquetas, y a qué red va cada terminal de
//! los transistores. Con las reglas del `.tech` de Magic del PDK, como los
//! transistores (ver `docs/formatos.md`, «Transistores y redes»).

pub(crate) mod context;
mod diff;
mod extract;
mod netlist;
mod probe;

pub use extract::{build, Net, NetLabel, NetPiece, Netlist, Resistor, Terminals};
pub use probe::LayoutNets;
pub use diff::{cell_net_changes, net_changes, net_label, pieces_changed, NetChange, NetChangeKind};
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

/// Las ventanas en una grilla uniforme, para saber rápido si una caja toca
/// alguna.
struct WindowGrid<'a> {
    windows: &'a [[f64; 4]],
    origin: (f64, f64),
    cell: f64,
    cells: HashMap<(i64, i64), Vec<u32>>,
}

impl<'a> WindowGrid<'a> {
    fn new(windows: &'a [[f64; 4]]) -> Self {
        let ext = windows.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |a, b| {
            [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
        });
        let side = (windows.len() as f64).sqrt().ceil().clamp(1.0, 256.0);
        let cell = ((ext[2] - ext[0]).max(ext[3] - ext[1]) / side).max(1e-9);
        let mut cells: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
        let origin = (ext[0], ext[1]);
        for (i, w) in windows.iter().enumerate() {
            for key in Self::keys(origin, cell, w) {
                cells.entry(key).or_default().push(i as u32);
            }
        }
        Self { windows, origin, cell, cells }
    }

    fn keys(origin: (f64, f64), cell: f64, b: &[f64; 4]) -> impl Iterator<Item = (i64, i64)> {
        let k = |v: f64, o: f64| ((v - o) / cell).floor() as i64;
        let (x0, x1, y0, y1) = (k(b[0], origin.0), k(b[2], origin.0), k(b[1], origin.1), k(b[3], origin.1));
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }

    fn touches(&self, b: &[f64; 4]) -> bool {
        let overlap = |w: &[f64; 4]| b[0] <= w[2] && w[0] <= b[2] && b[1] <= w[3] && w[1] <= b[3];
        // Una caja enorme (un pozo) toca casi seguro: se prueba directo.
        let big = (b[2] - b[0]).max(b[3] - b[1]) > self.cell * 16.0;
        if big {
            return self.windows.iter().any(overlap);
        }
        Self::keys(self.origin, self.cell, b).any(|k| self.cells.get(&k).is_some_and(|ids| ids.iter().any(|&i| overlap(&self.windows[i as usize]))))
    }
}

/// Los polígonos recortados a las ventanas, capa por capa.
fn clip_to(polys: Vec<OwnedPolygon>, windows: &[[f64; 4]]) -> Vec<OwnedPolygon> {
    let rect = |b: &[f64; 4]| {
        let p = |x, y| gdstk_rs::Point2D { x, y };
        OwnedPolygon { layer: 0, datatype: 0, points: vec![p(b[0], b[1]), p(b[2], b[1]), p(b[2], b[3]), p(b[0], b[3])] }
    };
    let win: Vec<OwnedPolygon> = windows.iter().map(rect).collect();
    // Antes de recortar, se descarta lo que no toca ninguna ventana (una
    // grilla de las ventanas: la mayoría de los polígonos está lejos).
    let near = WindowGrid::new(windows);
    let mut by_tag: HashMap<(u32, u32), Vec<OwnedPolygon>> = HashMap::new();
    for p in polys.into_iter().filter(|p| near.touches(&devices::extract::bbox(p))) {
        by_tag.entry((p.layer, p.datatype)).or_default().push(p);
    }
    by_tag
        .into_par_iter()
        .flat_map(|((layer, datatype), group)| {
            let tag = gdstk_rs::GdsTag { layer, datatype };
            gdstk_rs::boolean_owned(&group, &win, gdstk_rs::BoolOp::And, tag).unwrap_or_default()
        })
        .collect()
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
    cell_nets_in(lib, cell, rules, magic, None)
}

/// Como [`cell_nets`], solo dentro de unas ventanas (cajas en unidades de la
/// librería) y con unos tipos: la geometría se recorta a ellas, sin
/// transistores y con todas las redes (para [`pieces_changed`]). Sirve para
/// mirar solo alrededor de un cambio y en las capas que cambiaron.
pub fn cell_nets_in(
    lib: &Library,
    cell: &Cell<'_>,
    rules: &DeviceRules,
    magic: Option<&gdstk_rs::magic::MagInfo>,
    window: Option<(&[[f64; 4]], &[String])>,
) -> Netlist {
    let windows = window.map(|(w, _)| w);
    let clip = |polys: Vec<OwnedPolygon>| match windows {
        Some(w) => clip_to(polys, w),
        None => polys,
    };
    let inside = |(x, y): (f64, f64)| windows.is_none_or(|w| w.iter().any(|b| x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3]));
    let names: HashMap<(u32, u32), String> = lib.layer_names().into_iter().map(|(t, n)| ((t.layer, t.datatype), n)).collect();
    let types = match window {
        Some((_, only)) => only.to_vec(),
        None => wanted_types(rules),
    };
    let own_labels: Vec<((u32, u32), String, (f64, f64))> = cell
        .labels()
        .map(|l| {
            let o = l.origin();
            ((l.layer(), l.texttype()), l.text().into_owned(), (o.x, o.y))
        })
        .filter(|(_, _, at)| inside(*at))
        .collect();

    let (regions, mut labels, devices) = if names.values().any(|n| rules.device_type(n).is_some()) {
        // Magic: cada capa es un tipo.
        let tags: Vec<(u32, u32)> = names.iter().filter(|(_, n)| types.iter().any(|t| t == rules.canonical(n))).map(|(&t, _)| t).collect();
        let mut by_type: HashMap<String, Vec<OwnedPolygon>> = HashMap::new();
        let polys = clip(devices::flatten(cell, &tags));
        for p in &polys {
            let p = p.clone();
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
        let devs = match windows {
            None => devices::cell_devices(lib, cell, rules),
            Some(_) => Vec::new(),
        };
        (regions, labels, devs)
    } else {
        let mut wanted = rules.type_layers(&types);
        if windows.is_none() {
            wanted.extend(rules.used_layers());
        }
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
        let layers = LayerPolys::new(clip(devices::flatten(cell, &tags)));
        let devs = if windows.is_some() { Vec::new() } else { devices::extract(rules, &layers, lib.unit() / 1e-6) };
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
    extract::build_with(rules, regions, &labels, devices, lib.unit() / 1e-6, windows.is_some())
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

/// La netlist SPICE de una celda de un layout, lista para compararla con la
/// de un esquemático (LVS).
#[derive(Clone, Debug)]
pub struct LayoutSpice {
    /// La celda comparada (la pedida, o la top).
    pub cell: String,
    /// `.subckt <celda> <pines>` con un transistor por finger (ver [`spice`]).
    pub spice: String,
    /// Avisos de la extracción (redes con dos nombres, etiquetas sueltas…).
    pub warnings: Vec<String>,
}

/// [`LayoutSpice`] de `cell` (por defecto, la top) del layout `bytes`. `path`
/// es su ruta: un `.mag` busca ahí sus sub-celdas, en `files` (la misma
/// versión) y en el PDK. `unit`: el sufijo de W y L (ver [`spice`]).
pub fn layout_spice(
    bytes: &[u8],
    path: &str,
    files: Option<&dyn viewer_core::FileSource>,
    cell: Option<&str>,
    unit: &str,
) -> Result<LayoutSpice, String> {
    use crate::source::ReadError;
    let message = |e: ReadError| match e {
        ReadError::NotLayout => format!("{path}: no es un layout GDSII, OASIS ni Magic"),
        ReadError::Parse(m) => format!("{path}: {m}"),
    };
    let side = crate::source::collect(bytes, Some(path), files).map_err(message)?.read(None).map_err(message)?;
    let lib = &side.lib;
    let top = match cell {
        Some(name) => lib.find_cell(name).ok_or_else(|| format!("{path}: no tiene la celda {name}"))?,
        None => crate::select_top_cell(lib).ok_or_else(|| format!("{path}: no tiene una celda top"))?,
    };
    let rules = devices::rules_for_library(lib, Some(path))
        .ok_or_else(|| format!("{path}: no se reconoce el PDK (sin reglas de transistores)"))?;
    let nl = cell_nets(lib, &top, rules, side.info.as_ref());
    Ok(LayoutSpice { cell: top.name().to_string(), spice: spice(top.name(), &nl, rules, unit), warnings: nl.warnings.clone() })
}
