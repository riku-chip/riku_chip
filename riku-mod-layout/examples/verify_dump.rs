//! Volcado de texto de lo que Riku lee de un layout, en el mismo formato que
//! `tools/verify/klayout_dump.py`, para compararlos con `diff`.
//!
//! ```text
//! verify_dump cells <layout>            # cada top cell: bbox, polígonos y área por capa, labels
//! verify_dump xor <a> <b> <cell>        # área añadida / eliminada por capa (XOR)
//! ```
//!
//! Coordenadas y áreas en µm / µm². Ver `docs/dev/development.md`, «Verification».

use std::collections::BTreeMap;
use std::process::ExitCode;

use gdstk_rs::Library;
use riku_mod_layout::{diff_cell, flatten_labels, DiffConfig};

fn load(path: &str) -> Library {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    Library::from_bytes_any(&bytes).unwrap_or_else(|e| panic!("{path}: {e:?}"))
}

fn cells(path: &str) {
    let lib = load(path);
    let uf = lib.unit() / 1e-6;
    let mut tops: Vec<_> = lib.top_level().cells().collect();
    tops.sort_by(|a, b| a.name().cmp(b.name()));
    for cell in tops {
        println!("CELL {}", cell.name());
        let mut layers: BTreeMap<(u32, u32), (usize, f64)> = BTreeMap::new();
        for p in cell.get_polygons().build().polygons() {
            let e = layers.entry((p.layer(), p.datatype())).or_insert((0, 0.0));
            e.0 += 1;
            e.1 += p.area() * uf * uf;
        }
        if layers.is_empty() {
            println!("BBOX empty");
        } else {
            let b = cell.bbox();
            println!("BBOX {:.3} {:.3} {:.3} {:.3}", b.min_x * uf, b.min_y * uf, b.max_x * uf, b.max_y * uf);
        }
        for ((l, d), (n, area)) in &layers {
            println!("L {l}/{d} {n} {area:.6}");
        }
        let mut labels: Vec<String> = flatten_labels(&lib, &cell)
            .into_iter()
            .map(|t| {
                let (x, y) = (t.origin.x * uf, t.origin.y * uf);
                format!("{}/{} {} ({x:.3},{y:.3})", t.tag.layer, t.tag.datatype, t.text)
            })
            .collect();
        labels.sort();
        for l in labels {
            println!("T {l}");
        }
    }
}

fn xor(a: &str, b: &str, cell: &str) {
    let (la, lb) = (load(a), load(b));
    let d = diff_cell(Some(&la), Some(&lb), cell, &DiffConfig::default());
    let mut per_layer: BTreeMap<(u32, u32), (f64, f64)> = BTreeMap::new();
    for g in &d.geometry {
        let e = per_layer.entry((g.layer.layer, g.layer.datatype)).or_insert((0.0, 0.0));
        e.0 += g.added_area_um2;
        e.1 += g.removed_area_um2;
    }
    let mut rows: Vec<String> = per_layer.iter().map(|((l, d), (add, rem))| format!("{l}/{d} +{add:.6} -{rem:.6}")).collect();
    rows.sort();
    for r in rows {
        println!("{r}");
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["cells", path] => cells(path),
        ["xor", a, b, cell] => xor(a, b, cell),
        _ => {
            eprintln!("uso: verify_dump cells <layout> | verify_dump xor <a> <b> <cell>");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
