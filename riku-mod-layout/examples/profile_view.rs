//! Mide cuánto tarda el visor en armar la escena de un layout (lo que hace
//! `riku gui archivo.gds` antes de dibujar), sin abrir ventana.
//!
//! ```text
//! profile_view <layout>
//! ```

use std::time::Instant;

use riku_mod_layout::GdsBackend;
use viewer_core::{CancellationToken, ViewerBackend};

fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).map(str::to_string))
        .and_then(|l| l.split_whitespace().nth(1)?.parse::<f64>().ok())
        .map_or(0.0, |kb| kb / 1024.0)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let path = std::env::args().nth(1).expect("uso: profile_view <layout>");
    let bytes = std::fs::read(&path).expect("no se pudo leer el archivo");
    let t = Instant::now();
    let scene = GdsBackend::new()
        .load(bytes, Some(path.clone()), CancellationToken::new())
        .await
        .expect("no se pudo cargar");
    println!(
        "escena: {} elementos en {:.2}s · RSS {:.0} MB",
        scene.len(),
        t.elapsed().as_secs_f64(),
        rss_mb()
    );
    for (k, v) in scene.metadata() {
        println!("  {k}: {v}");
    }

    // Chip completo en un lienzo de 870 px: qué va uno a uno, por capa y tamaño.
    let Some((index, elements)) = scene.indexed() else { return };
    let bb = scene.bbox();
    let px_world = bb.width().max(bb.height()) / 870.0;
    let t = Instant::now();
    let q = viewer_core::LodQuery {
        bbox: bb,
        px_world,
        lod: true,
        block_px: viewer_core::index::BLOCK_PX,
        min_text_px: 3.0,
        budget: viewer_core::index::BUDGET,
    };
    let vis = index.visible(elements, &q, &|_| false);
    println!(
        "\nchip completo (1 px = {px_world:.3}): {} uno a uno, nivel {:?}, consulta {:.1} ms",
        vis.elements.len(),
        vis.level,
        t.elapsed().as_secs_f64() * 1e3
    );
    let mut per_layer: std::collections::BTreeMap<u16, (usize, f64, f64)> = Default::default();
    for &i in &vis.elements {
        let e = per_layer.entry(elements[i as usize].layer()).or_insert((0, f64::MAX, 0.0));
        e.0 += 1;
        e.1 = e.1.min(index.min_size(i as usize) / px_world);
        e.2 = e.2.max(index.size(i as usize) / px_world);
    }
    let mut rows: Vec<_> = per_layer.into_iter().collect();
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (layer, (n, min_w, max_s)) in rows.iter().take(10) {
        let name = scene.layer_paint(*layer).map_or(String::new(), |p| p.name.clone());
        println!("  {name:>16}: {n:>8} · ancho mín {min_w:.2} px · lado máx {max_s:.0} px");
    }
}
