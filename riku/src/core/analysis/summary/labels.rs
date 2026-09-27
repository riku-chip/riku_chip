//! Claves canónicas para el mapa `counts` y traducción a etiquetas humanas.
//!
//! Las constantes evitan typos cross-driver; `label_for` las traduce a texto
//! en singular/plural para el formateador. Claves no canónicas pasan sin
//! traducir y se imprimen tal cual (ver doc de [`super`]).

pub const COMPONENTS_ADDED: &str = "components_added";
pub const COMPONENTS_REMOVED: &str = "components_removed";
pub const COMPONENTS_MODIFIED: &str = "components_modified";
pub const COMPONENTS_RENAMED: &str = "components_renamed";
pub const NETS_ADDED: &str = "nets_added";
pub const NETS_REMOVED: &str = "nets_removed";
pub const NETS_MODIFIED: &str = "nets_modified";
/// Señales de una simulación (`.raw`).
pub const SIGNALS_ADDED: &str = "signals_added";
pub const SIGNALS_REMOVED: &str = "signals_removed";
pub const SIGNALS_MODIFIED: &str = "signals_modified";

/// Traduce una clave canónica a etiqueta corta humana (singular/plural).
/// Devuelve `None` si la clave no es canónica — el formateador puede entonces
/// imprimir la clave tal cual.
pub fn label_for(key: &str, count: i64) -> Option<String> {
    let plural = count.abs() != 1;
    let key = match key {
        COMPONENTS_ADDED if !plural => "label.component_added",
        COMPONENTS_ADDED => "label.components_added",
        COMPONENTS_REMOVED if !plural => "label.component_removed",
        COMPONENTS_REMOVED => "label.components_removed",
        COMPONENTS_MODIFIED if !plural => "label.component_modified",
        COMPONENTS_MODIFIED => "label.components_modified",
        COMPONENTS_RENAMED if !plural => "label.component_renamed",
        COMPONENTS_RENAMED => "label.components_renamed",
        NETS_ADDED if !plural => "label.net_added",
        NETS_ADDED => "label.nets_added",
        NETS_REMOVED if !plural => "label.net_removed",
        NETS_REMOVED => "label.nets_removed",
        NETS_MODIFIED if !plural => "label.net_modified",
        NETS_MODIFIED => "label.nets_modified",
        SIGNALS_ADDED if !plural => "label.signal_added",
        SIGNALS_ADDED => "label.signals_added",
        SIGNALS_REMOVED if !plural => "label.signal_removed",
        SIGNALS_REMOVED => "label.signals_removed",
        SIGNALS_MODIFIED if !plural => "label.signal_modified",
        SIGNALS_MODIFIED => "label.signals_modified",
        _ => return None,
    };
    Some(crate::i18n::tr!(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_for_singular_y_plural() {
        assert_eq!(label_for(COMPONENTS_ADDED, 1).unwrap(), "component added");
        assert_eq!(label_for(COMPONENTS_ADDED, 3).unwrap(), "components added");
        assert_eq!(label_for("clave_desconocida", 1), None);
    }
}
