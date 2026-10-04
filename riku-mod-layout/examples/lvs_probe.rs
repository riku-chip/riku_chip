//! Lo que la vista de LVS ubicaría en el layout para los nombres que da
//! Netgen (`riku lvs -f json`, del lado `layout`): cuántas compuertas o
//! pedazos devuelve la sonda y, de un dispositivo, la suma de W de los que
//! están en paralelo (Netgen los junta y los nombra por uno).
//!
//! ```text
//! lvs_probe <layout.gds|.oas> <celda> <nombre>…    # 19 20 7 9 0 Vout
//! lvs_probe [--sin-info] <celda.mag> <nombre>…     # Magic, como lo lee el visor
//! ```
//!
//! `--sin-info`: un `.mag` sin lo que el lector de Magic sabe de sus pines
//! (como abría el visor antes de la ronda 2), para ver qué nombres cambian.

use std::process::ExitCode;

use gdstk_rs::Library;
use riku_mod_layout::{devices, mag, nets};
use viewer_core::NetProbe;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let no_info = args.iter().position(|a| a == "--sin-info").map(|i| args.remove(i)).is_some();
    let Some(path) = args.first().cloned() else {
        eprintln!("uso: lvs_probe <layout> <celda> <nombre>…  |  lvs_probe [--sin-info] <celda.mag> <nombre>…");
        return ExitCode::from(2);
    };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (lib, info, cell, names) = if path.ends_with(".mag") {
        let (lib, info) = mag::build(&mag::collect(&bytes, &path, None).unwrap_or_else(|e| panic!("{path}: {e}")));
        let cell = std::path::Path::new(&path).file_stem().unwrap_or_default().to_string_lossy().to_string();
        (lib, (!no_info).then_some(info), cell, &args[1..])
    } else {
        let lib = Library::from_bytes_any(&bytes).unwrap_or_else(|e| panic!("{path}: {e:?}"));
        let Some(cell) = args.get(1).cloned() else {
            eprintln!("falta la celda");
            return ExitCode::from(2);
        };
        (lib, None, cell, &args[2..])
    };
    let Some(rules) = devices::rules_for_library(&lib, Some(&path)) else {
        eprintln!("ningún PDK conocido reconoce este layout");
        return ExitCode::from(1);
    };
    let Some(top) = lib.find_cell(&cell) else {
        eprintln!("{path}: sin celda {cell}");
        return ExitCode::from(1);
    };
    let nl = nets::cell_nets(&lib, &top, rules, info.as_ref());
    let probe = nets::LayoutNets::new(&nl, &|_| Vec::new(), lib.unit() / 1e-6);
    let groups = nets::parallel_groups(&nl);
    println!("{}: {} transistores, {} en paralelo (como Netgen), {} redes", cell, nl.devices.len(), groups.len(), nl.nets.len());
    println!("  pines: {}", nl.ports().join(" "));
    println!("  redes: {}", (0..nl.nets.len()).map(|i| nl.net_name(i)).collect::<Vec<_>>().join(" "));
    // W por L de un grupo: Netgen informa el de cada L por separado.
    let w_by_l = |g: &[usize]| -> String {
        let mut by_l: std::collections::BTreeMap<i64, f64> = Default::default();
        for &d in g {
            let dev = &nl.devices[d].0;
            *by_l.entry((dev.l_um * 1000.0).round() as i64).or_default() += dev.w_um;
        }
        by_l.iter().map(|(l, w)| format!("W={w:.3} L={:.3}", *l as f64 / 1000.0)).collect::<Vec<_>>().join(" + ")
    };
    for g in &groups {
        let (dev, t) = &nl.devices[g[0]];
        let n = |i: usize| nl.net_name(i);
        println!("  {:?}: {}  {}  g={} s/d={}/{} b={}", g, dev.model, w_by_l(g), n(t.g), n(t.s), n(t.d), n(t.b));
    }
    let mut ok = true;
    for name in names {
        if let Some(hit) = probe.device_named(name) {
            let i: usize = name.trim_start_matches(|c: char| c.is_ascii_alphabetic()).parse().unwrap_or(usize::MAX);
            let group = groups.iter().find(|g| g.contains(&i)).map_or(&[][..], Vec::as_slice);
            let model = nl.devices.get(i).map_or("?", |(d, _)| d.model.as_str());
            let same = hit.outline.len() == group.len();
            ok &= same;
            println!(
                "  {name:>6}  dispositivo  {model}  {} compuertas  ({}){}",
                hit.outline.len(),
                w_by_l(group),
                if same { "" } else { "  ← no coincide con el grupo" }
            );
        } else if let Some(hit) = probe.net_named(name) {
            println!("  {name:>6}  red          {} pedazos", hit.outline.len());
        } else {
            ok = false;
            println!("  {name:>6}  no se encontró");
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
