//! Prueba del LVS manual (ver `riku::lvs::manual`): el archivo de vínculos
//! entre los transistores del esquemático y los del layout, y lo que Riku
//! deduce de él.
//!
//! ```text
//! lvs_map <check|suggest|update|list> <raíz> <esquemático> <layout> [--cell C] [--map archivo]
//! ```
//!
//! - `check`: el estado (avance, parámetros, cortos, abiertos, lo movido).
//! - `suggest`: agrega al archivo los vínculos que se deducen sin adivinar.
//! - `update`: reescribe el archivo con las posiciones de ahora (después de
//!   mover la celda).
//! - `list`: los transistores de cada lado.
//!
//! El archivo por defecto es `lvs/<celda>.toml` en la raíz.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use riku::lvs::manual::{self, LayDevice, MapFile};
use riku::lvs::Pair;

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut opt = |name: &str| -> Option<String> {
        let i = args.iter().position(|a| a == name)?;
        let v = args.get(i + 1).cloned();
        args.drain(i..(i + 2).min(args.len()));
        v
    };
    let cell = opt("--cell");
    let map_arg = opt("--map");
    let [cmd, root, sch, layout] = args.as_slice() else {
        return Err("uso: lvs_map <check|suggest|update|list> <raíz> <esquemático> <layout> [--cell C] [--map archivo]".into());
    };
    let root = PathBuf::from(root);
    let pair = Pair { schematic: sch.clone(), layout: layout.clone(), cell: cell.clone() };
    let files: Arc<dyn viewer_core::FileSource> = Arc::new(viewer_core::DiskFiles::new(root.clone()));

    // Los dos lados.
    let s = riku::lvs::schematic_netlist(&pair, files.clone())?;
    let places = s.netlist.places.clone();
    let sch_devs = manual::schematic_devices(&s.netlist.text, &s.stem, &|n| places.instance(n).map(str::to_string));
    let bytes = std::fs::read(root.join(layout)).map_err(|e| format!("{layout}: {e}"))?;
    let ln = riku_mod_layout::nets::layout_netlist(&bytes, layout, Some(files.as_ref()), cell.as_deref())?;
    let lay_devs = manual::layout_devices(&ln);

    let map_path = map_arg.map(PathBuf::from).unwrap_or_else(|| root.join("lvs").join(format!("{}.toml", ln.cell)));
    let mut map = match std::fs::read_to_string(&map_path) {
        Ok(t) => MapFile::parse(&t).map_err(|e| format!("{}: {e}", map_path.display()))?,
        Err(_) => MapFile::new(sch, layout, cell.as_deref()),
    };

    match cmd.as_str() {
        "list" => {
            println!("Esquemático ({}):", sch_devs.len());
            for d in &sch_devs {
                println!("  {:<6} {:<28} D {} G {} S {} B {}  W {:?} L {:?} m {}", d.name, d.model, d.pins[0], d.pins[1], d.pins[2], d.pins[3], d.w, d.l, d.m);
            }
            println!("Layout, celda {} ({}):", ln.cell, lay_devs.len());
            for d in &lay_devs {
                println!("  {:<28} en ({:.3}, {:.3})  D {} G {} S {} B {}  W {} L {}", d.model, d.at.0, d.at.1, d.pins[0], d.pins[1], d.pins[2], d.pins[3], d.w, d.l);
            }
        }
        "suggest" => {
            let new = manual::suggest(&map, &sch_devs, &lay_devs);
            println!("{} vínculos sugeridos: {}", new.len(), new.iter().map(|b| b.schematic.as_str()).collect::<Vec<_>>().join(", "));
            map.binds.extend(new);
            save(&map_path, &map)?;
            report(&manual::check(&map, &sch_devs, &lay_devs), &lay_devs, sch_devs.len());
        }
        "update" => {
            let c = manual::check(&map, &sch_devs, &lay_devs);
            save(&map_path, &c.updated)?;
            report(&c, &lay_devs, sch_devs.len());
        }
        "check" => report(&manual::check(&map, &sch_devs, &lay_devs), &lay_devs, sch_devs.len()),
        other => return Err(format!("no conozco «{other}»")),
    }
    Ok(())
}

fn save(path: &Path, map: &MapFile) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, map.to_text()).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("→ {}", path.display());
    Ok(())
}

fn report(c: &manual::Check, lay: &[LayDevice], total_sch: usize) {
    let fingers: usize = c.bound.iter().map(|(_, f)| f.len()).sum();
    println!(
        "Vinculados: {} de {} transistores del esquemático · {} de {} del layout",
        c.bound.len(),
        total_sch,
        fingers,
        lay.len()
    );
    if let Some(m) = c.moved {
        println!("  la celda se movió: giro {}°{} y ({:.3}, {:.3}) µm; {} reubicados", (m.orient % 4) as u32 * 90, if m.orient >= 4 { " espejada" } else { "" }, m.dx, m.dy, m.count);
    }
    if !c.by_connectivity.is_empty() {
        println!("  reubicados por conectividad: {}", c.by_connectivity.join(", "));
    }
    for (s, what) in &c.models {
        println!("  modelo distinto en {s}: {what}");
    }
    for (s, what) in &c.params {
        println!("  parámetro distinto en {s}: {what}");
    }
    for (l, ss) in &c.shorts {
        println!("  CORTO: en el layout {l} une {}", ss.join(", "));
    }
    for (s, ls) in &c.opens {
        println!("  ABIERTO: {s} está partida en el layout: {}", ls.join(", "));
    }
    for (s, r) in &c.lost {
        println!("  perdido: {s} ({} en {:?})", r.model, r.at);
    }
    for s in &c.unknown {
        println!("  {s} ya no está en el esquemático");
    }
    if !c.unbound_schematic.is_empty() {
        println!("  sin vincular en el esquemático: {}", c.unbound_schematic.join(", "));
    }
    if !c.unbound_layout.is_empty() {
        let at: Vec<String> = c.unbound_layout.iter().map(|&i| format!("{} ({:.2}, {:.2})", short(&lay[i].model), lay[i].at.0, lay[i].at.1)).collect();
        println!("  sin vincular en el layout: {}", at.join(", "));
    }
    println!("{}", if c.clean() { "LVS manual: limpio." } else { "LVS manual: con pendientes." });
}

fn short(m: &str) -> &str {
    m.rsplit("__").next().unwrap_or(m)
}
