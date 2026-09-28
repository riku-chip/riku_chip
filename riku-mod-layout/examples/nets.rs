//! La netlist que Riku extrae de un layout, celda por celda, como `.subckt`
//! SPICE (para comparar con Netgen, ver `tools/verify/nets/`).
//!
//! ```text
//! nets [--unit u] <layout.gds|.oas> [celda…]   # sin celdas: las top cells
//! nets [--unit u] <celda.mag | carpeta>        # Magic: cada .mag con sus sub-celdas
//! ```
//!
//! `--unit`: el sufijo de W y L en µm (`u` por defecto; vacío para SKY130,
//! cuyas netlists usan `.option scale=1e-6`). Los avisos van a stderr.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use gdstk_rs::{Cell, Library};
use riku_mod_layout::{devices, mag, nets};

fn print_cell(lib: &Library, cell: &Cell<'_>, rules: &devices::DeviceRules, unit: &str, info: Option<&gdstk_rs::magic::MagInfo>) {
    let t = Instant::now();
    let nl = nets::cell_nets(lib, cell, rules, info);
    print!("{}", nets::spice(cell.name(), &nl, rules, unit));
    eprintln!("{}: {} redes, {} transistores en {:.2?}", cell.name(), nl.nets.len(), nl.devices.len(), t.elapsed());
    for w in &nl.warnings {
        eprintln!("  aviso: {w}");
    }
}

fn magic(path: &Path, unit: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let p = path.to_string_lossy();
    let (lib, info) = mag::build(&mag::collect(&bytes, &p, None)?);
    let rules = devices::rules_for_library(&lib, Some(&p)).ok_or("sin reglas para este layout")?;
    let name = path.file_stem().unwrap_or_default().to_string_lossy();
    let cell = lib.find_cell(&name).ok_or_else(|| format!("sin celda {name}"))?;
    print_cell(&lib, &cell, rules, unit, Some(&info));
    Ok(())
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut unit = "u".to_string();
    if let Some(i) = args.iter().position(|a| a == "--unit") {
        unit = args.get(i + 1).cloned().unwrap_or_default();
        args.drain(i..(i + 2).min(args.len()));
    }
    let Some(path) = args.first() else {
        eprintln!("uso: nets [--unit u] <layout> [celda…]");
        return ExitCode::from(2);
    };
    let p = Path::new(path);
    if p.is_dir() || p.extension().is_some_and(|e| e == "mag") {
        let mut files: Vec<_> = if p.is_dir() {
            std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| e.path()).filter(|f| f.extension().is_some_and(|e| e == "mag")).collect()).unwrap_or_default()
        } else {
            vec![p.to_path_buf()]
        };
        files.sort();
        for f in files {
            if let Err(e) = magic(&f, &unit) {
                eprintln!("{}: {e}", f.display());
            }
        }
        return ExitCode::SUCCESS;
    }
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let lib = Library::from_bytes_any(&bytes).unwrap_or_else(|e| panic!("{path}: {e:?}"));
    let Some(rules) = devices::rules_for_library(&lib, Some(path)) else {
        eprintln!("ningún PDK conocido reconoce este layout");
        return ExitCode::from(1);
    };
    let mut cells: Vec<_> = if args.len() > 1 {
        args[1..].iter().filter_map(|n| lib.find_cell(n)).collect()
    } else {
        lib.top_level().cells().collect()
    };
    cells.sort_by(|a, b| a.name().cmp(b.name()));
    for cell in cells {
        print_cell(&lib, &cell, rules, &unit, None);
    }
    ExitCode::SUCCESS
}
