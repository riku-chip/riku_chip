//! Mide lo que decide el diseño de la fase 6.4 (huella por pedazos):
//! cómo se reparten las referencias de la celda top, si aplanar por pedazos
//! da los mismos polígonos que aplanar entero, cuánto pesa cada etapa de la
//! huella (aplanar, hashear, ordenar) y cuánta memoria ocupa cada polígono.
//!
//! ```text
//! profile_prints <layout.gds> [celda] [hilos]
//! ```

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use gdstk_rs::{Cell, Library, Polygon};

type Prints = BTreeMap<(u32, u32), Vec<u64>>;

fn kb(key: &str) -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with(key)).map(str::to_string))
        .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
        .unwrap_or(0.0)
}
fn rss_mb() -> f64 {
    kb("VmRSS:") / 1024.0
}
fn peak_mb() -> f64 {
    kb("VmHWM:") / 1024.0
}

fn quantize(q: gdstk_rs::Point2D) -> (i64, i64) {
    ((q.x * 1e6).round() as i64, (q.y * 1e6).round() as i64)
}

/// Forma canónica como en `gds_diff::canonical_points`, sobre un buffer.
fn canonical_into(p: &Polygon<'_>, v: &mut Vec<(i64, i64)>) {
    v.clear();
    v.extend(p.points().map(quantize));
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

fn hash_alloc(p: &Polygon<'_>) -> u64 {
    let mut v = Vec::new();
    canonical_into(p, &mut v);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (p.layer(), p.datatype(), v).hash(&mut h);
    h.finish()
}

fn hash_buf(p: &Polygon<'_>, v: &mut Vec<(i64, i64)>) -> u64 {
    canonical_into(p, v);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (p.layer(), p.datatype(), &*v).hash(&mut h);
    h.finish()
}

fn prints_of(flat: &gdstk_rs::FlattenedPolygons<'_>, buf: &mut Vec<(i64, i64)>, into: &mut Prints) {
    for p in flat.polygons() {
        into.entry((p.layer(), p.datatype())).or_default().push(hash_buf(&p, buf));
    }
}

fn sort_all(p: &mut Prints) {
    for v in p.values_mut() {
        v.sort_unstable();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("uso: profile_prints <layout.gds> [celda] [hilos]");
        std::process::exit(2);
    };
    let threads: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(12);

    let bytes = std::fs::read(path).expect("leer");
    let t = Instant::now();
    let lib = Library::from_bytes_any(&bytes).expect("gds");
    println!("leer: {:.2}s · RSS {:.0} MB", t.elapsed().as_secs_f64(), rss_mb());
    let cell: Cell<'_> = match args.get(1) {
        Some(n) => lib.find_cell(n).expect("celda"),
        None => lib.top_level().cells().next().expect("top"),
    };
    println!("celda {} · {} polígonos propios · {} referencias", cell.name(), cell.polygon_count(), cell.references().count());

    // ── 1. Reparto de las referencias ──
    println!("\n== Referencias de la top (polígonos aplanados de cada una)");
    let t = Instant::now();
    let mut per_ref: Vec<(u64, String)> = cell
        .references()
        .map(|r| (r.get_polygons().build().count(), r.cell_name().to_string()))
        .collect();
    let own = cell.get_polygons().depth(0).build().count();
    let total: u64 = per_ref.iter().map(|x| x.0).sum::<u64>() + own;
    per_ref.sort_by(|a, b| b.0.cmp(&a.0));
    println!("total {total} · propios {own} ({:.1} %) · contar tardó {:.2}s", 100.0 * own as f64 / total as f64, t.elapsed().as_secs_f64());
    for (n, name) in per_ref.iter().take(10) {
        println!("  {n:>10}  {:5.1} %  {name}", 100.0 * *n as f64 / total as f64);
    }
    let big = per_ref.first().map_or(0.0, |x| x.0 as f64 / total as f64);
    println!("  la mayor concentra el {:.1} % → tope de aceleración por pedazos ≈ {:.1}×", 100.0 * big, 1.0 / big.max(1e-9));

    // ── 2. Huella entera: aplanar, hashear con Vec nuevo, hashear con buffer, ordenar ──
    // SKIP_WHOLE=1 salta esta etapa para medir la memoria de los pedazos sin
    // lo que el allocator retiene del aplanado entero.
    let skip_whole = std::env::var_os("SKIP_WHOLE").is_some();
    let mut whole: Prints = BTreeMap::new();
    let mut buf = Vec::new();
    if !skip_whole {
    println!("\n== Huella entera");
    let rss0 = rss_mb();
    let t = Instant::now();
    let flat = cell.get_polygons().build();
    let t_flat = t.elapsed().as_secs_f64();
    let n = flat.count();
    let rss1 = rss_mb();
    println!("aplanar: {t_flat:.2}s · {n} polígonos · +{:.0} MB → {:.0} B/polígono", rss1 - rss0, (rss1 - rss0) * 1024.0 * 1024.0 / n as f64);

    let t = Instant::now();
    let mut a: Prints = BTreeMap::new();
    for p in flat.polygons() {
        a.entry((p.layer(), p.datatype())).or_default().push(hash_alloc(&p));
    }
    let t_hash_alloc = t.elapsed().as_secs_f64();
    let t = Instant::now();
    prints_of(&flat, &mut buf, &mut whole);
    let t_hash_buf = t.elapsed().as_secs_f64();
    let t = Instant::now();
    sort_all(&mut whole);
    let t_sort = t.elapsed().as_secs_f64();
    sort_all(&mut a);
    assert_eq!(a, whole, "hash con Vec nuevo y con buffer deben coincidir");
    println!("hashear: {t_hash_alloc:.2}s con Vec nuevo · {t_hash_buf:.2}s con buffer · ordenar {t_sort:.2}s");
    drop(flat);
    println!("tras soltar el aplanado: RSS {:.0} MB (pico {:.0} MB)", rss_mb(), peak_mb());
    }

    // ── 3. Por pedazos, un hilo: mismos hashes ──
    println!("\n== Huella por pedazos (propio + cada referencia)");
    let t = Instant::now();
    let mut chunked: Prints = BTreeMap::new();
    let mut peak_chunk = 0.0f64;
    {
        let f = cell.get_polygons().depth(0).build();
        prints_of(&f, &mut buf, &mut chunked);
    }
    for r in cell.references() {
        let f = r.get_polygons().build();
        prints_of(&f, &mut buf, &mut chunked);
        peak_chunk = peak_chunk.max(rss_mb());
    }
    sort_all(&mut chunked);
    println!("1 hilo: {:.2}s · RSS máximo durante los pedazos {:.0} MB", t.elapsed().as_secs_f64(), peak_chunk);
    if skip_whole { whole = chunked.clone(); } else { println!("iguales a la huella entera: {}", chunked == whole); }

    // ── 4. Por pedazos, en paralelo ──
    let refs: Vec<gdstk_rs::Reference<'_>> = cell.references().collect();
    for threads in [1, 2, 4, 6, threads] {
    let next = AtomicUsize::new(0);
    let t = Instant::now();
    let parts: Vec<Prints> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut out: Prints = BTreeMap::new();
                    let mut buf = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i > refs.len() {
                            break;
                        }
                        let f = if i == 0 { cell.get_polygons().depth(0).build() } else { refs[i - 1].get_polygons().build() };
                        prints_of(&f, &mut buf, &mut out);
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let t_par = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let mut merged: Prints = BTreeMap::new();
    for p in parts {
        for (k, v) in p {
            merged.entry(k).or_default().extend(v);
        }
    }
    sort_all(&mut merged);
    println!("{threads} hilos: {t_par:.2}s + juntar y ordenar {:.2}s · iguales: {} · pico del proceso {:.0} MB", t.elapsed().as_secs_f64(), merged == whole, peak_mb());
    }
}
