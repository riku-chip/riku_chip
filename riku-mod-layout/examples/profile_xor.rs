//! Mide lo que decide el XOR por cuadrantes (fase 6.5.b): el XOR de una capa
//! de una instancia entre dos versiones, entero contra partido en k×k
//! cuadrantes (cada polígono va a los cuadrantes que toca su bbox y el
//! resultado se recorta al cuadrante), comparando tiempos y áreas.
//!
//! ```text
//! profile_xor <a.gds> <b.gds> <cell instanciada> <layer> <datatype>
//! ```

use std::time::Instant;

use gdstk_rs::{xor_split_owned, GdsTag, Library, OwnedPolygon, Point2D};
use rayon::prelude::*;

fn area(v: &[OwnedPolygon]) -> f64 {
    v.iter()
        .map(|p| {
            let n = p.points.len();
            (0..n).map(|i| p.points[i].x * p.points[(i + 1) % n].y - p.points[(i + 1) % n].x * p.points[i].y).sum::<f64>().abs()
                / 2.0
        })
        .sum()
}

fn bbox(p: &OwnedPolygon) -> [f64; 4] {
    p.points.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, q| {
        [b[0].min(q.x), b[1].min(q.y), b[2].max(q.x), b[3].max(q.y)]
    })
}

/// Recorta un polígono a un rectángulo (Sutherland–Hodgman).
fn clip(p: &OwnedPolygon, r: [f64; 4]) -> Option<OwnedPolygon> {
    let mut pts: Vec<Point2D> = p.points.clone();
    for side in 0..4 {
        let inside = |q: &Point2D| match side {
            0 => q.x >= r[0],
            1 => q.x <= r[2],
            2 => q.y >= r[1],
            _ => q.y <= r[3],
        };
        let cross = |a: &Point2D, b: &Point2D| -> Point2D {
            let (v, horizontal) = match side {
                0 => (r[0], false),
                1 => (r[2], false),
                2 => (r[1], true),
                _ => (r[3], true),
            };
            if horizontal {
                let t = (v - a.y) / (b.y - a.y);
                Point2D { x: a.x + t * (b.x - a.x), y: v }
            } else {
                let t = (v - a.x) / (b.x - a.x);
                Point2D { x: v, y: a.y + t * (b.y - a.y) }
            }
        };
        let mut out = Vec::with_capacity(pts.len() + 4);
        for i in 0..pts.len() {
            let (a, b) = (&pts[(i + pts.len() - 1) % pts.len()], &pts[i]);
            match (inside(a), inside(b)) {
                (true, true) => out.push(*b),
                (true, false) => out.push(cross(a, b)),
                (false, true) => {
                    out.push(cross(a, b));
                    out.push(*b);
                }
                (false, false) => {}
            }
        }
        pts = out;
        if pts.len() < 3 {
            return None;
        }
    }
    Some(OwnedPolygon { layer: p.layer, datatype: p.datatype, points: pts })
}

