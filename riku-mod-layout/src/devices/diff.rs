//! Transistores que cambiaron entre dos versiones de una celda.
//!
//! Se emparejan por posición: el de A y el de B son el mismo si la compuerta
//! de uno contiene el punto interior del otro (un transistor que se agrandó
//! sigue siendo el mismo; uno que se movió más que su propio tamaño cuenta
//! como quitado y agregado). Entre los emparejados, cambió si cambió el
//! modelo, W o L (más de medio nanómetro).

use std::collections::HashMap;

use gdstk_rs::{Cell, Library};

use super::extract::{point_in, Device};
use super::DeviceRules;

/// Un transistor en el reporte: sus medidas y dónde está (µm).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceDesc {
    pub model: String,
    pub w_um: f64,
    pub l_um: f64,
    /// Un punto dentro de la compuerta.
    pub at_um: [f64; 2],
    /// Caja de la compuerta: `[min_x, min_y, max_x, max_y]`.
    pub bbox_um: [f64; 4],
}

/// Un transistor agregado (`before` vacío), quitado (`after` vacío) o que
/// cambió de modelo, W o L.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceChange {
    pub cell: String,
    pub before: Option<DeviceDesc>,
    pub after: Option<DeviceDesc>,
}

/// Medio nanómetro: W y L salen de áreas y bordes en punto flotante.
const TOLERANCE_UM: f64 = 5e-4;

fn desc(d: &Device, unit_um: f64) -> DeviceDesc {
    let b = d.gate.points.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, q| {
        [b[0].min(q.x), b[1].min(q.y), b[2].max(q.x), b[3].max(q.y)]
    });
    DeviceDesc {
        model: d.model.clone(),
        w_um: d.w_um,
        l_um: d.l_um,
        at_um: [d.at.0 * unit_um, d.at.1 * unit_um],
        bbox_um: b.map(|v| v * unit_um),
    }
}

/// Los cambios entre los transistores de una celda en A y en B (mismas
/// unidades de librería en los dos lados; `unit_um`: µm por unidad).
pub fn device_changes(cell: &str, a: &[Device], b: &[Device], unit_um: f64) -> Vec<DeviceChange> {
    let mut used = vec![false; a.len()];
    let mut out = Vec::new();
    for db in b {
        let hit = a.iter().enumerate().find(|(i, da)| {
            !used[*i] && (point_in(&da.gate.points, db.at.0, db.at.1) || point_in(&db.gate.points, da.at.0, da.at.1))
        });
        match hit {
            Some((i, da)) => {
                used[i] = true;
                let same = da.model == db.model
                    && (da.w_um - db.w_um).abs() <= TOLERANCE_UM
                    && (da.l_um - db.l_um).abs() <= TOLERANCE_UM;
                if !same {
                    out.push(DeviceChange {
                        cell: cell.to_string(),
                        before: Some(desc(da, unit_um)),
                        after: Some(desc(db, unit_um)),
                    });
                }
            }
            None => out.push(DeviceChange { cell: cell.to_string(), before: None, after: Some(desc(db, unit_um)) }),
        }
    }
    for (i, da) in a.iter().enumerate() {
        if !used[i] {
            out.push(DeviceChange { cell: cell.to_string(), before: Some(desc(da, unit_um)), after: None });
        }
    }
    out
}

/// Polígonos que tendría la celda aplanada, sin aplanarla: los propios más
/// los de cada instancia por sus repeticiones.
pub fn flat_polygon_estimate(lib: &Library, cell: &Cell<'_>) -> u64 {
    fn walk(lib: &Library, cell: &Cell<'_>, memo: &mut HashMap<String, u64>, depth: u32) -> u64 {
        if let Some(&n) = memo.get(cell.name()) {
            return n;
        }
        let mut n = cell.polygon_count();
        if depth < 64 {
            for r in cell.references() {
                if let Some(child) = lib.find_cell(r.cell_name()) {
                    n = n.saturating_add(walk(lib, &child, memo, depth + 1).saturating_mul(r.repetition_count().max(1)));
                }
            }
        }
        memo.insert(cell.name().to_string(), n);
        n
    }
    walk(lib, cell, &mut HashMap::new(), 0)
}

/// Transistores de una celda en los dos lados, si no es demasiado grande
/// (`max_polygons` aplanados); `None` si lo es.
pub fn cell_device_changes(
    la: &Library,
    lb: &Library,
    name: &str,
    rules: &DeviceRules,
    max_polygons: u64,
) -> Option<Vec<DeviceChange>> {
    let (ca, cb) = (la.find_cell(name)?, lb.find_cell(name)?);
    if flat_polygon_estimate(la, &ca).max(flat_polygon_estimate(lb, &cb)) > max_polygons {
        return None;
    }
    let (a, b) = rayon::join(|| super::cell_devices(la, &ca, rules), || super::cell_devices(lb, &cb, rules));
    Some(device_changes(name, &a, &b, lb.unit() / 1e-6))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdstk_rs::{OwnedPolygon, Point2D};

    fn dev(model: &str, x: f64, w: f64) -> Device {
        let p = |x, y| Point2D { x, y };
        Device {
            model: model.into(),
            magic: "nfet".into(),
            gate: OwnedPolygon { layer: 0, datatype: 0, points: vec![p(x, 0.0), p(x + 0.15, 0.0), p(x + 0.15, w), p(x, w)] },
            at: (x + 0.075, w / 2.0),
            w_um: w,
            l_um: 0.15,
            sd_at: Vec::new(),
        }
    }

    #[test]
    fn pairs_by_position_and_reports_what_changed() {
        let a = [dev("nfet", 0.0, 0.42), dev("nfet", 1.0, 0.65), dev("pfet", 2.0, 1.0)];
        // El primero se agrandó, el segundo igual, el tercero se quitó y hay uno nuevo.
        let b = [dev("nfet", 0.0, 0.84), dev("nfet", 1.0, 0.65), dev("nfet", 5.0, 0.42)];
        let ch = device_changes("INV", &a, &b, 1.0);
        let summary: Vec<(Option<f64>, Option<f64>)> =
            ch.iter().map(|c| (c.before.as_ref().map(|d| d.w_um), c.after.as_ref().map(|d| d.w_um))).collect();
        assert_eq!(summary, [(Some(0.42), Some(0.84)), (None, Some(0.42)), (Some(1.0), None)]);
        assert!(ch.iter().all(|c| c.cell == "INV"));
        assert!(device_changes("INV", &a, &a, 1.0).is_empty(), "sin cambios");
    }

    #[test]
    fn a_model_change_with_the_same_size_is_a_change() {
        let ch = device_changes("X", &[dev("nfet_01v8", 0.0, 0.42)], &[dev("nfet_01v8_lvt", 0.0, 0.42)], 1.0);
        assert_eq!(ch.len(), 1);
        assert_eq!(ch[0].after.as_ref().unwrap().model, "nfet_01v8_lvt");
    }
}
