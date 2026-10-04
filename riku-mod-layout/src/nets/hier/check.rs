//! El oráculo de la ronda 5: dos netlists de la misma celda (la plana de
//! siempre y la jerárquica aplanada) son la misma si tienen los mismos
//! transistores (modelo, W, L y posición) y las mismas redes: lo que une cada
//! una (sus terminales y sus etiquetas), con los mismos nombres y pines.

use std::collections::HashMap;

use crate::nets::Netlist;

/// Las diferencias entre `a` y `b` (vacío si son la misma netlist), hasta
/// `max` de ellas. `unit_um`: µm por unidad, para mostrar posiciones.
pub fn same_netlist(a: &Netlist, b: &Netlist, unit_um: f64, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let q = |v: f64| (v * unit_um * 1e4).round() as i64;
    let key = |d: &crate::devices::Device| (d.model.clone(), q(d.at.0), q(d.at.1));
    let pos = |d: &crate::devices::Device| format!("{} en ({:.3}, {:.3})", d.model, d.at.0 * unit_um, d.at.1 * unit_um);
    let mut in_b: HashMap<(String, i64, i64), Vec<usize>> = HashMap::new();
    for (j, (d, _)) in b.devices.iter().enumerate() {
        in_b.entry(key(d)).or_default().push(j);
    }
    if a.devices.len() != b.devices.len() {
        out.push(format!("transistores: {} y {}", a.devices.len(), b.devices.len()));
    }
    // Red de A → red de B, y al revés: tienen que ser una función en cada sentido.
    let mut maps: (HashMap<usize, usize>, HashMap<usize, usize>) = (HashMap::new(), HashMap::new());
    type Maps = (HashMap<usize, usize>, HashMap<usize, usize>);
    let link = |m: &mut Maps, x: usize, y: usize, why: &dyn Fn() -> String, out: &mut Vec<String>| {
        let e1 = *m.0.entry(x).or_insert(y);
        let e2 = *m.1.entry(y).or_insert(x);
        if e1 != y {
            out.push(format!("la red {} de A va a dos de B ({} y {}): {}", a.net_name(x), b.net_name(e1), b.net_name(y), why()));
        }
        if e2 != x {
            out.push(format!("la red {} de B va a dos de A ({} y {}): {}", b.net_name(y), a.net_name(e2), a.net_name(x), why()));
        }
    };
    // Por posición exacta o, si no, el de B cuya compuerta contiene el punto de A.
    let mut used = vec![false; b.devices.len()];
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut alone = Vec::new();
    for (i, (da, _)) in a.devices.iter().enumerate() {
        let exact = in_b.get_mut(&key(da)).and_then(|js| js.pop());
        match exact {
            Some(j) => {
                used[j] = true;
                pairs.push((i, j));
            }
            None => alone.push(i),
        }
    }
    let b_grid = crate::devices::extract::Grid::new(b.devices.iter().map(|(d, _)| d.gate.clone()).collect());
    for i in alone {
        let da = &a.devices[i].0;
        match b_grid.find(da.at.0, da.at.1).map(|j| j as usize).filter(|&j| !used[j] && b.devices[j].0.model == da.model) {
            Some(j) => {
                used[j] = true;
                pairs.push((i, j));
            }
            None => out.push(format!("solo en A: {}", pos(da))),
        }
    }
    for (j, u) in used.iter().enumerate() {
        if !u {
            out.push(format!("solo en B: {}", pos(&b.devices[j].0)));
        }
    }
    for &(i, j) in &pairs {
        let (da, ta) = &a.devices[i];
        let (db, tb) = &b.devices[j];
        if (da.w_um - db.w_um).abs() > 1e-6 || (da.l_um - db.l_um).abs() > 1e-6 {
            out.push(format!("{}: W/L {}/{} y {}/{}", pos(da), da.w_um, da.l_um, db.w_um, db.l_um));
        }
        // Fuente y drenaje pueden venir en otro orden.
        let straight = maps.0.get(&ta.s).is_none_or(|&y| y == tb.s) && maps.0.get(&ta.d).is_none_or(|&y| y == tb.d);
        let (s, d) = if straight { (tb.s, tb.d) } else { (tb.d, tb.s) };
        let why = || pos(da);
        link(&mut maps, ta.g, tb.g, &why, &mut out);
        link(&mut maps, ta.s, s, &why, &mut out);
        link(&mut maps, ta.d, d, &why, &mut out);
        link(&mut maps, ta.b, tb.b, &why, &mut out);
    }
    for (i, (la, na)) in a.labels.iter().zip(&a.label_nets).enumerate() {
        let nb = b.labels.iter().zip(&b.label_nets).find(|(lb, _)| lb.text == la.text && lb.at == la.at).map(|(_, n)| *n);
        match (na, nb) {
            (Some(x), Some(Some(y))) => link(&mut maps, *x, y, &|| format!("etiqueta {}", la.text), &mut out),
            (None, Some(None)) => {}
            _ => out.push(format!("etiqueta {} ({i}): {:?} y {:?}", la.text, na, nb)),
        }
    }
    for (&x, &y) in &maps.0 {
        let (na, nb) = (&a.nets[x], &b.nets[y]);
        if na.name != nb.name || na.port != nb.port {
            out.push(format!("red {:?}/{:?}: pin {} y {}", na.name, nb.name, na.port, nb.port));
        }
        if na.substrate != nb.substrate {
            out.push(format!("red {}: sustrato {} y {}", a.net_name(x), na.substrate, nb.substrate));
        }
    }
    out.sort();
    out.dedup();
    let n = out.len();
    out.truncate(max);
    if n > max {
        out.push(format!("… {n} en total"));
    }
    out
}
