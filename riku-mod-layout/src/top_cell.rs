use gdstk_rs::{Cell, Library};

/// Celda que no es parte del diseño: KLayout guarda en `$$$CONTEXT_INFO$$$`
/// (vacía) de qué PCells y librerías salen las celdas, y al leer la consume.
/// Riku la ignora en el diff, en la lista de celdas y al elegir la top: si
/// no, una versión guardada con KLayout y otra sin él tendrían una celda
/// "añadida", y como nadie la instancia, sería la top por orden alfabético.
pub(crate) fn is_meta_cell(name: &str) -> bool {
    name == "$$$CONTEXT_INFO$$$"
}

/// Elige la cell raiz a renderizar de una Library, con tie-break determinista.
///
/// - 0 top cells (library ciclica o vacia) -> `None`.
/// - 1 top cell -> esa.
/// - N>1 top cells -> la primera por nombre lexicografico ascendente.
///   Reproducible entre corridas, no requiere recorrer geometria.
///
/// Las celdas de metadatos ([`is_meta_cell`]) no cuentan.
pub(crate) fn select_top_cell<'a>(lib: &'a Library) -> Option<Cell<'a>> {
    let tops = lib.top_level();
    (0..tops.count())
        .map(|i| (i, tops.cell(i).name().to_string()))
        .filter(|(_, name)| !is_meta_cell(name))
        .min_by(|a, b| a.1.cmp(&b.1))
        .map(|(i, _)| tops.cell(i))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdstk_rs::Library;

    fn fixture(name: &str) -> Library {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
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
