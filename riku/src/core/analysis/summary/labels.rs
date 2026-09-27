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
    let label = match key {
        COMPONENTS_ADDED if !plural => "componente añadido",
        COMPONENTS_ADDED => "componentes añadidos",
        COMPONENTS_REMOVED if !plural => "componente eliminado",
        COMPONENTS_REMOVED => "componentes eliminados",
        COMPONENTS_MODIFIED if !plural => "componente modificado",
        COMPONENTS_MODIFIED => "componentes modificados",
        COMPONENTS_RENAMED if !plural => "componente renombrado",
        COMPONENTS_RENAMED => "componentes renombrados",
        NETS_ADDED if !plural => "net añadida",
        NETS_ADDED => "nets añadidas",
        NETS_REMOVED if !plural => "net eliminada",
        NETS_REMOVED => "nets eliminadas",
        NETS_MODIFIED if !plural => "net modificada",
        NETS_MODIFIED => "nets modificadas",
        SIGNALS_ADDED if !plural => "señal nueva",
        SIGNALS_ADDED => "señales nuevas",
        SIGNALS_REMOVED if !plural => "señal eliminada",
        SIGNALS_REMOVED => "señales eliminadas",
        SIGNALS_MODIFIED if !plural => "señal cambió",
        SIGNALS_MODIFIED => "señales cambiaron",
        _ => return None,
    };
    Some(label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_for_singular_y_plural() {
        assert_eq!(label_for(COMPONENTS_ADDED, 1).unwrap(), "componente añadido");
        assert_eq!(
            label_for(COMPONENTS_ADDED, 3).unwrap(),
            "componentes añadidos"
        );
        assert_eq!(label_for("clave_desconocida", 1), None);
    }
}
