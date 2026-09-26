//! Modelos de dominio de riku.
//!
//! Los tipos de cambio (`FileChange`, `Change`, `Element`, `ChangeKind`…) y
//! `FileFormat` son del núcleo (`riku-kernel`): el dominio ya no depende de
//! los tipos de ningún formato. Aquí solo quedan los tipos propios del VCS.

pub use riku_kernel::{Bounds, Change, ChangeKind, Detail, Element, FileChange, FileFormat, Value, Via};
