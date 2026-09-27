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
}
