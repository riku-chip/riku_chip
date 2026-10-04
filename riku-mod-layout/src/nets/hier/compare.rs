//! Abiertos, cortos y transistores que cambiaron en una celda, comparando
//! sus resúmenes de las dos versiones (ver `docs/ronda-5/design.md`, D22).
//!
//! Se compara al nivel de la celda: sus redes (las propias y las de sus
//! hijas) y sus transistores propios. Las anclas son las de siempre (las
//! etiquetas y los transistores propios, emparejados por posición) más las
//! de las instancias: una hija con la misma huella y la misma transformación
//! en los dos lados (gemela) tiene las mismas redes; una hija que cambió,
//! en el mismo lugar, sus puertos por nombre.

use std::collections::HashMap;

use super::build::{CellNets, Cells};
use crate::devices::{device_changes, DeviceChange};
use crate::nets::diff::{label_pairs, net_changes_with};
use crate::nets::{NetChange, Netlist, Terminals};

/// La celda vista como una `Netlist`: sus redes, y sus transistores y
/// etiquetas propios con las redes de la celda.
fn view(c: &CellNets) -> Netlist {
    let m = |i: usize| c.own_map[i] as usize;
    Netlist {
        pieces: Vec::new(),
        nets: c.nets.clone(),
        devices: c
            .own
            .devices
            .iter()
            .map(|(d, t)| (d.clone(), Terminals { d: m(t.d), g: m(t.g), s: m(t.s), b: m(t.b) }))
            .collect(),
        resistors: c.own.resistors.iter().map(|(r, e)| (r.clone(), [m(e[0]), m(e[1])])).collect(),
        labels: c.own.labels.clone(),
        label_nets: c.label_nets.clone(),
        warnings: Vec::new(),
    }
}

/// Las instancias de `a` y `b` emparejadas: gemelas (misma huella y
/// transformación) y, de las que quedan, las del mismo nombre y transformación.
fn pair_insts(a: &CellNets, b: &CellNets, grid: f64) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let mut by_key: HashMap<_, Vec<usize>> = HashMap::new();
    for (j, i) in b.insts.iter().enumerate().rev() {
        by_key.entry((i.key, i.xf.key(grid))).or_default().push(j);
    }
    let mut used = vec![false; b.insts.len()];
    let mut twins = Vec::new();
    let mut rest = Vec::new();
    for (i, inst) in a.insts.iter().enumerate() {
        match by_key.get_mut(&(inst.key, inst.xf.key(grid))).and_then(Vec::pop) {
            Some(j) => {
                used[j] = true;
                twins.push((i, j));
            }
            None => rest.push(i),
        }
    }
    let mut by_name: HashMap<_, Vec<usize>> = HashMap::new();
    for (j, i) in b.insts.iter().enumerate().rev().filter(|(j, _)| !used[*j]) {
        by_name.entry((i.cell.as_str(), i.xf.key(grid))).or_default().push(j);
    }
    let moved = rest
        .into_iter()
        .filter_map(|i| by_name.get_mut(&(a.insts[i].cell.as_str(), a.insts[i].xf.key(grid)))?.pop().map(|j| (i, j)))
        .collect();
    (twins, moved)
}

/// Cómo nombrar una red de la celda que solo viene de una hija:
/// `<hija>@(x, y)/<su red>`.
fn from_child(c: &CellNets, cells: &Cells, net: usize, unit_um: f64) -> Option<String> {
    for (k, map) in c.inst_maps.iter().enumerate() {
        let Some(n) = map.iter().position(|&m| m as usize == net) else { continue };
        let inst = &c.insts[k];
        let child = cells.get(&inst.key)?;
        let inner = child
            .nets
            .get(n)
            .and_then(|x| x.name.clone())
            .or_else(|| from_child(child, cells, n, unit_um))
            .unwrap_or(format!("n{n}"));
        return Some(format!("{}@({:.2}, {:.2})/{inner}", inst.cell, inst.xf.dx * unit_um, inst.xf.dy * unit_um));
    }
    None
}

/// Los abiertos, cortos y renombres de `cell` (a nivel de la celda) y sus
/// transistores propios que cambiaron. `changed`: las cajas (µm) de sus
/// cambios de geometría, para ubicar cada abierto o corto.
pub fn cell_changes(
    cell: &str,
    (a, ca): (&CellNets, &Cells),
    (b, cb): (&CellNets, &Cells),
    unit_um: f64,
    changed: &[[f64; 4]],
) -> (Vec<NetChange>, Vec<DeviceChange>) {
    let clock = std::time::Instant::now();
    let profile = std::env::var_os("RIKU_PROFILE").is_some();
    let lap = |what: &str| {
        if profile {
            eprintln!("[redes]   {cell} {what}: {:?}", clock.elapsed());
        }
    };
    let devices = device_changes(
        cell,
        &a.own.devices.iter().map(|(d, _)| d.clone()).collect::<Vec<_>>(),
        &b.own.devices.iter().map(|(d, _)| d.clone()).collect::<Vec<_>>(),
        unit_um,
    );
    lap("transistores");
    let (va, vb) = (view(a), view(b));
    let (twins, moved) = pair_insts(a, b, 1e-4 / unit_um);
    let mut extra: Vec<(usize, usize)> = Vec::new();
    for &(i, j) in &twins {
        let (ma, mb) = (&a.inst_maps[i], &b.inst_maps[j]);
        extra.extend(ma.iter().zip(mb).map(|(&x, &y)| (x as usize, y as usize)));
    }
    // Cada etiqueta con la del mismo texto más cerca (una celda puede tener
    // dos `gnd` en redes que se unen recién arriba); una vez por par de hijas.
    let mut by_pair: HashMap<(super::NetKey, super::NetKey), Vec<(usize, usize)>> = HashMap::new();
    for &(i, j) in &moved {
        let (ka, kb) = (a.insts[i].key, b.insts[j].key);
        let (Some(xa), Some(xb)) = (ca.get(&ka), cb.get(&kb)) else { continue };
        let pairs = by_pair
            .entry((ka, kb))
            .or_insert_with(|| label_pairs((&xa.own.labels, &xa.label_nets), (&xb.own.labels, &xb.label_nets)));
        for &(x, y) in pairs.iter() {
            extra.push((a.inst_maps[i][x] as usize, b.inst_maps[j][y] as usize));
        }
    }
    extra.sort_unstable();
    extra.dedup();
    lap(&format!("anclas ({} gemelas, {} cambiadas, {} anclas)", twins.len(), moved.len(), extra.len()));
    let name = |side_b: bool, i: usize| if side_b { from_child(b, cb, i, unit_um) } else { from_child(a, ca, i, unit_um) };
    let nets = net_changes_with(cell, (&va, &vb), unit_um, changed, &extra, &name);
    lap("redes");
    (nets, devices)
}
