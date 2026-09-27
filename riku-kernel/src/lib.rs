//! Núcleo de Riku.
//!
//! Define el vocabulario con el que los módulos de formato (Xschem, layouts
//! GDS/OASIS/Magic, simulaciones, …) le cuentan al resto del programa qué cambió entre dos
//! versiones de un archivo. No depende de ningún formato ni motor: los
//! módulos traducen sus tipos a estos y la CLI, el visor, `log` y `status`
//! solo conocen estos.
//!
//! - [`FileChange`]: todo lo que cambió en un archivo.
//! - [`Change`]: un cambio, con su [`Element`] tipado (sin convenciones de
//!   strings como `"net:X"` o `"TOP:L1/0:INV"`).
//! - [`FormatModule`] y [`Registry`]: el contrato de los módulos de formato y
//!   el registro donde se enchufan.
//! - [`legacy`]: la forma anterior (JSON v1) para una versión de transición.

mod change;
mod format;
pub mod legacy;
mod module;

pub use change::{Bounds, Change, ChangeKind, Detail, Element, FileChange, Value, Via};
pub use format::FileFormat;
pub use module::{DiffOptions, FormatModule, ModuleInfo, Registry};
// Otros archivos de la misma versión (formatos de varios archivos).
pub use viewer_core::{DiffFiles, DiskFiles, FileSource};
