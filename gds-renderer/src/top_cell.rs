use gdstk_rs::{Cell, Library};

/// Elige la cell raiz a renderizar de una Library, con tie-break determinista.
///
/// - 0 top cells (library ciclica o vacia) -> `None`.
/// - 1 top cell -> esa.
/// - N>1 top cells -> la primera por nombre lexicografico ascendente.
///   Reproducible entre corridas, no requiere recorrer geometria.
pub fn select_top_cell<'a>(lib: &'a Library) -> Option<Cell<'a>> {
    let tops = lib.top_level();
    let count = tops.count();
    if count == 0 {
        return None;
    }
    if count == 1 {
        return Some(tops.cell(0));
    }
    let mut best_idx: u64 = 0;
    let mut best_name: String = tops.cell(0).name().to_string();
    for i in 1..count {
        let name = tops.cell(i).name().to_string();
        if name < best_name {
            best_name = name;
            best_idx = i;
        }
    }
    Some(tops.cell(best_idx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdstk_rs::Library;

    fn fixture(name: &str) -> Library {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Library::from_bytes(&bytes).expect("parse fixture")
    }

    #[test]
    fn picks_only_top_when_single() {
        let lib = fixture("top_single.gds");
        let top = select_top_cell(&lib).expect("Some(cell)");
        assert_eq!(top.name(), "ALPHA");
    }

    #[test]
    fn picks_alphabetical_min_when_multiple_tops() {
        let lib = fixture("top_multi.gds");
        let top = select_top_cell(&lib).expect("Some(cell)");
        // "ALPHA" < "BETA" < "ZETA" lexicograficamente.
        assert_eq!(top.name(), "ALPHA");
    }

    #[test]
    fn picks_root_in_nested_hierarchy() {
        let lib = fixture("top_nested.gds");
        let top = select_top_cell(&lib).expect("Some(cell)");
        // TOP es raiz de la cadena TOP -> INV -> GATE.
        assert_eq!(top.name(), "TOP");
    }
}
