//! Vistas agregadas (`Summary`) sobre un `FileChange`.
//!
//! `riku status` y `riku log` necesitan presentar muchos archivos en una sola
//! pantalla. El `FileChange` completo es demasiado verboso para eso —
//! `FileSummary` es una agregación pensada para listas: pocas claves, fácil de
//! formatear en una línea, y categorizada (semantic / cosmetic / unchanged).
//!
//! El mapa `counts` es flexible a propósito: cada driver decide qué eventos
//! reporta (`components_added`, `nets_renamed`, `polygons_added_M1`, ...). El
//! formateador traduce las claves conocidas a etiquetas humanas; las que no
//! conoce las muestra tal cual. Eso permite añadir formatos sin tocar el core.
//!
//! Las claves canónicas aceptadas por el formateador de texto se documentan
//! en [`labels`].

mod build;
pub mod labels;
mod types;

pub use labels::label_for;
pub use types::{DetailEntry, DetailKind, DetailLevel, FileSummary, SummaryCategory};