fn instance_polys(lib: &Library, name: &str, tag: GdsTag) -> Vec<OwnedPolygon> {
    let top = lib.top_level().cells().next().expect("top");
    // El nombre de la top: la celda entera; si no, su primera instancia con
    // ese nombre.
    let flat = if top.name() == name {
        top.get_polygons().with_filter(tag.layer, tag.datatype).build()
    } else {
        let r = top.references().find(|r| r.cell_name() == name).expect("instancia");
        r.get_polygons().with_filter(tag.layer, tag.datatype).build()
    };
    flat.polygons().map(|p| OwnedPolygon { layer: p.layer(), datatype: p.datatype(), points: p.points().collect() }).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [a, b, name, layer, dt] = args.as_slice() else {
        eprintln!("uso: profile_xor <a.gds> <b.gds> <cell instanciada> <layer> <datatype>");
        std::process::exit(2);
    };
    let tag = GdsTag { layer: layer.parse().unwrap(), datatype: dt.parse().unwrap() };
    let (la, lb) = (
        Library::from_bytes_any(&std::fs::read(a).unwrap()).unwrap(),
        Library::from_bytes_any(&std::fs::read(b).unwrap()).unwrap(),
    );
    let (pa, pb) = (instance_polys(&la, name, tag), instance_polys(&lb, name, tag));
    let edges: usize = pa.iter().chain(&pb).map(|p| p.points.len()).sum();
    println!("{name} {}/{}: {} + {} polígonos, {edges} vértices", tag.layer, tag.datatype, pa.len(), pb.len());

    // SKIP_WHOLE=1: no medir el XOR entero (cuando tarda minutos).
    let (ref_add, ref_rem) = if std::env::var_os("SKIP_WHOLE").is_some() {
        (f64::NAN, f64::NAN)
    } else {
        let t = Instant::now();
        let whole = xor_split_owned(&pa, &pb, tag);
        let t_whole = t.elapsed();
        let (ref_add, ref_rem) = (area(&whole.added), area(&whole.removed));
        println!(
            "entero: {t_whole:.2?} · +{:.3} −{:.3} µm² ({} + {} polígonos)",
            ref_add,
            ref_rem,
            whole.added.len(),
            whole.removed.len()
        );
        (ref_add, ref_rem)
    };

    let all: Vec<[f64; 4]> = pa.iter().chain(&pb).map(bbox).collect();
    let bounds = all.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, x| {
        [b[0].min(x[0]), b[1].min(x[1]), b[2].max(x[2]), b[3].max(x[3])]
    });
    for k in [2usize, 4, 8, 16, 32] {
        let (w, h) = ((bounds[2] - bounds[0]) / k as f64, (bounds[3] - bounds[1]) / k as f64);
        let tiles: Vec<[f64; 4]> = (0..k * k)
            .map(|i| {
                let (gx, gy) = ((i % k) as f64, (i / k) as f64);
                [bounds[0] + gx * w, bounds[1] + gy * h, bounds[0] + (gx + 1.0) * w, bounds[1] + (gy + 1.0) * h]
            })
            .collect();
        let touches = |b: &[f64; 4], t: &[f64; 4]| b[0] <= t[2] && b[2] >= t[0] && b[1] <= t[3] && b[3] >= t[1];
        let pick = |polys: &[OwnedPolygon], t: &[f64; 4]| -> Vec<OwnedPolygon> {
            polys.iter().filter(|p| touches(&bbox(p), t)).cloned().collect()
        };
        let run = |t: &[f64; 4]| -> (f64, f64, usize, std::time::Duration) {
            let t0 = Instant::now();
            let (sa, sb) = (pick(&pa, t), pick(&pb, t));
            let split = xor_split_owned(&sa, &sb, tag);
            let add: Vec<OwnedPolygon> = split.added.iter().filter_map(|p| clip(p, *t)).collect();
            let rem: Vec<OwnedPolygon> = split.removed.iter().filter_map(|p| clip(p, *t)).collect();
            (area(&add), area(&rem), sa.len() + sb.len(), t0.elapsed())
        };
        let t = Instant::now();
        let seq: Vec<_> = tiles.iter().map(run).collect();
        let t_seq = t.elapsed();
        let t = Instant::now();
        let par: Vec<_> = tiles.par_iter().map(run).collect();
        let t_par = t.elapsed();
        let (add, rem): (f64, f64) = (seq.iter().map(|x| x.0).sum(), seq.iter().map(|x| x.1).sum());
        let fed: usize = seq.iter().map(|x| x.2).sum();
        let slowest = seq.iter().map(|x| x.3).max().unwrap();
        let ok = (add - ref_add).abs() < 1e-6 && (rem - ref_rem).abs() < 1e-6;
        println!(
            "{k:>2}×{k:<2}: secuencial {t_seq:.2?} · paralelo {t_par:.2?} · cuadrante más lento {slowest:.2?} · polígonos entregados {fed} ({:.2}× por los bordes) · áreas {} (+{:.3} −{:.3}) · paralelo {:?}",
            fed as f64 / (pa.len() + pb.len()) as f64,
            if ok { "iguales" } else { "DISTINTAS" },
            add,
            rem,
            par.len()
        );
    }
}
