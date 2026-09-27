//! Mide dónde se va el tiempo del diff de un layout grande, etapa por etapa,
//! con el mismo trabajo que hace `gds_diff` (leer, huella por celda aplanando
//! la jerarquía, aplanar por capa y XOR) pero con un tope de tiempo por etapa:
//! lo que no alcanza se extrapola.
//!
//! ```text
//! profile_diff <a.gds> <b.gds> [segundos_por_etapa]
//! ```

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use gdstk_rs::{xor_split_flat, Library};

fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).map(str::to_string))
        .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
        .map_or(0.0, |kb| kb / 1024.0)
}

fn load(path: &str) -> (Library, Duration) {
    let t = Instant::now();
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let lib = Library::from_bytes_any(&bytes).unwrap_or_else(|e| panic!("{path}: {e:?}"));
    (lib, t.elapsed())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [a, b, rest @ ..] = args.as_slice() else {
        eprintln!("uso: profile_diff <a.gds> <b.gds> [segundos_por_etapa]");
        std::process::exit(2);
    };
    let budget = Duration::from_secs_f64(rest.first().and_then(|s| s.parse().ok()).unwrap_or(60.0));

    println!("== Lectura");
    let (la, ta) = load(a);
    println!("A: {:.2}s  (RSS {:.0} MB)", ta.as_secs_f64(), rss_mb());
    let (lb, tb) = load(b);
    println!("B: {:.2}s  (RSS {:.0} MB)", tb.as_secs_f64(), rss_mb());

    let names = |l: &Library| -> BTreeSet<String> { l.cells().map(|c| c.name().to_string()).collect() };
    let (na, nb) = (names(&la), names(&lb));
    let common: Vec<&String> = na.intersection(&nb).collect();
    let own_polys: u64 = lb.cells().map(|c| c.polygon_count()).sum();
    let refs: usize = lb.cells().map(|c| c.references().count()).sum();
    let layers: BTreeSet<(u32, u32)> =
        la.layers().into_iter().chain(lb.layers()).map(|t| (t.layer, t.datatype)).collect();
    println!(
        "celdas A {} · B {} · comunes {} · polígonos propios (B) {own_polys} · references (B) {refs} · capas {}",
        na.len(),
        nb.len(),
        common.len(),
        layers.len()
    );
    let tops: Vec<String> = lb.top_level().cells().map(|c| c.name().to_string()).collect();
    println!("top cells (B): {tops:?}");

    // ── Etapa 1: huella = aplanar toda la jerarquía de cada celda común ──
    println!("\n== Huella por celda (aplanar jerarquía completa, A y B)");
    let skip_fp = std::env::var_os("SKIP_FP").is_some();
    let t = Instant::now();
    let mut per_cell: Vec<(f64, u64, &str)> = Vec::new();
    let mut flat_total = 0u64;
    for name in &common {
        if skip_fp || t.elapsed() > budget {
            break;
        }
        let tc = Instant::now();
        let (ca, cb) = (la.find_cell(name).unwrap(), lb.find_cell(name).unwrap());
        let fa = ca.get_polygons().build();
        let fb = cb.get_polygons().build();
        let n = fa.count() + fb.count();
        flat_total += n;
        per_cell.push((tc.elapsed().as_secs_f64(), n, name.as_str()));
    }
    let done = per_cell.len();
    let spent = t.elapsed().as_secs_f64();
    println!(
        "{done}/{} celdas en {spent:.1}s · {flat_total} polígonos aplanados · RSS {:.0} MB",
        common.len(),
        rss_mb()
    );
    if done < common.len() {
        println!("  (tope alcanzado: el resto no se midió)");
    }
    per_cell.sort_by(|x, y| y.0.total_cmp(&x.0));
    println!("  más lentas:");
    for (s, n, name) in per_cell.iter().take(8) {
        println!("    {s:8.3}s  {n:>10} pol  {name}");
    }
    let leaf_like = per_cell.iter().filter(|(s, ..)| *s < 0.001).count();
    println!("  celdas < 1 ms: {leaf_like}");

    // ── Etapa 1b (PRINTS=1): por capa de la top, cuántos polígonos tienen
    //    un gemelo exacto (mismos vértices, en el mismo orden) en el otro lado.
    if std::env::var_os("PRINTS").is_some() {
        if let (Some(top), true) = (tops.first(), true) {
            let (ca, cb) = (la.find_cell(top).unwrap(), lb.find_cell(top).unwrap());
            println!("\n== Gemelos exactos por capa en {top}");
            // CANON=1: sentido antihorario y empezando por el vértice menor.
            let canon = std::env::var_os("CANON").is_some();
            let q = |p: &gdstk_rs::Polygon<'_>| -> Vec<(i64, i64)> {
                let mut v: Vec<(i64, i64)> =
                    p.points().map(|v| ((v.x * 1e6).round() as i64, (v.y * 1e6).round() as i64)).collect();
                if canon {
                    v.dedup();
                    if v.len() > 1 && v.first() == v.last() {
                        v.pop();
                    }
                    let n = v.len();
                    let area2: i128 = (0..n)
                        .map(|i| {
                            let (a, b) = (v[i], v[(i + 1) % n]);
                            a.0 as i128 * b.1 as i128 - b.0 as i128 * a.1 as i128
                        })
                        .sum();
                    if area2 < 0 {
                        v.reverse();
                    }
                    if let Some(k) = (0..n).min_by_key(|&i| v[i]) {
                        v.rotate_left(k);
                    }
                }
                v
            };
            for (l, d) in &layers {
                let t = Instant::now();
                let fa = ca.get_polygons().with_filter(*l, *d).build();
                let fb = cb.get_polygons().with_filter(*l, *d).build();
                let mut set: std::collections::HashMap<Vec<(i64, i64)>, usize> = std::collections::HashMap::new();
                for p in fa.polygons() {
                    *set.entry(q(&p)).or_default() += 1;
                }
                let mut twins = 0usize;
                for p in fb.polygons() {
                    if let Some(n) = set.get_mut(&q(&p)) {
                        if *n > 0 {
                            *n -= 1;
                            twins += 1;
                        }
                    }
                }
                println!("    {l:>3}/{d:<3} A {:>9} · B {:>9} · gemelos {twins:>9} · {:.2}s", fa.count(), fb.count(), t.elapsed().as_secs_f64());
            }
        }
        return;
    }

    // ── Etapa 2: la celda top, capa por capa: aplanar filtrado + XOR ──
    let Some(top) = tops.first() else { return };
    let (ca, cb) = (la.find_cell(top), lb.find_cell(top));
    let (Some(ca), Some(cb)) = (ca, cb) else {
        println!("\n(la top de B no existe en A)");
        return;
    };
    println!("\n== Celda top {top}: por capa, aplanar A y B filtrado + XOR");
    let t = Instant::now();
    let (mut t_flat, mut t_xor, mut n_layers) = (0.0, 0.0, 0usize);
    let mut rows = Vec::new();
    for (l, d) in &layers {
        if t.elapsed() > budget {
            break;
        }
        let tf = Instant::now();
        let fa = ca.get_polygons().with_filter(*l, *d).build();
        let fb = cb.get_polygons().with_filter(*l, *d).build();
        let f = tf.elapsed().as_secs_f64();
        let tx = Instant::now();
        let split = xor_split_flat(&fa, &fb);
        let x = tx.elapsed().as_secs_f64();
        t_flat += f;
        t_xor += x;
        n_layers += 1;
        let row = (f + x, *l, *d, fa.count() + fb.count(), f, x, split.added.len() + split.removed.len());
        println!("    {:>3}/{:<3} {:>9} pol  aplanar {:7.2}s  XOR {:7.2}s  → {} pol  (RSS {:.0} MB)", row.1, row.2, row.3, row.4, row.5, row.6, rss_mb());
        rows.push(row);
    }
    println!(
        "{n_layers}/{} capas en {:.1}s · aplanar {t_flat:.1}s · XOR {t_xor:.1}s · RSS {:.0} MB",
        layers.len(),
        t.elapsed().as_secs_f64(),
        rss_mb()
    );
    rows.sort_by(|x, y| y.0.total_cmp(&x.0));
    for (_, l, d, n, f, x, diff) in rows.iter().take(8) {
        println!("    {l:>3}/{d:<3} {n:>9} pol  aplanar {f:7.2}s  XOR {x:7.2}s  → {diff} pol de diferencia");
    }
    if n_layers < layers.len() {
        let per = t.elapsed().as_secs_f64() / n_layers.max(1) as f64;
        println!("  extrapolado a todas las capas: ~{:.0}s", per * layers.len() as f64);
    }
}
