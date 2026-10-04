//! Extracción jerárquica de redes y transistores, con memoria por huella
//! (ver `docs/ronda-5/design.md`).

mod build;
mod check;
mod key;
mod memo;
mod touch;
pub mod xf;

pub use build::{extract, flatten, locate, CellNets, Cells, HierNets, Inst, Options, Stats};
pub use check::same_netlist;
pub use key::NetKey;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices;
    use crate::style::Pdk;
    use gdstk_rs::Library;

    fn sram() -> Library {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/GDS/sram_16x8_sky130.gds");
        Library::from_bytes(&std::fs::read(&path).expect("sram")).expect("gds")
    }

    /// La SRAM de OpenRAM del repo (varios niveles, celdas de contacto, el
    /// `dnwell` arriba y el cierre del pozo P): por celdas da la misma
    /// netlist que aplanada.
    #[test]
    fn the_sram_example_gives_the_same_netlist_by_cells() {
        let lib = sram();
        let rules = devices::compiled(Pdk::Sky130).expect("sky130");
        let top = crate::select_top_cell(&lib).expect("top");
        let flat = crate::nets::cell_nets(&lib, &top, rules, None);
        let h = extract(&lib, &top, rules, None, Options { inline: 256 });
        let diffs = same_netlist(&flat, &h.flatten(false), lib.unit() / 1e-6, 20);
        assert!(diffs.is_empty(), "{diffs:#?}");
        assert!(h.stats.cells + h.stats.remembered > 10, "{:?}", h.stats);
        // Otra vez: todo sale de la memoria, con el mismo resultado.
        let again = extract(&lib, &top, rules, None, Options { inline: 256 });
        assert_eq!(again.stats.cells, 0, "{:?}", again.stats);
        assert!(same_netlist(&flat, &again.flatten(false), lib.unit() / 1e-6, 5).is_empty());
    }

    /// Sin meter nada en el padre, las celdas de contacto quedan solas y la
    /// netlist cambia: el umbral importa (ver `docs/ronda-5`).
    #[test]
    fn without_inlining_contact_cells_the_netlist_differs() {
        let lib = sram();
        let rules = devices::compiled(Pdk::Sky130).expect("sky130");
        let top = crate::select_top_cell(&lib).expect("top");
        let flat = crate::nets::cell_nets(&lib, &top, rules, None);
        let h = extract(&lib, &top, rules, None, Options { inline: 0 });
        assert!(!same_netlist(&flat, &h.flatten(false), lib.unit() / 1e-6, 1).is_empty());
    }
}
