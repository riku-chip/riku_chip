//! Extrae una celda de las dos maneras, plana y jerárquica, y las compara
//! (ver `docs/ronda-5/design.md`, D20.5).
//!
//! ```text
//! hier_check <layout.gds|.oas|.mag> [celda]
//! RIKU_HIER_INLINE=256   # umbral para meter una sub-celda en su padre
//! ```

use std::process::ExitCode;
use std::time::Instant;

use gdstk_rs::Library;
use riku_mod_layout::{devices, mag, nets};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("uso: hier_check <layout> [celda]");
        return ExitCode::from(2);
    };
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (lib, info) = if path.ends_with(".mag") {
        let p = std::path::Path::new(path);
        let dir = p.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
        let folder = viewer_core::DiskFiles::new(dir);
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let col = mag::collect(&bytes, &name, Some(&folder)).unwrap_or_else(|e| panic!("{path}: {e}"));
        let (lib, info) = mag::build(&col);
        (lib, Some(info))
    } else {
        (Library::from_bytes_any(&bytes).unwrap_or_else(|e| panic!("{path}: {e:?}")), None)
    };
    let rules = devices::rules_for_library(&lib, Some(path)).expect("PDK");
    let top = match args.get(1) {
        Some(c) => lib.find_cell(c).expect("celda"),
        None => lib.top_level().cell(0),
    };
    let unit_um = lib.unit() / 1e-6;
    let skip_flat = std::env::var("HIER_NO_FLAT").is_ok();
    let t = Instant::now();
    let opts = nets::hier::Options::default();
    let h = nets::hier::extract(&lib, &top, rules, info.as_ref(), opts);
    let (root, cells, stats) = (&h.root, &h.cells, h.stats);
    let hier = h.flatten(false);
    let th = t.elapsed();
    println!(
        "{}: jerárquica {:?} ({} celdas + {} de memoria, {} metidas, vecindades {} + {} reusadas): {} transistores, {} redes",
        top.name(),
        th,
        stats.cells,
        stats.remembered,
        stats.inlined,
        stats.neighbourhoods,
        stats.reused,
        hier.devices.len(),
        hier.nets.len()
    );
    if std::env::var_os("HIER_TWICE").is_some() {
        let t = Instant::now();
        let h2 = nets::hier::extract(&lib, &top, rules, info.as_ref(), opts);
        println!("  otra vez: {:?} ({} celdas + {} de memoria)", t.elapsed(), h2.stats.cells, h2.stats.remembered);
    }
    if let Ok(w) = std::env::var("HIER_WHERE") {
        let v: Vec<f64> = w.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        for line in nets::hier::locate(root, cells, (v[0] / unit_um, v[1] / unit_um)) {
            println!("  @ {line}");
        }
    }
    if skip_flat {
        return ExitCode::SUCCESS;
    }
    let t = Instant::now();
    let flat = nets::cell_nets(&lib, &top, rules, info.as_ref());
    println!("  plana {:?}: {} transistores, {} redes", t.elapsed(), flat.devices.len(), flat.nets.len());
    let diffs = nets::hier::same_netlist(&flat, &hier, unit_um, 30);
    if diffs.is_empty() {
        println!("  IGUALES");
        ExitCode::SUCCESS
    } else {
        for d in &diffs {
            println!("  ≠ {d}");
        }
        ExitCode::from(1)
    }
}
