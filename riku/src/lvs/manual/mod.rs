//! LVS manual y progresivo: qué transistor del esquemático es cuál del
//! layout lo dice el diseñador (o lo acepta de las sugerencias), queda en un
//! archivo versionado junto al diseño, y Riku comprueba lo que se deduce de
//! eso: los parámetros de cada par, las redes (un corto o un abierto
//! aparecen en cuanto dos vínculos se contradicen) y cuánto falta vincular.
//!
//! Un transistor del layout se nombra por su modelo y un punto de su
//! compuerta, en µm y en coordenadas de la celda comparada: no cambia si se
//! renumera nada ni si la celda se mueve en el chip. Si se movió todo dentro
//! de la celda, se busca el movimiento rígido que vuelve a alinear los
//! vínculos; lo que siga sin aparecer se busca por conectividad (el único
//! transistor del modelo correcto conectado a las redes ya vinculadas).
//!
//! La entrada es la netlist SPICE de cada lado (la misma que leería Netgen):
//! es el formato estándar y el único donde el modelo, W y L de cada
//! transistor ya están resueltos (después de expandir el `format` del
//! símbolo), con los sub-circuitos y sus parámetros. Por eso no hay otra
//! estructura en el netlister: serían dos representaciones que divergen.

mod check;
mod devices;
mod file;
mod index;
mod session;
mod suggest;
#[cfg(test)]
mod tests;

pub use check::*;
pub use devices::*;
pub use file::*;
use index::*;
pub use session::*;
pub use suggest::*;

/// Versión del archivo de vínculos.
pub const SCHEMA: &str = "riku-lvs-map/v1";

/// Distancia (µm) hasta la que un punto del archivo es la compuerta.
pub(super) const TOL: f64 = 0.01;
