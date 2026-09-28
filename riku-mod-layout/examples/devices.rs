//! Transistores que Riku reconoce en un layout, celda por celda, en el mismo
//! formato que `tools/verify/devices/compare_spice.py` espera.
//!
//! ```text
//! devices <layout.gds|.oas> [celda…]   # sin celdas: las top cells
//! devices <celda.mag | carpeta>        # Magic: cada .mag con sus sub-celdas
//! ```
//!
//! Cada línea: `DEV <modelo> W=<µm> L=<µm>`, ordenadas; W y L con 3 decimales.

use std::path::Path;
use std::process::ExitCode;

use gdstk_rs::{Cell, Library};
use riku_mod_layout::{devices, mag};

fn print_cell(lib: &Library, cell: &Cell<'_>, rules: &devices::DeviceRules) {
    println!("CELL {}", cell.name());
    let mut lines: Vec<String> =
        devices::cell_devices(lib, cell, rules).iter().map(|d| format!("DEV {} W={:.3} L={:.3}", d.model, d.w_um, d.l_um)).collect();
    lines.sort();
    for l in lines {
        println!("{l}");
    }
}

/// Un `.mag` con sus sub-celdas (de su carpeta y del PDK).
fn magic(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let p = path.to_string_lossy();
    let (lib, _) = mag::build(&mag::collect(&bytes, &p, None)?);
    let rules = devices::rules_for_library(&lib, Some(&p)).ok_or("sin reglas de transistores para este layout")?;
    let name = path.file_stem().unwrap_or_default().to_string_lossy();
    let cell = lib.find_cell(&name).ok_or_else(|| format!("sin celda {name}"))?;
    print_cell(&lib, &cell, rules);
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("uso: devices <layout> [celda…]");
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
            if let Err(e) = magic(&f) {
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
    eprintln!("{} tipos de transistor", rules.devices.len());
    let mut cells: Vec<_> = if args.len() > 1 {
        args[1..].iter().filter_map(|n| lib.find_cell(n)).collect()
    } else {
        lib.top_level().cells().collect()
    };
    cells.sort_by(|a, b| a.name().cmp(b.name()));
    for cell in cells {
        print_cell(&lib, &cell, rules);
    }
    ExitCode::SUCCESS
}
