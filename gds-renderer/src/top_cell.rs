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
